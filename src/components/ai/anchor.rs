//! Shared page-space anchor watchers: glue a [`PageAnchor`] to the live page
//! host so the selection Info pill and the gloss card both follow scroll/zoom
//! and die when their origin leaves a configurable band of the viewport.
//!
//! The pure data type lives in `pdf_core::gloss::PageAnchor` so state can hold
//! it without depending on the component layer.

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, GlossMark};
use pdf_core::layout::ViewMode;
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::marks::MARK_RADIUS;
use crate::components::ai::gloss::util::{add_window_capture_listener, viewport_size};
use crate::components::document::dom_helpers::by_id;

// Single public binding — do not also `use` PageAnchor above or rustc E0252s.
pub use pdf_core::gloss::PageAnchor;

/// The selection "Info" pill lives until its origin fully leaves the viewport.
pub const MENU_EXIT_FRAC: f64 = 1.0;
/// The expanded gloss card tolerates scroll until its origin passes this
/// fraction of the viewport height (or leaves the top edge).
pub const CARD_EXIT_FRAC: f64 = 0.8;

pub fn host_id_for(page: u32, single: bool) -> String {
    if single {
        format!("sp-{page}-pg")
    } else {
        format!("cont-{}-pg", page.saturating_sub(1))
    }
}

/// Live viewport-space box for a page anchor. `None` when the scale is invalid
/// or the host page is not mounted (virtualized away) — which by itself counts
/// as "the anchor left the page".
pub fn screen_box(anchor: &PageAnchor, scale: f64, single: bool) -> Option<GlossBox> {
    if scale <= 0.0 {
        return None;
    }
    let hr = by_id(&host_id_for(anchor.page, single))?.get_bounding_client_rect();
    let h = anchor.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + anchor.rect.x * scale,
        y: hr.top() + anchor.rect.y * scale,
        w: anchor.rect.w * scale,
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

pub fn mark_screen_box(mark: &GlossMark, scale: f64, single: bool) -> Option<GlossBox> {
    screen_box(&PageAnchor::from_mark(mark), scale, single)
}

/// Capture the current DOM selection as a page-space anchor.
pub fn capture_selection(page: u32, scale: f64) -> Option<PageAnchor> {
    if scale <= 0.0 {
        return None;
    }
    let sel = web_sys::window()?.get_selection().ok()??;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    let host = el.closest(".pdf-page").ok().flatten()?;
    let hr = host.get_bounding_client_rect();
    let rects = range.get_client_rects()?;
    let (mut l, mut t, mut r, mut b) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut found = false;
    for i in 0..rects.length() {
        let Some(rc) = rects.get(i) else {
            continue;
        };
        if rc.width() <= 0.0 || rc.height() <= 0.0 {
            continue;
        }
        found = true;
        l = l.min(rc.left());
        t = t.min(rc.top());
        r = r.max(rc.right());
        b = b.max(rc.bottom());
    }
    if !found {
        return None;
    }
    Some(PageAnchor {
        page,
        rect: GlossBox {
            x: (l - hr.left()) / scale,
            y: (t - hr.top()) / scale,
            w: ((r - l) / scale).max(1.0),
            h: ((b - t) / scale).max(1.0),
            r: 0.0,
        },
    })
}

pub fn capture_selection_mark(
    page: u32,
    scale: f64,
    word: String,
    context: String,
) -> Option<GlossMark> {
    let a = capture_selection(page, scale)?;
    Some(GlossMark {
        id: format!("g{page}-{}", js_sys::Date::now() as u64),
        page,
        word,
        context,
        rect: a.rect,
    })
}

#[derive(Clone, Copy)]
pub struct AnchorWatch {
    /// Live viewport-space box of the anchor (None = page not mounted).
    pub screen: RwSignal<Option<GlossBox>>,
    /// Origin left the allowed band: above the viewport top, or below
    /// `exit_frac` of the viewport height (or the page unmounted).
    pub exited: RwSignal<bool>,
    /// Synchronous re-derive (reads the DOM now). Call before using `screen`
    /// inside the same tick that the mark changed.
    pub refresh: Callback<()>,
}

/// Reusable "glued to the page, dies when the origin leaves" behaviour.
///
/// The screen box is re-derived whenever scroll / zoom / view mode / page /
/// container size change (plus a capture-phase scroll listener so *any*
/// scroller is caught, and window resize). `exit_frac` is the fraction of the
/// viewport height the origin may reach before `exited` flips: `1.0` means
/// "fully out of the viewport", `0.8` means "past 80% of the height".
pub fn watch_page_anchor(
    anchor: Signal<Option<PageAnchor>>,
    scale: Signal<f64>,
    mode: Signal<ViewMode>,
    scroll_top: Signal<f64>,
    page: Signal<u32>,
    exit_frac: f64,
) -> AnchorWatch {
    let screen = RwSignal::new(None::<GlossBox>);
    let exited = RwSignal::new(false);
    let tick = RwSignal::new(0u32);

    let refresh = Callback::new(move |_| {
        let a = anchor.get_untracked();
        let s = scale.get_untracked();
        let single = mode.get_untracked() == ViewMode::Single;
        let b = a.as_ref().and_then(|a| screen_box(a, s, single));
        if screen.get_untracked() != b {
            screen.set(b);
        }
        let (_, vh) = viewport_size();
        let out = match b {
            None => true,
            Some(b) => (b.y + b.h) < 0.0 || b.y > vh * exit_frac,
        };
        if exited.get_untracked() != out {
            exited.set(out);
        }
    });

    Effect::new(move |_| {
        let _ = anchor.get();
        let _ = scale.get();
        let _ = mode.get();
        let _ = scroll_top.get();
        let _ = page.get();
        let _ = tick.get();
        refresh.run(());
    });

    add_window_capture_listener("scroll", move |_| tick.update(|n| *n += 1));
    let h = window_event_listener_untyped("resize", move |_| tick.update(|n| *n += 1));
    on_cleanup(move || h.remove());

    AnchorWatch {
        screen,
        exited,
        refresh,
    }
}
