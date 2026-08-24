//! The gloss popover: the orchestrating state machine — the spring-driven
//! successor to the former warp-window AI popover.
//!
//! Two orthogonal phases run together:
//! * the **geometry phase** ([`GlossPhase`]): pill → expanded card → chip;
//! * the **data phase** ([`AiPhase`]): processing → streaming → done/error.
//!
//! Three things are load-bearing here and easy to undo by accident:
//!
//! * **The anchor is a [`GlossMark`], never a DOM node.** Every scroll, zoom,
//!   page or mode change re-projects the mark's page-space rect through
//!   whichever host currently renders that page, so the card sticks to the
//!   *text* even across the virtualizer's unmounts. The mark is written to
//!   localStorage at capture time and is deliberately NOT removed on dismiss:
//!   closing the card leaves the word highlighted, and clicking that highlight
//!   re-opens the card through the same spring.
//! * **Expansion is data-driven.** There is no expand timer; the card blooms
//!   out when the first snapshot (or an error) arrives, so it can never pop
//!   open empty. Until then the pill's scan line is the whole processing UI.
//! * **Only a scroll OUTSIDE the card collapses it.** Scroll/wheel/touch whose
//!   target is the card's own scroller are the card scrolling itself.

use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

use pdf_core::gloss::{place_expanded, GlossBox, GlossMark};
use pdf_core::layout::ViewMode;

use crate::components::ai::gloss::anchor::{capture_selection_mark, mark_screen_box};
use crate::components::ai::gloss::marks::GLOSS_OPEN_EVENT;
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
/// Header + divider + close button + paddings: everything the measure twin
/// does not contain. Height = measured content + this, never a fixed height.
const CARD_CHROME_H: f64 = 132.0;
/// Vertical scroll (px) past the expand point that folds the card back.
const COLLAPSE_SCROLL_PX: f64 = 8.0;
/// Per-document cap on persisted marks (oldest evicted). A reading session's
/// worth of looked-up words, bounded so localStorage can't grow without end.
const MARK_CAP: usize = 200;

#[component]
pub fn GlossAiPopover(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;
    let marks = state.reader.gloss.marks;

    // ── Data phase + content ──────────────────────────────────────────────
    let phase = RwSignal::new(AiPhase::Idle);
    let word = RwSignal::new(String::new());
    let word_info = RwSignal::new(None::<WordInfo>);
    let error_msg = RwSignal::new(None::<String>);
    let gphase = RwSignal::new(GlossPhase::Processing);

    // ── Anchor: the persisted mark this card belongs to ───────────────────
    // Everything positional derives from it, so the card cannot lose its
    // anchor when the page it sits on is remounted.
    let mark_v = StoredValue::new_local(None::<GlossMark>);
    let anchor = RwSignal::new(None::<GlossBox>);
    let pending_mark = RwSignal::new(None::<GlossMark>);

    let drag_box = RwSignal::new(None::<GlossBox>);
    let dragging = RwSignal::new(false);
    let grab = StoredValue::new_local(None::<(f64, f64)>);
    let expand_scroll_y = StoredValue::new_local(0.0_f64);
    let content_height = RwSignal::new(0.0_f64);
    let viewport = RwSignal::new(viewport_size());
    let reduced = reduced_motion_signal();

    // Re-project the page-space mark onto the screen. Called on every scroll /
    // zoom / mode / page / resize tick — this is the "sticks to the text" part.
    let recompute_anchor = move || {
        if let Some(m) = mark_v.get_value() {
            let single = state.reader.viewer.mode.get_untracked() == ViewMode::Single;
            let s = state.reader.viewer.zoom.display.get_untracked();
            if let Some(b) = mark_screen_box(&m, s, single) {
                anchor.set(Some(b));
            }
        } else {
            anchor.set(None);
        }
    };

    // Clicking a persisted highlight reopens the card with the same animation.
    let open_handle = window_event_listener(
        leptos::ev::Custom::new(GLOSS_OPEN_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(m) = serde_wasm_bindgen::from_value::<GlossMark>(ev.detail()) else {
                return;
            };
            detail.set(None);
            pending_mark.set(Some(m));
            popover_open.set(true);
        },
    );
    on_cleanup(move || open_handle.remove());

    // Expand ONLY when the first chunk (or an error) arrives. The chip's scan
    // line is the whole "thinking" UI; never pop an empty card.
    let _listener = listen_ai_chunks(move |chunk| match chunk {
        AiChunkEvent::Snapshot(info) => {
            if phase.get_untracked() == AiPhase::Processing {
                phase.set(AiPhase::Streaming);
                if gphase.get_untracked() == GlossPhase::Processing {
                    expand_scroll_y.set_value(current_scroll_y());
                    gphase.set(GlossPhase::Expanded);
                }
            }
            word_info.set(Some(info));
        }
        AiChunkEvent::Done => phase.set(AiPhase::Done),
        AiChunkEvent::Error(msg) => {
            error_msg.set(Some(msg));
            phase.set(AiPhase::Error);
            if gphase.get_untracked() == GlossPhase::Processing {
                gphase.set(GlossPhase::Expanded);
            }
        }
    });

    // Full dismiss back to Idle. NOTE: the mark itself is intentionally kept —
    // the highlight is the point, and it is what reopens this card later.
    let reset = move || {
        popover_open.set(false);
        phase.set(AiPhase::Idle);
        word.set(String::new());
        word_info.set(None);
        error_msg.set(None);
        gphase.set(GlossPhase::Processing);
        mark_v.set_value(None);
        anchor.set(None);
        drag_box.set(None);
        dragging.set(false);
        grab.set_value(None);
    };

    // Fold the expanded card back onto the word (scroll-to-close).
    let collapse_to_mark = move || {
        if gphase.get_untracked() != GlossPhase::Expanded || dragging.get_untracked() {
            return;
        }
        drag_box.set(None);
        gphase.set(GlossPhase::Compact);
    };

    // Record + persist a freshly captured mark (deduped against a re-explain
    // of the same word at the same spot).
    let add_mark = move |m: GlossMark| {
        let Some(path) = state.reader.document.path.get_untracked() else {
            return;
        };
        marks.update(|v| {
            let duplicate = v.iter().any(|o| {
                o.page == m.page
                    && o.word == m.word
                    && (o.rect.x - m.rect.x).abs() < 1.0
                    && (o.rect.y - m.rect.y).abs() < 1.0
            });
            if duplicate {
                return;
            }
            v.push(m.clone());
            if v.len() > MARK_CAP {
                v.remove(0);
            }
        });
        crate::storage::persist_gloss(&path, &marks.get_untracked());
    };

    // ── Effect: opening — from a fresh selection OR from a clicked mark ───
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        let sel = detail.get_untracked();
        let clicked = pending_mark.get_untracked();
        let scale = state.reader.viewer.zoom.display.get_untracked();
        let page_now = state.reader.viewer.page.get_untracked();
        let mark = match (clicked, sel) {
            // Reopening an existing highlight: it already knows its word,
            // context and rect.
            (Some(m), _) => m,
            // A fresh selection: capture it BEFORE the selection is cleared.
            (None, Some(s)) => {
                match capture_selection_mark(page_now, scale, s.text.clone(), s.context.clone()) {
                    Some(m) => {
                        add_mark(m.clone());
                        m
                    }
                    None => return,
                }
            }
            (None, None) => return,
        };

        mark_v.set_value(Some(mark.clone()));
        recompute_anchor();
        word.set(mark.word.clone());
        viewport.set(viewport_size());

        phase.set(AiPhase::Processing);
        word_info.set(None);
        error_msg.set(None);
        gphase.set(GlossPhase::Processing);
        drag_box.set(None);
        dragging.set(false);
        expand_scroll_y.set_value(current_scroll_y());

        // The native selection would fight the card's own text selection, and
        // the persisted mark has already replaced its tint.
        if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
            let _ = s.remove_all_ranges();
        }
        detail.set(None);
        pending_mark.set(None);

        if !pdf_engine::has_tauri() {
            error_msg.set(Some(
                "AI explanations are only available in the desktop app.".into(),
            ));
            phase.set(AiPhase::Error);
            gphase.set(GlossPhase::Expanded);
        } else {
            invoke_explain_word(mark.word, mark.context);
        }
        // No expand timer on purpose: the pill + scan line IS the processing UI.
    });

    // ── Effect: stick to the text ────────────────────────────────────────
    // Re-derive the anchor from the page-space mark whenever anything that
    // moves the page moves.
    Effect::new(move |_| {
        let _ = state.reader.viewer.scroll_top.get();
        let _ = state.reader.viewer.zoom.display.get();
        let _ = state.reader.viewer.mode.get();
        let _ = state.reader.viewer.page.get();
        let _ = state.reader.viewer.container_size.get();
        recompute_anchor();
    });

    // ── Derived geometry ─────────────────────────────────────────────────
    let expanded_target = Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        let w = CARD_WIDTH.min((vw - 32.0).max(260.0));
        // Height fits the measured content; the only clamp is the viewport.
        let h = (content_height.get() + CARD_CHROME_H).clamp(140.0, vh * 0.8);
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

    // The pill and the chip must track the word EXACTLY as it scrolls; only
    // the expanded card is worth springing.
    let snap = Signal::derive(move || {
        dragging.get() || reduced.get() || gphase.get() != GlossPhase::Expanded
    });
    let sprung = use_spring_box(target.into(), snap);

    let progress = Memo::new(move |_| {
        let (Some(b), Some(a), Some(e)) = (sprung.get(), anchor.get(), expanded_target.get()) else {
            return if gphase.get() == GlossPhase::Expanded { 1.0 } else { 0.0 };
        };
        ((b.w - a.w) / (e.w - a.w).max(1.0)).clamp(0.0, 1.0)
    });

    // Surface prop signals (unwrapped — the surface only renders while open).
    let phase_sig = Signal::derive(move || phase.get());
    let gphase_sig = Signal::derive(move || gphase.get());
    let box_sig = Signal::derive(move || sprung.get().unwrap_or_default());
    let expanded_sig = Signal::derive(move || expanded_target.get().unwrap_or_default());
    let progress_sig = Signal::derive(move || progress.get());
    let word_sig = Signal::derive(move || word.get());

    // ── Callbacks wired into the surface ─────────────────────────────────
    let on_hover_expand = Callback::new(move |_: ()| {
        if gphase.get_untracked() != GlossPhase::Compact
            || phase.get_untracked() == AiPhase::Processing
        {
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

    // ── Effect: scroll handling ──────────────────────────────────────────
    // Scrolling INSIDE the card never collapses it; scrolling the document
    // outside it does (expanded -> compact chip).
    Effect::new(move |_| {
        if !popover_open.get() {
            return;
        }
        // Capture-phase scroll from ANY scroller (window, #page-list, the
        // single-page container) — scroll doesn't bubble but does hit window
        // listeners in the capture phase, so one listener catches them all.
        add_window_capture_listener("scroll", move |ev: web_sys::Event| {
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_ref::<web_sys::Element>().cloned())
                && el.closest(".gloss-surface").ok().flatten().is_some()
            {
                return; // the card's own scroller
            }
            recompute_anchor();
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
            recompute_anchor();
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
                && el.closest(".gloss-surface").ok().flatten().is_some()
            {
                return;
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
    // (no minimum: a two-line answer gets a two-line card, and a later chunk
    // growing the twin morphs the card open a little further).
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

    // ── Effect: a page flip collapses an expanded card back onto its mark
    // (which may now be off screen — the anchor still knows where it is).
    Effect::new(move |_| {
        let _ = state.reader.viewer.page.get();
        if !popover_open.get_untracked() {
            return;
        }
        if gphase.get_untracked() == GlossPhase::Expanded {
            drag_box.set(None);
            gphase.set(GlossPhase::Compact);
        }
        recompute_anchor();
    });

    // ── Effect: a zoom re-renders the textLayer; the mark survives it, but
    // the open card would slide under the reader's hands mid-gesture — close
    // it and leave the highlight behind.
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
                phase=gphase_sig
                ai_phase=phase_sig
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
