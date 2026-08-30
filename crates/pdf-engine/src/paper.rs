//! The paper session: the `pdf-paper` crate's brain, wired to the engine's
//! eyes.
//!
//! Every colour decision — which pages to sample, what a page's paper is,
//! what the backdrop should show right now — lives in the pure crate and in
//! this state machine. The TS engine keeps only the pixel plumbing: it
//! stashes a raw frame per live render, renders offscreen samples on
//! request, remembers one cached colour per document path, and paints
//! whatever `--pdf-paper` it is told to.
//!
//! The lifecycle, in one breath:
//!
//! * [`configure`] — the reader's settings (blend on/off, mode, area, scan
//!   budget). A scan starts whenever the FIXED mode is live for a book with
//!   no colour yet; a detection-area change invalidates everything, because
//!   a histogram fed through one area says nothing about the other.
//! * [`document_open`] — a fresh book: reset, publish nothing until a colour
//!   is known, and ask the engine's cache in the background. A cache hit
//!   (under the SAME detection area) repaints instantly with zero sampling;
//!   a miss lets the scan run.
//! * [`live_frame`] — after every successful render, the one stashed raw
//!   frame feeds the per-page palette (continuous mode's raw material) and
//!   the interim colour (the fixed mode's stand-in until the scan lands).
//! * [`position`] — per scroll tick in continuous mode: the viewport's
//!   visible-paint-weighted mean page index. The palette interpolates along
//!   its ladder at exactly that point, so the backdrop meets the pages where
//!   they are — no seam at the dominant-page handover, which is what the
//!   old page-pair blend could not do.
//!
//! Every spawned task carries the session's generation token and re-checks
//! it after each `await`, so a sample or scan page started for one book can
//! never land in the next.

use std::cell::RefCell;

use wasm_bindgen_futures::spawn_local;

use pdf_paper::{PagePalette, PaperConfig, PaperDetector, PaperMode, Rgb, PAPER_SHARE};

use crate::api;
use crate::bridge;

struct Session {
    config: PaperConfig,
    blend_on: bool,
    doc_path: Option<String>,
    num_pages: u32,
    /// Per-page colours, fed by every frame (continuous mode's ladder).
    palette: PagePalette,
    /// The pooled fixed-scan detector — fed by SCAN frames only, so the
    /// book's colour is a deterministic function of the scan, not of which
    /// pages happened to be browsed.
    document: PaperDetector,
    /// The fixed colour, once a scan completes or the cache answers.
    fixed_final: Option<Rgb>,
    /// The first live page's colour — the fixed mode's interim until the
    /// scan lands, and the continuous mode's fallback at book open.
    interim: Option<Rgb>,
    /// The last colour handed to the engine. Unknown resolutions HOLD it
    /// (the backdrop must not flash), so it is cleared only deliberately.
    published: Option<String>,
    /// The reader's page-ladder position as of the last [`position`] call.
    position: f64,
    /// The fixed scan's resumable cursor: the next page to sample. `0`
    /// means no scan has started for this book (under this area).
    scan_cursor: u32,
    scanning: bool,
    /// Pages whose offscreen look-ahead sample is in flight.
    sampling: std::collections::HashSet<u32>,
    /// Generation token: bumped on document open/close and area change.
    epoch: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            config: PaperConfig::default(),
            blend_on: false,
            doc_path: None,
            num_pages: 0,
            palette: PagePalette::new(),
            document: PaperDetector::new(),
            fixed_final: None,
            interim: None,
            published: None,
            position: 1.0,
            scan_cursor: 0,
            scanning: false,
            sampling: std::collections::HashSet::new(),
            epoch: 0,
        }
    }
}

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::default());
}

/// Run `f` against the session. The borrow ends before any engine call.
fn with<R>(f: impl FnOnce(&mut Session) -> R) -> R {
    SESSION.with(|s| f(&mut s.borrow_mut()))
}

/// Spawn an engine-talking task, but ONLY when a real engine is attached.
/// Host `cargo test` has no JS runtime, so tasks that would talk to it
/// simply never start — the state-machine logic they drive is tested
/// directly instead.
fn spawn_engine<F: std::future::Future<Output = ()> + 'static>(f: impl FnOnce() -> F + 'static) {
    if bridge::has_pdf_reader() {
        spawn_local(async move {
            f().await;
        });
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// The reader's paper settings changed (or are being restated on mount).
///
/// `blend_on` gates the background scan — an idle reader with blend off
/// should not be rendering a hundred pages behind the shelf — while the
/// mode and area steer every decision the session makes.
pub fn configure(blend_on: bool, mut config: PaperConfig) {
    config.sanitize();
    let (area_changed, mode_now_continuous) = with(|s| {
        let area_changed = s.config.area != config.area;
        s.blend_on = blend_on;
        s.config = config;
        (area_changed, s.config.mode == PaperMode::Continuous)
    });
    if area_changed {
        // Everything colour-shaped was computed through the old area: drop
        // it all and let the frames re-detect. The published colour goes
        // too — the backdrop falls back to the theme paper for the moment
        // it takes the first new frame to land.
        let area = with(|s| {
            s.palette.clear();
            s.document.reset();
            s.fixed_final = None;
            s.interim = None;
            s.published = None;
            s.scan_cursor = 0;
            s.scanning = false;
            s.sampling.clear();
            s.epoch += 1;
            s.config.area
        });
        api::set_paper(None, false, area);
    }
    publish();
    start_scan_if_needed();
    if mode_now_continuous {
        ensure_lookahead();
    }
}

/// A document opened: reset for the new book, clear the shelf's colour, and
/// ask the engine's cache whether this book's paper is already known.
pub fn document_open(path: &str, num_pages: u32) {
    let epoch = with(|s| {
        s.epoch += 1; // abandon the previous book's in-flight samples/scans
        s.doc_path = Some(path.to_string());
        s.num_pages = num_pages;
        s.palette.clear();
        s.document.reset();
        s.fixed_final = None;
        s.interim = None;
        s.published = None;
        s.position = 1.0;
        s.scan_cursor = 0;
        s.scanning = false;
        s.sampling.clear();
        s.epoch
    });
    let open_area = with(|s| s.config.area);
    api::set_paper(None, false, open_area); // the previous book's colour must not linger

    let path = path.to_string();
    spawn_engine(move || async move {
        let area = with(|s| s.config.area);
        let cached = api::cached_paper(&path, area).await.ok().flatten();
        let should_scan = with(|s| {
            if s.epoch != epoch || s.doc_path.as_deref() != Some(path.as_str()) {
                return false; // the book changed under the lookup
            }
            match cached {
                // A valid cache hit IS the fixed colour: no scan, no work.
                // A scan that started before the lookup answered stops at
                // its next page (`scanning` goes false), never overwriting
                // the full-book answer with a partial one.
                Some(hit) => {
                    if let Some(colour) = Rgb::parse_hex(&hit.hex) {
                        s.fixed_final = Some(colour);
                        s.scan_cursor = u32::MAX; // the book counts as scanned
                        s.scanning = false;
                    }
                    false
                }
                None => scan_should_start(s),
            }
        });
        publish();
        if should_scan {
            start_scan();
        }
    });
}

/// The document closed (or the app is tearing down): forget everything and
/// drop the backdrop back to the theme paper.
pub fn document_close() {
    with(|s| {
        *s = Session {
            config: s.config,
            blend_on: s.blend_on,
            ..Session::default()
        };
    });
    let area = with(|s| s.config.area);
    api::set_paper(None, false, area); // the shelf shows the theme paper
}

/// A live render of `canvas_id` just completed: drain its stashed raw frame
/// into the palette (and the interim, while no fixed colour is known).
pub fn live_frame(canvas_id: &str) {
    if let Some(frame) = api::take_paper_frame(canvas_id) {
        feed_frame(&frame);
    }
}

/// The viewport's position along the page ladder (1-based, fractional; the
/// visible-paint-weighted mean page index). Per scroll tick in continuous
/// mode; harmless everywhere else. `pos <= 0` means "geometry unknown" and
/// holds the last position.
pub fn position(pos: f64) {
    if !pos.is_finite() || pos <= 0.0 {
        return;
    }
    let moved = with(|s| {
        let moved = (pos - s.position).abs() > f64::EPSILON;
        s.position = pos;
        moved
    });
    if moved {
        publish();
        ensure_lookahead();
    }
}

// ---------------------------------------------------------------------------
// Feeding & publishing
// ---------------------------------------------------------------------------

/// Feed one raw frame (live stash or offscreen sample) into the session.
fn feed_frame(frame: &api::PaperFrame) {
    let changed = with(|s| feed_state(s, frame));
    if changed {
        publish();
        ensure_lookahead();
    }
}

/// The state half of a feed, for in-borrow use. Returns whether anything
/// the publish reads has changed.
fn feed_state(s: &mut Session, frame: &api::PaperFrame) -> bool {
    if s.doc_path.is_none() || frame.width == 0 || frame.height == 0 {
        return false;
    }
    let mut page = PaperDetector::new();
    page.feed(
        s.config.area,
        frame.width as usize,
        frame.height as usize,
        &frame.data,
        s.config.edge_width as usize,
    );
    let Some(colour) = page.dominant(PAPER_SHARE) else {
        return false; // an artwork page has no paper to contribute
    };
    let mut changed = s.palette.get(frame.page) != Some(colour);
    s.palette.set(frame.page, colour);
    if s.fixed_final.is_none() && s.interim.is_none() {
        s.interim = Some(colour);
        changed = true;
    }
    changed
}

/// The colour the current mode resolves right now, if any.
fn resolve(s: &Session) -> Option<Rgb> {
    match s.config.mode {
        // Fixed: the scan's (or cache's) answer, with the interim standing
        // in until it lands.
        PaperMode::Fixed => s.fixed_final.or(s.interim),
        // Continuous: the palette's ladder at the reader's position, with
        // whatever the fixed side knows as the book-open fallback.
        PaperMode::Continuous => s.palette.colour_at(s.position).or(s.fixed_final.or(s.interim)),
    }
}

/// Hand the resolved colour to the engine — or clear it, but only when the
/// session is deliberately blank (no book, blend off). An UNKNOWN colour
/// holds what is already published: the backdrop must not flash to the
/// theme paper while a sample is still in flight.
fn publish() {
    publish_with(false);
}

/// [`publish`], optionally persisting a just-landed fixed colour.
fn publish_with(persist_final: bool) {
    let outcome = with(|s| {
        if s.doc_path.is_none() || !s.blend_on {
            return (None, s.published.take(), false, s.config.area);
        }
        match resolve(s).map(|c| c.to_hex()) {
            Some(hex) => {
                let persist = persist_final
                    && s.config.mode == PaperMode::Fixed
                    && s.fixed_final.is_some();
                let changed = s.published.as_deref() != Some(hex.as_str());
                if changed {
                    s.published = Some(hex.clone());
                }
                (Some(hex), None, changed || persist, s.config.area)
            }
            None => (None, None, false, s.config.area), // hold
        }
    });
    match outcome {
        (Some(hex), _, send, area) if send => {
            api::set_paper(Some(hex.as_str()), persist_final, area)
        }
        // Deliberate blank: no book / blend off — clear the backdrop.
        (None, Some(_), _, area) => api::set_paper(None, false, area),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// The fixed scan
// ---------------------------------------------------------------------------

/// Would a scan be useful right now? (Fixed mode live, a book open, no
/// final colour, no scan running.)
fn scan_should_start(s: &Session) -> bool {
    s.blend_on
        && s.config.mode == PaperMode::Fixed
        && s.doc_path.is_some()
        && s.fixed_final.is_none()
        && !s.scanning
}

fn start_scan_if_needed() {
    if !with(|s| scan_should_start(s)) {
        return;
    }
    start_scan();
}

/// Spawn the background scan: sample pages `scan_cursor..=cap` one at a
/// time (the engine yields between pages so live renders never queue
/// behind it), pooling every frame into the document detector, then settle
/// the book's colour. The cursor makes the scan RESUMABLE: cancelling it
/// (blend off, mode flip, book change) and starting again later continues
/// from the page after the last one fed.
fn start_scan() {
    let epoch = with(|s| {
        s.scanning = true;
        if s.scan_cursor == 0 {
            s.scan_cursor = 1;
        }
        s.epoch
    });
    spawn_engine(move || scan_task(epoch));
}

async fn scan_task(epoch: u64) {
    loop {
        // The next page to sample, or None when the scan is done/cancelled.
        let next = with(|s| {
            if s.epoch != epoch || !s.scanning {
                return None; // cancelled or superseded
            }
            let last = s.num_pages.min(s.config.scan_pages);
            if s.scan_cursor > last {
                return Some(0); // done
            }
            Some(s.scan_cursor)
        });
        let Some(page) = next else { return };
        if page == 0 {
            finish_scan(epoch);
            return;
        }
        let frame = api::sample_paper_page(page).await.ok().flatten();
        let changed = with(|s| {
            if s.epoch != epoch || !s.scanning {
                return false; // the book changed under the sample
            }
            let mut changed = false;
            if let Some(f) = &frame {
                s.document.feed(
                    s.config.area,
                    f.width as usize,
                    f.height as usize,
                    &f.data,
                    s.config.edge_width as usize,
                );
                changed = feed_state(s, f); // the scan fills the palette for free
            }
            s.scan_cursor = page + 1;
            changed
        });
        if changed {
            // A scan page can be the first frame the book ever offered (no
            // live render yet): the interim it establishes must publish.
            publish();
        }
    }
}

/// The scan covered its budget: settle the book's colour from the pooled
/// histogram (falling back to the interim for photo books with no paper
/// majority) and persist it — this is the ONE publish that writes the
/// per-document cache.
fn finish_scan(epoch: u64) {
    let done = with(|s| {
        if s.epoch != epoch {
            return false;
        }
        s.scanning = false;
        s.fixed_final = s.document.dominant(PAPER_SHARE).or(s.interim);
        s.fixed_final.is_some()
    });
    if done {
        publish_with(true);
    }
}

// ---------------------------------------------------------------------------
// The continuous look-ahead
// ---------------------------------------------------------------------------

/// The pages whose colour continuous mode wants known: the pair the reader
/// is straddling plus the one after it, so the colour is resolved before
/// the reader arrives. Pure — the test exercises exactly this choice.
fn lookahead_wants(s: &Session) -> Vec<u32> {
    if !s.blend_on || s.config.mode != PaperMode::Continuous || s.num_pages == 0 {
        return Vec::new();
    }
    let base = s.position.floor().max(1.0) as u32;
    let mut wants = Vec::new();
    for page in [base, base + 1, base + 2] {
        if (1..=s.num_pages).contains(&page)
            && !s.palette.contains(page)
            && !s.sampling.contains(&page)
        {
            wants.push(page);
        }
    }
    wants
}

/// Resolve (offscreen) the pages [`lookahead_wants`] names, one spawn each,
/// all generation-guarded so a sample for one book cannot land in the next.
fn ensure_lookahead() {
    let pages = with(|s| {
        let wants = lookahead_wants(s);
        for page in &wants {
            s.sampling.insert(*page);
        }
        wants
    });
    for page in pages {
        spawn_engine(move || async move {
            let epoch = with(|s| s.epoch);
            let frame = api::sample_paper_page(page).await.ok().flatten();
            let changed = with(|s| {
                if s.epoch != epoch {
                    return false;
                }
                s.sampling.remove(&page);
                match &frame {
                    Some(f) => feed_state(s, f),
                    None => false, // unreadable page: nothing to learn
                }
            });
            if changed {
                publish();
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Tests: the state machine runs on the host (bridge calls are guarded, so
// only the in-Rust transitions are exercised — the colour math itself is
// the pdf-paper crate's own test surface).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_paper::PaperArea;

    /// A uniform `w × h` frame of one colour.
    fn uniform(page: u32, w: u32, h: u32, colour: [u8; 3]) -> api::PaperFrame {
        let mut data = vec![255u8; (w * h * 4) as usize];
        for i in (0..data.len()).step_by(4) {
            data[i] = colour[0];
            data[i + 1] = colour[1];
            data[i + 2] = colour[2];
        }
        api::PaperFrame { page, width: w, height: h, data }
    }

    const CREAM: [u8; 3] = [0xfa, 0xf4, 0xe8];
    const INK: [u8; 3] = [0x40, 0x40, 0x40];
    const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

    fn reset_session(config: PaperConfig, blend_on: bool) {
        with(|s| {
            *s = Session {
                config,
                blend_on,
                ..Session::default()
            };
        });
    }

    fn published() -> Option<String> {
        with(|s| s.published.clone())
    }

    #[test]
    fn a_live_frame_publishes_the_interim_in_fixed_mode() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    #[test]
    fn the_scan_settles_the_pooled_colour_over_the_interim() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM)); // interim: cream

        let epoch = with(|s| s.epoch);
        with(|s| {
            // Pages 2..=10 are all ink: pooled, ink owns the book.
            for page in 2..=10 {
                let f = uniform(page, 32, 32, INK);
                s.document.feed(PaperArea::WholePage, 32, 32, &f.data, 10);
            }
        });
        finish_scan(epoch);
        assert_eq!(published().as_deref(), Some("#404040"));
    }

    #[test]
    fn a_position_straddling_pages_blends_their_shares() {
        // THE continuous-mode regression: 40% page 1 + 60% page 2 must read
        // as 60% of page 2's colour — the old pair blend snapped to the
        // dominant page's colour instead, which read as a mismatch.
        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, WHITE));
        position(1.6);
        let want = pdf_paper::lerp(
            Rgb::new(CREAM[0], CREAM[1], CREAM[2]),
            Rgb::new(WHITE[0], WHITE[1], WHITE[2]),
            0.6,
        )
        .to_hex();
        assert_eq!(published().as_deref(), Some(want.as_str()));
    }

    #[test]
    fn resting_on_a_page_publishes_exactly_its_colour() {
        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        feed_frame(&uniform(2, 32, 32, INK));
        position(2.0);
        assert_eq!(published().as_deref(), Some("#404040"));
    }

    #[test]
    fn an_artwork_frame_contributes_nothing_and_holds_the_published() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        // An artwork page: sixteen distinct colour bands, each 6.25% of the
        // pixels — no bucket reaches the 10% paper share, so the page has
        // no colour to contribute and the backdrop holds what it had.
        let mut art = uniform(2, 32, 32, CREAM);
        for y in 0..32usize {
            for band in 0..16u8 {
                for x in (band as usize * 2)..(band as usize * 2 + 2) {
                    let i = (y * 32 + x) * 4;
                    art.data[i] = band.wrapping_mul(16);
                    art.data[i + 1] = 255 - band.wrapping_mul(15);
                    art.data[i + 2] = band.wrapping_mul(7).wrapping_add(3);
                }
            }
        }
        feed_frame(&art);
        assert_eq!(published().as_deref(), Some("#faf4e8"));
        assert!(!with(|s| s.palette.contains(2)));
    }

    #[test]
    fn an_area_change_invalidates_everything() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published().as_deref(), Some("#faf4e8"));

        configure(
            true,
            PaperConfig {
                area: PaperArea::Edges,
                ..PaperConfig::default()
            },
        );
        assert_eq!(published(), None); // cleared: re-detect through the new area
        assert!(with(|s| s.palette.is_empty()));
        // A fresh scan is armed from page 1 under the new area (the task
        // itself only spawns against a real engine).
        assert_eq!(with(|s| s.scan_cursor), 1);
        assert!(with(|s| s.scanning));
    }

    #[test]
    fn blend_off_never_publishes() {
        reset_session(PaperConfig::default(), false);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        assert_eq!(published(), None);
    }

    #[test]
    fn closing_the_document_forgets_the_book() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        document_close();
        assert_eq!(published(), None);
        assert!(with(|s| s.palette.is_empty() && s.doc_path.is_none()));
    }

    #[test]
    fn the_lookahead_names_the_pair_and_the_page_after() {
        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(3, 32, 32, CREAM)); // page 3 known (a live frame)
        // Set the position directly: `position()` would mark the wanted
        // pages as in-flight (spawned samples), which is exactly what the
        // NEXT assertion must not see.
        with(|s| s.position = 3.0);
        let wants = with(|s| lookahead_wants(s));
        assert_eq!(wants, vec![4, 5]); // 3 is known; the pair's next page +1
    }

    #[test]
    fn the_lookahead_stops_at_the_last_page() {
        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 3);
        with(|s| s.position = 3.0);
        assert_eq!(with(|s| lookahead_wants(s)), vec![3]); // page 3, nothing after
    }

    #[test]
    fn the_lookahead_is_quiet_in_fixed_mode_and_when_blend_is_off() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        with(|s| s.position = 1.0);
        assert!(with(|s| lookahead_wants(s)).is_empty());

        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            false,
        );
        assert!(with(|s| lookahead_wants(s)).is_empty());
    }

    #[test]
    fn a_cache_hit_counts_as_scanned_without_a_task() {
        reset_session(PaperConfig::default(), true);
        document_open("/fake/book.pdf", 10);
        with(|s| {
            s.fixed_final = Some(Rgb::new(0xfa, 0xf4, 0xe8));
            s.scan_cursor = u32::MAX;
        });
        assert!(!with(|s| scan_should_start(s))); // fixed_final is set: no scan
        publish();
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }

    #[test]
    fn continuous_falls_back_to_the_interim_at_book_open() {
        // Continuous mode with an empty palette (samples still in flight):
        // the first live colour holds the backdrop instead of flashing.
        reset_session(
            PaperConfig {
                mode: PaperMode::Continuous,
                ..PaperConfig::default()
            },
            true,
        );
        document_open("/fake/book.pdf", 10);
        feed_frame(&uniform(1, 32, 32, CREAM));
        position(4.2); // nothing sampled that far yet
        assert_eq!(published().as_deref(), Some("#faf4e8"));
    }
}
