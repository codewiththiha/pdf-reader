//! Window-level interactions: Escape handling, outside-press dismissal,
//! origin-exit collapse, page-flip/zoom guards, and the outro's
//! settle-unmount. Each is a small self-contained hook so the popover stays
//! wiring + view; the generic part of dismissal (outside press, topmost
//! Escape, exclusion selectors) is the primitive `use_dismiss`, and only the
//! two-step gloss semantics (first Escape collapses, second gives up) stay
//! here.

use leptos::prelude::*;
use pdf_core::gloss::{boxes_close, GlossBox};
use wasm_bindgen::JsCast;

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::controller::GlossController;
use crate::components::primitives::floating::dismiss::{use_dismiss, DismissPolicy, DismissTrigger};
use crate::components::primitives::hooks::use_viewport::viewport_size;
use crate::components::ai::types::GlossPhase;
use crate::state::AppState;

/// Escape collapses the expanded card; a second Escape on the bare chip gives
/// up on the gloss entirely. Outside presses collapse too (only while
/// expanded — a bare chip is reachable by design).
pub fn use_dismiss_interactions(ctrl: GlossController) {
    // Two-step Escape: the gloss owns its own meaning (collapse → reset), so
    // the primitive is used for the outside-press half and the Escape half
    // stays here as domain policy.
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() != "Escape" {
                return;
            }
            match ctrl.geometry.gphase.get_untracked() {
                // First Escape closes the card (with the outro); a second one
                // on the bare chip gives up on the gloss entirely.
                GlossPhase::Expanded => ctrl.commands.collapse_to_mark.run(()),
                _ => ctrl.commands.reset.run(()),
            }
        });
        on_cleanup(move || key.remove());
    });

    // A press inside the surface is the card's own interaction; anywhere else
    // collapses an expanded card (compact chips stay put).
    use_dismiss(
        ctrl.geometry.surface_visible.into(),
        ctrl.commands.collapse_to_mark,
        DismissPolicy {
            escape: false,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: vec![".gloss-surface"],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );
}

/// Scrolling does not kill the card instantly: it tracks its anchor until the
/// origin crosses CARD_EXIT_FRAC of the viewport height, leaves the top, or
/// its page is virtualized away — then it collapses back onto the mark.
///
/// The band only closes a card whose origin was INSIDE the band at some point
/// while open (`exit_armed`). A card opened near the bottom edge starts with
/// its origin already past the band; collapsing it on spawn is what made low
/// opens read as "the card can't decide where to spawn". Those cards get the
/// hard exit instead: page unmounted, or origin fully out of the viewport.
pub fn use_origin_exit_collapse(watch: AnchorWatch, ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Expanded {
            return;
        }

        // Hard exit: the mark's page unmounted, or the origin fully left the
        // viewport (top or bottom). Collapses no matter how the card opened.
        let (_, vh) = viewport_size();
        let gone = match watch.screen.get() {
            None => true,
            Some(b) => (b.y + b.h) < 0.0 || b.y > vh,
        };
        if gone {
            ctrl.commands.collapse_to_mark.run(());
            return;
        }

        // Soft band: arm while the origin is inside; only an armed card is
        // collapsed by the band. Opened low → unarmed → survives; scroll up
        // (arms) then back down past the band → collapses, as designed.
        if !watch.exited.get() {
            ctrl.geometry.exit_armed.set(true);
            return;
        }
        if ctrl.geometry.exit_armed.get_untracked() {
            ctrl.commands.collapse_to_mark.run(());
        }
    });
}

/// The outro's hand-off: once the collapsing surface has morphed down onto
/// the anchor, unmount it and let the in-page stroke take over. Doing this on
/// SETTLE rather than on a timer is what keeps the two from being visible at
/// once (the stroke is drawn on the same exact-fit box the surface lands on).
pub fn use_settle_unmount(
    ctrl: GlossController,
    anchor: Signal<Option<GlossBox>>,
    sprung: Signal<Option<GlossBox>>,
) {
    Effect::new(move |_| {
        if !ctrl.geometry.surface_visible.get() || ctrl.geometry.gphase.get() != GlossPhase::Compact {
            return;
        }
        let Some(a) = anchor.get() else {
            // The mark's page unmounted mid-morph: there is nothing left to
            // land on, so drop the surface now.
            ctrl.geometry.surface_visible.set(false);
            return;
        };
        if sprung.get().is_some_and(|b| boxes_close(b, a, 0.5)) {
            ctrl.geometry.surface_visible.set(false);
        }
    });
}

/// A page flip collapses an expanded card back onto its mark (which may now
/// be off screen — the anchor still knows where it is).
pub fn use_page_flip_collapse(state: AppState, ctrl: GlossController) {
    Effect::new(move |_| {
        let _ = state.reader.viewer.page.get();
        if ctrl.geometry.surface_visible.get_untracked() {
            ctrl.commands.collapse_to_mark.run(());
        }
    });
}

/// A zoom re-renders the textLayer; the mark survives it, but the open card
/// would slide under the reader's hands mid-gesture — close it and leave the
/// highlight behind.
pub fn use_zoom_reset(state: AppState, ctrl: GlossController) {
    Effect::new(move |_| {
        if !state.reader.viewer.zoom_animating.get() {
            return;
        }
        if state.reader.ai_selection.popover_open.get_untracked() {
            ctrl.commands.reset.run(());
        }
    });
}
