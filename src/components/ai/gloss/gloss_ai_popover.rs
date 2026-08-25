//! The gloss popover: wiring + view. The state machine lives in
//! [`controller`], targeting math in [`placement`], pointer physics in
//! [`drag`], window behaviour in [`interactions`], and stream/measure hooks
//! in [`hooks`].
//!
//! Two orthogonal phases run together:
//! * the **geometry phase** ([`GlossPhase`]): stroke → expanded card → chip;
//! * the **data phase** ([`AiPhase`]): processing → streaming → done/error.
//!
//! Four things are load-bearing here and easy to undo by accident:
//!
//! * **The anchor is a [`GlossMark`], never a DOM node.** Every scroll, zoom,
//!   page or mode change re-projects the mark's page-space rect through
//!   whichever host currently renders that page, so the card sticks to the
//!   *text* even across the virtualizer's unmounts. The mark is written to
//!   localStorage at capture time and is deliberately NOT removed on dismiss:
//!   closing the card leaves the word highlighted, and clicking that highlight
//!   re-opens the card through the same spring.
//! * **There is exactly one highlighter at a time.** The native `::selection`
//!   tint is cleared the moment the gloss takes over; while the model works
//!   there is NO surface at all (the in-page stroke thinks via drift/sweep/halo);
//!   and after the outro the surface unmounts once it has settled onto the
//!   stroke, so a chip can never sit on top of the mark it came from.
//! * **Every close is an outro, not a cut.** Origin-exit, Escape and outside
//!   clicks all run `collapse_to_mark`, and the spring is NOT snapped while
//!   compact, so the card visibly morphs back down onto the word before
//!   handing over to the persisted stroke.
//! * **Re-opening is recall, not a rescan.** Snapshots are cached by mark id,
//!   so clicking a stroke morphs the card open on `AiPhase::Done` content
//!   without touching the backend. The spring is hard-reset onto the new
//!   word's mark on every open so the morph never flies in from the previous
//!   card's resting place.

use leptos::prelude::*;

use crate::components::ai::anchor::{watch_page_anchor, PageAnchor, CARD_EXIT_FRAC};
use crate::components::ai::gloss::context_menu::GlossContextMenu;
use crate::components::ai::gloss::controller::{
    use_gloss_controller, use_open_effect, use_open_listener,
};
use crate::components::ai::gloss::drag::use_card_drag;
use crate::components::ai::gloss::hooks::use_ai_chunks::use_ai_chunks;
use crate::components::ai::gloss::hooks::use_content_measure::use_content_measure;
use crate::components::ai::gloss::interactions::{
    use_dismiss_interactions, use_origin_exit_collapse, use_page_flip_collapse, use_settle_unmount,
    use_zoom_reset,
};
use crate::components::ai::gloss::placement::{CARD_WIDTH, expanded_target, spring_target};
use crate::components::ai::gloss::selection_bar::GlossSelectBar;
use crate::components::ai::gloss::selection_mode::use_select_mode;
use crate::components::ai::gloss::gloss_surface::GlossSurface;
use crate::components::ai::gloss::undo_toast::GlossUndoToast;
use crate::components::ai::types::{AiError, AiPhase, GlossPhase};
use crate::components::ai::word_info::{LoadingShimmer, WordInfoSections};
use crate::components::primitives::hooks::use_viewport::use_viewport;
use crate::components::primitives::motion::reduced_motion::reduced_motion_signal;
use crate::components::primitives::motion::spring::{use_spring_box, SpringBox};
use crate::state::AppState;
use pdf_core::gloss::GlossBox;

#[component]
pub fn GlossAiPopover(state: AppState) -> impl IntoView {
    // ── State machine hub ─────────────────────────────────────────────
    let ctrl = use_gloss_controller(state);

    // ONE shared, page-aware anchor: follows scroll/zoom/mode/page, and
    // flags `exited` once the origin passes CARD_EXIT_FRAC of the viewport
    // height (or leaves the top, or its page unmounts).
    let watch = watch_page_anchor(
        Signal::derive(move || ctrl.mark_sig.get().map(|m| PageAnchor::from_mark(&m))),
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.mode.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        CARD_EXIT_FRAC,
    );
    let anchor = watch.screen;

    // Reactive viewport (the shared primitive): resize-aware signal, owned by
    // this reactive scope. Replaces the old local snapshot + refresh listener.
    let viewport = use_viewport();
    let reduced = reduced_motion_signal();

    // ── Card targeting ────────────────────────────────────────────────
    let (measure_ref, content_height) = use_content_measure(ctrl.word, ctrl.word_info);
    let expanded = expanded_target(anchor.into(), content_height, viewport);
    let target = spring_target(
        anchor.into(),
        ctrl.gphase,
        ctrl.drag_offset,
        expanded,
        viewport,
    );

    // Snapping while compact was what made closing read as a cut: the spring
    // teleported the surface onto the anchor instead of morphing down to it.
    // Only the processing phase (where no surface exists anyway) snaps.
    let snap = Signal::derive(move || {
        ctrl.dragging.get() || reduced.get() || ctrl.gphase.get() == GlossPhase::Processing
    });
    let spring: SpringBox<GlossBox> = use_spring_box(target.into(), snap);
    let sprung = spring.value;

    let progress = Memo::new(move |_| {
        let (Some(b), Some(a), Some(e)) = (sprung.get(), anchor.get(), expanded.get()) else {
            return if ctrl.gphase.get() == GlossPhase::Expanded {
                1.0
            } else {
                0.0
            };
        };
        ((b.w - a.w) / (e.w - a.w).max(1.0)).clamp(0.0, 1.0)
    });

    // ── Open/close plumbing ───────────────────────────────────────────
    use_open_listener(state, ctrl);
    use_open_effect(state, ctrl, watch, spring, viewport);
    use_ai_chunks(state, ctrl);

    // ── Window-level behaviour ────────────────────────────────────────
    use_dismiss_interactions(ctrl);
    use_origin_exit_collapse(watch, ctrl);
    use_settle_unmount(ctrl, anchor.into(), sprung.into());
    use_page_flip_collapse(state, ctrl);
    use_zoom_reset(state, ctrl);

    // ── Drag physics ──────────────────────────────────────────────────
    let drag = use_card_drag(ctrl, expanded);

    // ── Mark management (selection mode, context menu, undo) ──────────
    let sm = use_select_mode(state, ctrl);

    // ── Surface props (unwrapped — it only renders while visible) ─────
    let phase_sig = Signal::derive(move || ctrl.gphase.get());
    let box_sig = Signal::derive(move || sprung.get().unwrap_or_default());
    let expanded_sig = Signal::derive(move || expanded.get().unwrap_or_default());
    let progress_sig = Signal::derive(move || progress.get());
    let word_sig = Signal::derive(move || ctrl.word.get());

    view! {
        // Invisible measure twin — pixel-exact replica of the scroll column
        // in GlossSurface (same width, px-5/pt-6/pb-4, header, separator),
        // so the measured height already includes chrome and wrap.
        <div
            node_ref=measure_ref
            class=format!("pointer-events-none invisible fixed left-0 top-0 {}", crate::components::primitives::floating::types::z::CONTENT)
            style=format!("width:{CARD_WIDTH}px")
            aria-hidden="true"
        >
            <div class="px-5 pb-4 pt-6">
                <header class="mb-4">
                    <h2 class="text-lg font-semibold leading-tight text-balance text-ink">
                        {move || ctrl.word.get()}
                    </h2>
                </header>
                <div class="mb-4 h-px"></div>
                {move || ctrl.word_info.get().map(|info| view! { <WordInfoSections info=info /> })}
            </div>
        </div>

        <Show when=move || ctrl.surface_visible.get() && ctrl.phase.get() != AiPhase::Idle>
            <GlossSurface
                phase=phase_sig
                box_=box_sig
                expanded=expanded_sig
                progress=progress_sig
                word=word_sig
                on_drag_start=drag.on_drag_start
            >
                {move || match ctrl.phase.get() {
                    AiPhase::Processing => view! { <LoadingShimmer /> }.into_any(),
                    AiPhase::Streaming | AiPhase::Done => match ctrl.word_info.get() {
                        Some(info) => view! { <WordInfoSections info=info /> }.into_any(),
                        None => view! { <LoadingShimmer /> }.into_any(),
                    },
                    AiPhase::Error => {
                        let err = ctrl.error.get().unwrap_or_else(AiError::unknown);
                        let msg = err.friendly().into_owned();
                        let retryable = err.retryable;
                        view! {
                            <div class="ai-text-reveal flex flex-col gap-3 p-1">
                                <p class="text-sm leading-relaxed text-ink/80">{msg}</p>
                                <Show when=move || retryable>
                                    <button
                                        type="button"
                                        on:click=move |_| ctrl.retry.run(())
                                        class="self-start rounded-full border border-line bg-surface \
                                               px-4 py-1.5 text-sm font-medium text-ink \
                                               transition-[transform,background-color] duration-150 ease-out \
                                               hover:bg-line active:scale-[0.96] \
                                               focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                    >
                                        "Try again"
                                    </button>
                                </Show>
                            </div>
                        }
                        .into_any()
                    }
                    AiPhase::Idle => ().into_any(),
                }}
            </GlossSurface>
        </Show>

        // Mark management chrome: the bottom-right selection bar, the
        // right-click remove menu, and the undo toast. All three sit above
        // the expanded surface (z 50): bar 60, menu/toast 70.
        <GlossSelectBar state ctrl undo=sm.undo />
        <GlossContextMenu state ctrl menu=sm.menu undo=sm.undo />
        <GlossUndoToast state ctrl undo=sm.undo />
    }
}
