//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)` — `Auto` resolves to a glide,
//!   and the reader's scroll switch is what decides whether it may (see
//!   `scroll_mode`)
//! - mode flip -> scroll: `scroll_to_index(Start, Instant)` when re-entering
//!   continuous mode
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends.
//!
//! A page write that arrives WHILE a transaction holds the geometry (the
//! resume jump on open, an outline click during a fit slide, a search hit
//! mid-gesture) is not dropped, though — it is held by [`JumpGate`] and
//! replayed on the frame the transaction closes. Without that replay, every
//! open from the library lost its resume point: the reader mounts, its
//! container's first measure opens a fit transaction, the resume jump lands
//! one frame later inside it, and the dominant arm "corrected" the page back
//! to 1 — which reading_progress then dutifully saved over the real position.
//!
//! LAYOUT. Each axis owns a pair of one-way arms, and both axes share the
//! gate:
//!
//!   * [`gate`]       — [`JumpGate`], the held-write replay and its tests
//!   * [`vertical`]   — the two continuous-scroll arms
//!   * [`horizontal`] — the two horizontal-strip arms
//!
//! This file keeps the wiring plus the two pure helpers both axes use.

mod gate;
mod horizontal;
mod vertical;

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::ReaderState;

use super::on_mode_change::on_mode_change;

pub(super) use gate::JumpGate;

/// Shared suppression flag for the two one-way syncs.
pub(super) struct NavSyncState {
    pub suppress: Rc<Cell<bool>>,
    /// A mode restore is in flight: the scroll→page sync must stand down so it
    /// does not read the not-yet-restored strip and clobber the preserved page.
    ///
    /// A REACTIVE signal on purpose, not a `Cell`. The dominant arm reads it
    /// as a dependency and returns early while it is up; when the restore
    /// lands and it falls, that write re-runs the arm against the now-true
    /// dominant. A `Cell` hand-off left the arm with no reason to re-run after
    /// a mode flip, so the page counter sat on the pre-restore value until the
    /// reader scrolled. Making it reactive closes that without re-introducing
    /// the read-a-strip-mid-restore race the flag exists to prevent.
    pub defer: RwSignal<bool>,
}

impl NavSyncState {
    fn new() -> Self {
        Self {
            suppress: Rc::new(Cell::new(false)),
            defer: RwSignal::new(false),
        }
    }
}

/// The page a strip's dominant item (0-based index) corresponds to, SAFELY.
///
/// The naive `dominant as u32 + 1` is a real footgun: a strip that has not
/// yet resolved a window (a freshly-mounted mode flip, a mid-fit measure)
/// can report a sentinel or out-of-range index, and if that index is
/// `usize::MAX` the `as u32 + 1` WRAPS to 0 — so a view-mode change would
/// reset the reader to page 0, which reading-progress then persisted over
/// the real position. Clamping to `[1, page_count]` makes a momentary
/// no-window read harmless instead of destructive.
pub(super) fn page_from_dominant(dominant: usize, num_pages: u32) -> u32 {
    let raw = dominant.saturating_add(1) as u64;
    raw.clamp(1, u64::from(num_pages.max(1))) as u32
}

/// How a page-to-scroll jump should travel: gliding while the reader's scroll
/// switch allows it, in one step when it does not. Read UNTRACKED, because the
/// flag that stops a jump gliding must not be what re-runs the jump.
pub(super) fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn navigation_sync(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let nav = NavSyncState::new();
    let mode = state.viewer.mode;
    // The tracked form of "a zoom transaction is in flight", for the effects
    // that must RESYNC when one lands (below); the untracked
    // `zooming_now()` stays on the ones that must only not fight it.
    let zooming = state.viewer.zooming();
    // One gate per axis: its page→scroll arm holds navigation writes that
    // arrive mid-transaction, and its dominant arm defers to the held write
    // on the flush that closes the transaction (see JumpGate).
    let gate = Rc::new(JumpGate::new());
    let h_gate = Rc::new(JumpGate::new());

    // The mode-restore flag is shared with on_mode_change, which raises it
    // during a mode flip and lowers it once the restored scroll has landed.
    on_mode_change(state, virtualizer.clone(), h_virtualizer.clone(), nav.defer);

    vertical::dominant_to_page(state, &virtualizer, &nav, mode, zooming, &gate);
    vertical::page_to_scroll(state, &virtualizer, &nav, mode, zooming, &gate);
    horizontal::dominant_to_page(state, &h_virtualizer, &nav, mode, zooming, &h_gate);
    horizontal::page_to_scroll(state, &h_virtualizer, &nav, mode, zooming, &h_gate);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The view-mode-change regression: a strip that reports a sentinel index
    /// (no window resolved yet) maps to a real page, never to 0.
    #[test]
    fn a_sentinel_dominant_index_never_becomes_page_zero() {
        // usize::MAX used to wrap to 0 through `as u32 + 1`.
        assert_eq!(page_from_dominant(usize::MAX, 300), 300);
        // A truly empty strip reads as page 1 (the first page), not 0.
        assert_eq!(page_from_dominant(0, 300), 1);
        // An index beyond the book clamps to the last page.
        assert_eq!(page_from_dominant(999, 50), 50);
        // With no pages known yet, everything clamps to page 1.
        assert_eq!(page_from_dominant(999, 0), 1);
        // Ordinary indices round-trip.
        assert_eq!(page_from_dominant(41, 300), 42);
    }
}
