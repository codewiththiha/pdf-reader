//! Window-level interactions: Escape/outside dismiss, origin-exit collapse,
//! resize refresh, page-flip/zoom guards, and the outro's settle-unmount.
//! Each is a small self-contained hook so the popover stays wiring + view.

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, boxes_close};
use wasm_bindgen::JsCast;

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::util::viewport_size;
use crate::components::ai::types::GlossPhase;
use crate::state::AppState;

/// Escape collapses the expanded card; a second Escape on the bare chip gives
/// up on the gloss entirely. Outside taps collapse too.
pub fn use_dismiss_interactions(ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.surface_visible.get() {
            return;
        }

        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() != "Escape" {
                return;
            }
            match ctrl.gphase.get_untracked() {
                // First Escape closes the card (with the outro); a second one
                // on the bare chip gives up on the gloss entirely.
                GlossPhase::Expanded => ctrl.collapse_to_mark.run(()),
                _ => ctrl.reset.run(()),
            }
        });

        let pd = window_event_listener_untyped("pointerdown", move |ev: web_sys::Event| {
            // A press inside the surface is the card's own interaction.
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                && el.closest(".gloss-surface").ok().flatten().is_some()
            {
                return;
            }
            if ctrl.gphase.get_untracked() == GlossPhase::Expanded {
                ctrl.drag_offset.set(None);
                ctrl.gphase.set(GlossPhase::Compact);
            }
        });

        on_cleanup(move || {
            key.remove();
            pd.remove();
        });
    });
}

/// Scrolling does not kill the card instantly: it tracks its anchor until the
/// origin crosses CARD_EXIT_FRAC of the viewport height, leaves the top, or
/// its page is virtualized away — then it collapses back onto the mark.
pub fn use_origin_exit_collapse(watch: AnchorWatch, ctrl: GlossController) {
    Effect::new(move |_| {
        if !ctrl.surface_visible.get() {
            return;
        }
        if watch.exited.get() && ctrl.gphase.get() == GlossPhase::Expanded {
            ctrl.collapse_to_mark.run(());
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
    sprung: RwSignal<Option<GlossBox>>,
) {
    Effect::new(move |_| {
        if !ctrl.surface_visible.get() || ctrl.gphase.get() != GlossPhase::Compact {
            return;
        }
        let Some(a) = anchor.get() else {
            // The mark's page unmounted mid-morph: there is nothing left to
            // land on, so drop the surface now.
            ctrl.surface_visible.set(false);
            return;
        };
        if sprung.get().is_some_and(|b| boxes_close(b, a, 0.5)) {
            ctrl.surface_visible.set(false);
        }
    });
}

/// Keep the viewport size fresh while a surface exists, so placement clamps
/// stay honest through window resizes.
pub fn use_viewport_refresh(ctrl: GlossController, viewport: RwSignal<(f64, f64)>) {
    Effect::new(move |_| {
        if !ctrl.surface_visible.get() {
            return;
        }
        let h = window_event_listener_untyped("resize", move |_| {
            viewport.set(viewport_size());
        });
        on_cleanup(move || h.remove());
    });
}

/// A page flip collapses an expanded card back onto its mark (which may now
/// be off screen — the anchor still knows where it is).
pub fn use_page_flip_collapse(state: AppState, ctrl: GlossController) {
    Effect::new(move |_| {
        let _ = state.reader.viewer.page.get();
        if ctrl.surface_visible.get_untracked() {
            ctrl.collapse_to_mark.run(());
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
            ctrl.reset.run(());
        }
    });
}
