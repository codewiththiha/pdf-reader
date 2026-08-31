//! The horizontal-strip axis: dominant item → page counter, and page counter
//! → scroll position. Mirrors [`super::vertical`]; the two axes differ in the
//! view mode they answer to and in the alignment a jump uses (`Center`, so a
//! fling through the strip keeps the page mid-frame rather than pinned left).

use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, Virtualizer};

use crate::state::ReaderState;

use super::gate::JumpGate;
use super::{NavSyncState, page_from_dominant, scroll_mode};

/// The horizontal strip's dominant item drives the page counter.
pub(super) fn dominant_to_page(
    state: ReaderState,
    virtualizer: &Virtualizer,
    nav: &NavSyncState,
    mode: RwSignal<ViewMode>,
    zooming: Signal<bool>,
    gate: &Rc<JumpGate>,
) {
    let suppress = nav.suppress.clone();
    let gate = gate.clone();
    let page = state.viewer.page;
    let v = virtualizer.clone();
    let defer = nav.defer;
    Effect::new(move |_| {
        if mode.get() != ViewMode::ScrollHorizontal {
            return;
        }
        // Same restore guard as the vertical arm (see vertical.rs).
        if defer.get() {
            return;
        }
        let dominant = page_from_dominant(v.dominant().get(), state.document.num_pages.get());
        // Tracked for the same reason as the vertical arm: a follow keeps
        // the window frozen for a whole burst, and this is what catches the
        // page up when it lands.
        if zooming.get() {
            return;
        }
        // Defer to a held navigation for the closing flush (see the
        // vertical twin for the reasoning).
        if gate.pending().is_some() {
            return;
        }
        if page.get_untracked() == dominant {
            return;
        }
        suppress.set(true);
        page.set(dominant);
    });
}

/// A page write scrolls the horizontal strip to that page.
pub(super) fn page_to_scroll(
    state: ReaderState,
    virtualizer: &Virtualizer,
    nav: &NavSyncState,
    mode: RwSignal<ViewMode>,
    zooming: Signal<bool>,
    gate: &Rc<JumpGate>,
) {
    let suppress = nav.suppress.clone();
    let gate = gate.clone();
    let page = state.viewer.page;
    let v = virtualizer.clone();
    Effect::new(move |_| {
        if mode.get() != ViewMode::ScrollHorizontal {
            return;
        }
        // While a zoom transaction is in flight the anchor owns the
        // strip position; re-centering a page here would fight it. The
        // gate holds the write and replays it when the transaction lands.
        let page_now = page.get();
        let zooming = zooming.get();
        let Some((target, reassert)) = gate.admit(page_now, zooming) else {
            if zooming {
                suppress.set(false);
            }
            return;
        };
        if reassert {
            suppress.set(false);
            page.set(target);
        }
        if suppress.get() {
            suppress.set(false);
            return;
        }
        if target == 0 {
            return;
        }
        v.scroll_to_index((target - 1) as usize, Align::Center, scroll_mode(state));
    });
}
