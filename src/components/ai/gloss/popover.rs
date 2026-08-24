//! The gloss popover: the orchestrating state machine — the spring-driven
//! successor to the former warp-window AI popover.
//!
//! Two orthogonal phases run together:
//! * the **geometry phase** ([`GlossPhase`]): bloom → expanded card → chip;
//! * the **data phase** ([`AiPhase`]): processing → streaming → done/error.
//!
//! The card body is fed by the chunk listener exactly as the old popover's
//! was; only the *box* is new — sprung open from the selected word, folded
//! back to it on scroll, and draggable by a grab handle.

use std::time::Duration;

use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

use pdf_core::gloss::{pad_box, place_expanded, GlossBox};

use crate::components::ai::gloss::anchor::{capture_selection_anchor, live_anchor_box};
use crate::components::ai::gloss::spring::use_spring_box;
use crate::components::ai::gloss::surface::GlossSurface;
use crate::components::ai::gloss::util::{
    add_window_capture_listener, card_scroller_can_absorb, current_scroll_y,
    reduced_motion_signal, target_inside_card_scroller, viewport_size,
};
use crate::components::ai::types::{AiPhase, GlossPhase, WordInfo};
use crate::components::ai::word_info::{LoadingShimmer, WordInfoSections};
use crate::services::ai::{AiChunkEvent, invoke_explain_word, listen_ai_chunks};
use crate::state::AppState;

/// Expanded card geometry constants (same numbers as the reference).
const CARD_WIDTH: f64 = 320.0;
const CARD_RADIUS: f64 = 24.0;
const CARD_MIN_HEIGHT: f64 = 260.0;
/// The bloom beat before the card blooms out (reference `mockGatherDelay`).
const EXPAND_DELAY_MS: u64 = 340;
const EXPAND_DELAY_REDUCED_MS: u64 = 80;
/// Vertical scroll (px) past the expand point that folds the card back.
const COLLAPSE_SCROLL_PX: f64 = 8.0;

#[component]
pub fn GlossAiPopover(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // ── Data phase + content (identical wiring to the old popover) ────────
    let phase = RwSignal::new(AiPhase::Idle);
    let word = RwSignal::new(String::new());
    let word_info = RwSignal::new(None::<WordInfo>);
    let error_msg = RwSignal::new(None::<String>);

    // One listener for the component's life: every chunk lands in the signals.
    let _listener = listen_ai_chunks(move |chunk| match chunk {
        AiChunkEvent::Snapshot(info) => {
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

    // ── Geometry phase + anchor/drag signals ─────────────────────────────
    let gphase = RwSignal::new(GlossPhase::Processing);
    let anchor_el = StoredValue::new_local(None::<web_sys::Element>);
    let anchor = RwSignal::new(None::<GlossBox>);
    let drag_box = RwSignal::new(None::<GlossBox>);
    let dragging = RwSignal::new(false);
    let grab = StoredValue::new_local(None::<(f64, f64)>);
    let expand_scroll_y = StoredValue::new_local(0.0_f64);
    let content_height = RwSignal::new(420.0_f64);
    let viewport = RwSignal::new(viewport_size());
    let reduced = reduced_motion_signal();

    // Full dismiss back to Idle.
    let reset = move || {
        popover_open.set(false);
        phase.set(AiPhase::Idle);
        word.set(String::new());
        word_info.set(None);
        error_msg.set(None);
        gphase.set(GlossPhase::Processing);
        anchor.set(None);
        drag_box.set(None);
        dragging.set(false);
        grab.set_value(None);
    };

    // Live re-measure of the captured anchor span (the PDF replacement for a
    // <mark>): keeps the last box if a zoom re-render detached the span.
    let sync_anchor = move || {
        if let Some(el) = anchor_el.get_value() {
            if let Some(b) = live_anchor_box(&el) {
                anchor.set(Some(b));
            }
        }
    };
    // Fold the expanded card back onto the word (scroll-to-close).
    let collapse_to_mark = move || {
        if gphase.get_untracked() != GlossPhase::Expanded {
            return;
        }
        if dragging.get_untracked() {
            return;
        }
        drag_box.set(None);
        gphase.set(GlossPhase::Compact);
    };

    // ── Derived geometry (memo chain identical to the reference) ─────────
    let expanded_target = Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        let w = CARD_WIDTH.min((vw - 32.0).max(260.0));
        let h = content_height.get().max(CARD_MIN_HEIGHT).min(vh * 0.74);
        Some(place_expanded(a, w, h, vw, vh, CARD_RADIUS))
    });

    let target = Memo::new(move |_| {
        let a = anchor.get()?;
        match gphase.get() {
            // Expanded: follow a manual drag, else the viewport-clamped card.
            GlossPhase::Expanded => Some(drag_box.get().or(expanded_target.get()).unwrap_or(a)),
            // Processing + compact both hug the word.
            _ => Some(a),
        }
    });

    let snap = Signal::derive(move || {
        dragging.get() || gphase.get() == GlossPhase::Processing || reduced.get()
    });
    let sprung = use_spring_box(target.into(), snap);

    let progress = Memo::new(move |_| {
        let (Some(b), Some(a), Some(e)) = (sprung.get(), anchor.get(), expanded_target.get()) else {
            return if gphase.get() == GlossPhase::Expanded { 1.0 } else { 0.0 };
        };
        ((b.w - a.w) / (e.w - a.w).max(1.0)).clamp(0.0, 1.0)
    });

    // Surface prop signals (unwrapped — the surface only renders while open).
    let phase_sig = Signal::derive(move || gphase.get());
    let box_sig = Signal::derive(move || sprung.get().unwrap_or_default());
    let expanded_sig = Signal::derive(move || expanded_target.get().unwrap_or_default());
    let progress_sig = Signal::derive(move || progress.get());
    let word_sig = Signal::derive(move || word.get());

    // ── Callbacks wired into the surface ─────────────────────────────────
    let on_hover_expand = Callback::new(move |_: ()| {
        if gphase.get_untracked() != GlossPhase::Compact {
            return;
        }
        expand_scroll_y.set_value(current_scroll_y());
        gphase.set(GlossPhase::Expanded);
    });
    let on_dismiss = Callback::new(move |_: ()| reset());
    let on_drag_start = Callback::new(move |(cx, cy, origin): (f64, f64, GlossBox)| {
        grab.set_value(Some((cx - origin.x, cy - origin.y)));
        dragging.set(true);
    });

    // ── Effect: opening starts a backend run + the bloom beat ────────────
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        let Some(sel) = detail.get_untracked() else {
            return;
        };

        // 1. Capture BEFORE clearing: the span element + a padded rect.
        anchor_el.set_value(capture_selection_anchor());
        let rect = &sel.rect;
        anchor.set(Some(pad_box(
            GlossBox {
                x: rect.x,
                y: rect.y,
                w: rect.width,
                h: rect.height,
                r: 0.0,
            },
            5.0,
            3.0,
        )));
        word.set(sel.text.clone());
        viewport.set(viewport_size());

        // 2. Reset data/geometry and kill the selection + menu.
        phase.set(AiPhase::Processing);
        word_info.set(None);
        error_msg.set(None);
        gphase.set(GlossPhase::Processing);
        drag_box.set(None);
        dragging.set(false);
        expand_scroll_y.set_value(current_scroll_y());

        if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
            let _ = s.remove_all_ranges();
        }
        detail.set(None);

        // 3. Fire the backend run (or surface a clear error in plain-browser dev).
        if !pdf_engine::has_tauri() {
            error_msg.set(Some(
                "AI explanations are only available in the desktop app.".into(),
            ));
            phase.set(AiPhase::Error);
        } else {
            invoke_explain_word(sel.text, sel.context);
        }

        // 4. Processing -> Expanded after the bloom beat.
        let ms = if reduced.get_untracked() {
            EXPAND_DELAY_REDUCED_MS
        } else {
            EXPAND_DELAY_MS
        };
        let handle = set_timeout_with_handle(
            move || {
                expand_scroll_y.set_value(current_scroll_y());
                gphase.set(GlossPhase::Expanded);
            },
            Duration::from_millis(ms),
        )
        .ok();
        on_cleanup(move || {
            if let Some(h) = handle {
                h.clear();
            }
        });
    });

    // ── Effect: scroll-to-close + wheel/touch guards ─────────────────────
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        // Capture-phase scroll from ANY scroller (window, #page-list, the
        // single-page container) — scroll doesn't bubble but does hit window
        // listeners in the capture phase, so one listener catches them all.
        // (The listener owns its own cleanup via the reactive owner.)
        add_window_capture_listener("scroll", move |_ev: web_sys::Event| {
            sync_anchor();
            if (current_scroll_y() - expand_scroll_y.get_value()).abs() > COLLAPSE_SCROLL_PX {
                collapse_to_mark();
            }
        });
        let wheel = window_event_listener_untyped("wheel", move |ev: web_sys::Event| {
            let we = ev.unchecked_ref::<web_sys::WheelEvent>();
            if card_scroller_can_absorb(we) {
                return; // the card's own scroller still has room
            }
            if we.delta_y().abs() + we.delta_x().abs() > 2.0 {
                collapse_to_mark();
            }
        });
        let touch = window_event_listener_untyped("touchmove", move |ev: web_sys::Event| {
            if target_inside_card_scroller(&ev) {
                return;
            }
            collapse_to_mark();
        });
        let resize = window_event_listener_untyped("resize", move |_| {
            viewport.set(viewport_size());
            sync_anchor();
        });
        on_cleanup(move || {
            wheel.remove();
            touch.remove();
            resize.remove();
        });
    });

    // ── Effect: Escape / outside-tap ─────────────────────────────────────
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() != "Escape" {
                return;
            }
            match gphase.get_untracked() {
                GlossPhase::Expanded => {
                    drag_box.set(None);
                    gphase.set(GlossPhase::Compact);
                }
                _ => reset(),
            }
        });
        let pd = window_event_listener_untyped("pointerdown", move |ev: web_sys::Event| {
            // A press inside the surface is the card's own interaction.
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            {
                if el.closest(".gloss-surface").ok().flatten().is_some() {
                    return;
                }
            }
            if gphase.get_untracked() == GlossPhase::Expanded {
                drag_box.set(None);
                gphase.set(GlossPhase::Compact);
            }
        });
        on_cleanup(move || {
            key.remove();
            pd.remove();
        });
    });

    // ── Effect: dragging the expanded card ───────────────────────────────
    Effect::new(move |_| {
        if !dragging.get() {
            return;
        }
        let mv = window_event_listener_untyped("pointermove", move |ev: web_sys::Event| {
            let me = ev.unchecked_ref::<web_sys::MouseEvent>();
            let Some((dx, dy)) = grab.get_value() else {
                return;
            };
            let Some(e) = expanded_target.get_untracked() else {
                return;
            };
            let (vw, vh) = viewport_size();
            let x = (me.client_x() as f64 - dx).clamp(12.0, vw - e.w - 12.0);
            let y = (me.client_y() as f64 - dy).clamp(12.0, vh - e.h - 12.0);
            drag_box.set(Some(GlossBox { x, y, ..e }));
        });
        let end = move || {
            grab.set_value(None);
            dragging.set(false);
            expand_scroll_y.set_value(current_scroll_y());
        };
        let up = window_event_listener_untyped("pointerup", move |_| end());
        let cancel = window_event_listener_untyped("pointercancel", move |_| end());
        on_cleanup(move || {
            mv.remove();
            up.remove();
            cancel.remove();
        });
    });

    // ── Effect: measure the real content so the card height fits the answer
    let measure_ref: NodeRef<html::Div> = NodeRef::new();
    Effect::new(move |_| {
        let _ = word_info.get(); // re-measure when data changes
        if let Some(el) = measure_ref.get() {
            // Defer to the next frame so the twin reflects the new content.
            request_animation_frame(move || {
                let next = el.scroll_height() as f64;
                content_height.update(|h| {
                    if (*h - next).abs() > 2.0 {
                        *h = next;
                    }
                });
            });
        }
    });

    // ── Effect: a page flip in single-page mode collapses an expanded card
    // (the anchor span lives on the old page; there is no scroll to fold it).
    Effect::new(move |_| {
        let _ = state.reader.viewer.page.get();
        if !popover_open.get_untracked() {
            return;
        }
        if gphase.get_untracked() == GlossPhase::Expanded {
            drag_box.set(None);
            gphase.set(GlossPhase::Compact);
        }
    });

    // ── Effect: a zoom re-renders the textLayer and detaches the anchor span
    // (the one place the mark-based reference can't be matched) — dismiss.
    Effect::new(move |_| {
        if !state.reader.viewer.zoom_animating.get() {
            return;
        }
        if popover_open.get_untracked() {
            reset();
        }
    });

    view! {
        // Invisible measure twin at card width — the card height tracks the
        // real answer rather than a fixed guess (cf. the reference's MeasureCard).
        <div
            node_ref=measure_ref
            class="pointer-events-none invisible fixed left-0 top-0 z-0"
            style=format!("width:{CARD_WIDTH}px")
            aria-hidden="true"
        >
            {move || word_info.get().map(|info| view! { <WordInfoSections info=info /> })}
        </div>

        <Show when=move || popover_open.get() && phase.get() != AiPhase::Idle>
            <GlossSurface
                phase=phase_sig
                box_=box_sig
                expanded=expanded_sig
                progress=progress_sig
                word=word_sig
                on_hover_expand=on_hover_expand
                on_dismiss=on_dismiss
                on_drag_start=on_drag_start
            >
                {move || match phase.get() {
                    AiPhase::Processing => view! { <LoadingShimmer /> }.into_any(),
                    AiPhase::Streaming | AiPhase::Done => match word_info.get() {
                        Some(info) => view! { <WordInfoSections info=info /> }.into_any(),
                        None => view! { <LoadingShimmer /> }.into_any(),
                    },
                    AiPhase::Error => {
                        let msg = error_msg
                            .get()
                            .unwrap_or_else(|| "Something went wrong.".into());
                        view! {
                            <div class="ai-text-reveal p-1 text-sm text-red-400">{msg}</div>
                        }
                            .into_any()
                    }
                    AiPhase::Idle => ().into_any(),
                }}
            </GlossSurface>
        </Show>
    }
}
