//! Large-document cost of the two remaining hot paths the review asked
//! to measure rather than redesign:
//!
//!   1. Zoom `relayout_to` per animation frame
//!      in-place height scale + `DocumentLayout` rebuild + `anchored_scroll`
//!   2. Thumbnail-grid + continuous-view window queries
//!      (the math half of "open thumbnails on a 1,000-page PDF")
//!
//! Page counts: 100 / 500 / 1_000 / 5_000.
//!
//! Run with: `cargo bench -p pdf-core --bench large_doc --release`
//!
//! Verdict rule (from the review): if a 5k-page zoom frame stays well
//! under a 16 ms budget, leave the zoom path alone.

use std::hint::black_box;
use std::time::{Duration, Instant};

use pdf_core::layout::{visible_grid_rows, DocumentLayout, PAGE_GAP, RENDER_BUDGET};

const SIZES: &[usize] = &[100, 500, 1_000, 5_000];
const FRAME_BUDGET: Duration = Duration::from_millis(16);

fn mixed_heights(n: usize, scale: f64) -> Vec<f64> {
    (0..n)
        .map(|i| {
            let h = match i {
                _ if i % 37 == 0 => 612.0,
                _ if i % 13 == 0 => 842.0,
                _ if i % 7 == 0 => 1008.0,
                _ => 792.0,
            };
            h * scale
        })
        .collect()
}

/// One animation frame of `relayout_to`:
///   * `anchored_scroll` on the cached layout (O(log n))
///   * in-place scale of the height column (O(n), no alloc)
///   * rebuild the prefix-sum layout (O(n), the remaining allocation)
fn relayout_frame(
    heights: &mut [f64],
    layout: &DocumentLayout,
    factor: f64,
    scroll: f64,
    vh: f64,
) -> (DocumentLayout, f64) {
    let new_st = layout
        .anchored_scroll(scroll, vh, factor, vh * 0.5)
        .unwrap_or(scroll);
    for h in heights.iter_mut() {
        *h *= factor;
    }
    (DocumentLayout::new(heights, PAGE_GAP), new_st)
}

fn bench_zoom(n: usize) {
    let vh = 800.0;
    let mut heights = mixed_heights(n, 1.0);
    let mut layout = DocumentLayout::new(&heights, PAGE_GAP);
    // Park the viewport centre inside a page ~80% of the way through.
    let idx = (n * 4 / 5).saturating_sub(1);
    let mut scroll = layout.page_top(idx) + layout.height(idx) * 0.5 - vh * 0.5;

    // Warm up so the first-iteration allocator noise is not the story.
    for _ in 0..8 {
        let (next, st) = relayout_frame(&mut heights, &layout, 1.01, scroll, vh);
        layout = next;
        scroll = st;
    }

    // A ~200 ms zoom animation at 60 fps is ~12 frames. Time many more
    // so the per-frame number is stable, then report the mean.
    let frames = 240usize;
    let t = Instant::now();
    for i in 0..frames {
        // Alternate in/out so we don't blow past the clamp.
        let factor = if i % 2 == 0 { 1.05 } else { 1.0 / 1.05 };
        let (next, st) = relayout_frame(
            black_box(&mut heights),
            black_box(&layout),
            factor,
            scroll,
            vh,
        );
        layout = next;
        scroll = st;
    }
    let elapsed = t.elapsed();
    let per = elapsed / frames as u32;
    let status = if per < FRAME_BUDGET { "OK " } else { "HOT" };
    println!(
        "  zoom relayout_to   n={n:5}  {frames} frames  {elapsed:>10.2?}  {per:>8.2?}/frame  {status} (16ms budget)"
    );
    black_box((layout.total(), scroll));
}

fn bench_windows(n: usize) {
    let heights = mixed_heights(n, 1.0);
    let layout = DocumentLayout::new(&heights, PAGE_GAP);
    let vh = 800.0;
    let queries = 2_000usize;

    // Continuous-view render window (what PageList asks every scroll tick).
    let t = Instant::now();
    let mut acc = 0usize;
    for i in 0..queries {
        let st = (i as f64 * 37.0) % layout.total().max(1.0);
        if let Some((f, l)) = layout.window(st, vh, RENDER_BUDGET) {
            acc += l - f + 1;
        }
    }
    let win_t = t.elapsed();

    // Thumbnail grid: 2 columns, letter aspect, 720 px panel (MIN_VIEWPORT_H).
    let rows = n.div_ceil(2);
    let row_h = 120.0 * (792.0 / 612.0) + 8.0;
    let t = Instant::now();
    let mut mounted = 0usize;
    for i in 0..queries {
        let st = (i as f64 * 17.0) % ((rows as f64 * row_h).max(1.0));
        if let Some((f, l)) = visible_grid_rows(st, 720.0, rows, row_h, 2) {
            mounted += l - f + 1;
        }
    }
    let grid_t = t.elapsed();
    let avg_mounted = mounted as f64 / queries as f64;

    println!(
        "  windows            n={n:5}  page-window x{queries} {win_t:>10.2?}  thumb-rows x{queries} {grid_t:>10.2?}  avg mounted rows {avg_mounted:.1}  (acc {acc})"
    );
}

fn main() {
    println!();
    println!("pdf-core large-document bench");
    println!("zoom = one relayout_to frame (in-place scale + layout rebuild + anchor)");
    println!("windows = continuous render window + thumbnail-grid virtualization");
    println!();
    for &n in SIZES {
        bench_zoom(n);
        bench_windows(n);
        println!();
    }
}
