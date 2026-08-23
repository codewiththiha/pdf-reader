use leptos::prelude::*;

use super::types::{AiPhase, WordInfo};
use super::word_info::{LoadingShimmer, WordInfoSections};
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::services::ai::{AiChunkEvent, invoke_explain_word, listen_ai_chunks};
use crate::state::AppState;

/// The AI explanation popover. Renders as a "warp window" that starts at
/// the selection's bounding box, glows with a rainbow animation while the
/// backend thinks, then morphs into the full explanation card as chunks
/// of the streamed `WordInfo` arrive.
///
/// Carries `data-ai-popover` so the engine's selection tracker treats
/// presses inside it as AI-UI interaction and does not clear the
/// selection detail out from under the open popover.
#[component]
pub fn AiPopover(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // ── Local AI state ──────────────────────────────────────────────
    let phase = RwSignal::new(AiPhase::Idle);
    let word_info = RwSignal::new(None::<WordInfo>);
    let error_msg = RwSignal::new(None::<String>);

    // One listener for the component's life: every chunk lands in the
    // signals; the open-effect below resets them before it starts a new
    // run, and the signals are only read while the popover is open.
    let _listener = listen_ai_chunks(move |chunk| match chunk {
        AiChunkEvent::Snapshot(info) => {
            // The first snapshot is what triggers the morph to the card.
            if phase.get_untracked() == AiPhase::Processing {
                phase.set(AiPhase::Streaming);
            }
            word_info.set(Some(info));
        }
        AiChunkEvent::Done => phase.set(AiPhase::Done),
        AiChunkEvent::Error(msg) => {
            error_msg.set(Some(msg));
            phase.set(AiPhase::Error);
        }
    });

    // Shared close/reset; captures only Copy signals, so it is itself
    // `Copy` and can back both the backdrop press and the ✕ button.
    let reset = move || {
        popover_open.set(false);
        phase.set(AiPhase::Idle);
        word_info.set(None);
        error_msg.set(None);
    };

    // ── Derived: selection rect for positioning ─────────────────────
    let sel_rect = Signal::derive(move || detail.get().map(|d| d.rect));

    // ── Derived: warp window geometry ───────────────────────────────
    // Processing: pinned to the selection bounding box.
    // Streaming/Done/Error: morphed to a fixed-size card below it.
    let warp_style = Signal::derive(move || {
        let Some(rect) = sel_rect.get() else {
            return String::new();
        };

        match phase.get() {
            AiPhase::Processing => {
                // Exactly the selection's box, padded slightly so the
                // glow extends beyond the text.
                let pad = 4.0;
                format!(
                    "top:{}px; left:{}px; width:{}px; height:{}px;",
                    rect.y - pad,
                    rect.x - pad,
                    rect.width + pad * 2.0,
                    rect.height + pad * 2.0,
                )
            }
            AiPhase::Streaming | AiPhase::Done | AiPhase::Error => {
                // Morph to a card below the selection.
                let card_w = 320.0_f64;
                let card_left = (rect.x + rect.width / 2.0 - card_w / 2.0).max(8.0);
                let card_top = rect.y + rect.height + 12.0;
                format!(
                    "top:{}px; left:{}px; width:{}px; max-height:60vh;",
                    card_top, card_left, card_w,
                )
            }
            AiPhase::Idle => String::new(),
        }
    });

    // ── Derived: CSS class for the current phase ────────────────────
    let warp_class = Signal::derive(move || {
        let base = "ai-warp-window";
        let phase_class = phase.get().css_class();
        if phase_class.is_empty() {
            base.to_string()
        } else {
            format!("{base} {phase_class}")
        }
    });

    // ── Derived: selected word for the header ───────────────────────
    let selected_word = Signal::derive(move || {
        detail.get().map(|d| d.text).unwrap_or_default()
    });

    // ── Effect: opening the popover starts a backend run ────────────
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        phase.set(AiPhase::Processing);
        word_info.set(None);
        error_msg.set(None);

        let Some(sel) = detail.get_untracked() else {
            return;
        };

        // Without the desktop backend there is nobody to answer the
        // invoke; say so instead of shimmering forever (plain-browser
        // `trunk serve` development).
        if !pdf_engine::has_tauri() {
            error_msg.set(Some("AI explanations are only available in the desktop app.".to_string()));
            phase.set(AiPhase::Error);
            return;
        }

        invoke_explain_word(sel.text, sel.context);
    });

    view! {
        <Show when=move || popover_open.get() && phase.get() != AiPhase::Idle>
            // ── Backdrop: press outside to dismiss ──────────────────
            <div
                class="fixed inset-0 z-[85]"
                on:mousedown=move |_| reset()
            ></div>

            // ── The Warp Window ─────────────────────────────────────
            <div
                data-ai-popover="true"
                class=move || warp_class.get()
                style=move || warp_style.get()
                role="dialog"
                aria-label="AI word explanation"
                // Preventing the mousedown default keeps the document
                // selection alive while the popover is open (otherwise the
                // first press inside would collapse the highlight the
                // popover is anchored to). Trade-off, accepted for now:
                // the popover body is not text-selectable.
                on:mousedown=move |ev| ev.prevent_default()
            >
                <div class="ai-warp-inner">

                    // ── Header: word + close button ─────────────────
                    <div class="flex items-start justify-between gap-2 border-b border-line px-3 py-2">
                        <span class="ai-word-title ai-text-reveal">
                            {move || selected_word.get()}
                        </span>
                        <IconButton
                            icon=IconName::Close
                            title="Close"
                            size=14
                            on_click=reset
                        />
                    </div>

                    // ── Body: phase-dependent content ───────────────
                    {move || match phase.get() {
                        AiPhase::Processing => {
                            view! { <LoadingShimmer /> }.into_any()
                        }
                        AiPhase::Streaming | AiPhase::Done => {
                            match word_info.get() {
                                Some(info) => {
                                    view! { <WordInfoSections info=info /> }.into_any()
                                }
                                None => {
                                    view! { <LoadingShimmer /> }.into_any()
                                }
                            }
                        }
                        AiPhase::Error => {
                            let msg = error_msg.get()
                                .unwrap_or_else(|| "Something went wrong.".to_string());
                            view! {
                                <div class="ai-text-reveal p-3 text-sm text-red-400">
                                    {msg}
                                </div>
                            }.into_any()
                        }
                        AiPhase::Idle => ().into_any(),
                    }}
                </div>
            </div>
        </Show>
    }
}
