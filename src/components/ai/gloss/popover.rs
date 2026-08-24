//! The gloss popover: the orchestrating state machine — the spring-driven
//! successor to the former warp-window AI popover.
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
//! * **Every close is an outro, not a cut.** Close button, origin-exit,
//!   Escape and outside clicks all run `collapse_to_mark`, and the spring is
//!   NOT snapped while compact, so the card visibly morphs back down onto the
//!   word before handing over to the persisted stroke.
//! * **Re-opening is recall, not a rescan.** Snapshots are cached by mark id,
//!   so clicking a stroke morphs the card open on `AiPhase::Done` content
//!   without touching the backend. The spring is hard-reset onto the new
//!   word's mark on every open so the morph never flies in from the previous
//!   card's resting place.
//!
//! Open path is deterministic: both the Info pill and a saved stroke dispatch
//! `pdfreader:gloss-open` with the mark in the event detail. The listener
//! sets `pending_mark` and bumps `open_req` (which the open effect tracks),
//! so the effect always runs with a mark in hand — never a race against
//! `detail` being cleared or a stale `popover_open = true` no-op.

use std::collections::HashMap;

use leptos::{html, prelude::*};
use wasm_bindgen::JsCast;

use pdf_core::gloss::{boxes_close, GlossBox, GlossMark};

use crate::components::ai::anchor::{watch_page_anchor, PageAnchor, CARD_EXIT_FRAC};
use crate::components::ai::gloss::marks::GLOSS_OPEN_EVENT;
use crate::components::ai::gloss::spring::use_spring_box;
use crate::components::ai::gloss::surface::GlossSurface;
use crate::components::ai::gloss::util::{reduced_motion_signal, viewport_size};
use crate::components::ai::types::{AiPhase, GlossPhase, WordInfo};
use crate::components::ai::word_info::{LoadingShimmer, WordInfoSections};
use crate::services::ai::{invoke_explain_word, AiChunkEvent, AI_CHUNK_EVENT};
use crate::state::AppState;

/// Expanded card geometry constants.
const CARD_WIDTH: f64 = 360.0;
const CARD_RADIUS: f64 = 18.0;
/// Header + divider + close button + paddings: everything the measure twin
/// does not contain. Height = measured content + this, never a fixed height.
const CARD_CHROME_H: f64 = 132.0;
/// Per-document cap on persisted marks (oldest evicted). A reading session's
/// worth of looked-up words, bounded so localStorage can't grow without end.
const MARK_CAP: usize = 200;
/// Gap between the highlighter stroke and the card's near edge.
const CARD_GAP: f64 = 16.0;
/// Viewport margin the expanded card must stay inside.
const CARD_MARGIN: f64 = 12.0;

#[component]
pub fn GlossAiPopover(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;
    let marks = state.reader.gloss.marks;
    let processing_id = state.reader.gloss.processing_id;

    // ── Data phase + content ──────────────────────────────────────────────
    let phase = RwSignal::new(AiPhase::Idle);
    let word = RwSignal::new(String::new());
    let word_info = RwSignal::new(None::<WordInfo>);
    let error_msg = RwSignal::new(None::<String>);
    let gphase = RwSignal::new(GlossPhase::Processing);

    // ── Anchor: the persisted mark this card belongs to ───────────────────
    // Reactive so `watch_page_anchor` re-derives the moment a new mark opens;
    // `StoredValue` alone would leave the watch looking at a stale None.
    let mark_sig = RwSignal::new(None::<GlossMark>);
    let pending_mark = RwSignal::new(None::<GlossMark>);
    let open_req = RwSignal::new(0u64);

    // Anchor-relative drag offset (dx, dy). Stored as an offset — not a screen
    // box — so a dragged card still travels with the page when the anchor
    // moves on scroll. Compact phase ignores it and morphs home to the mark.
    let drag_offset = RwSignal::new(None::<(f64, f64)>);
    let dragging = RwSignal::new(false);
    let grab = StoredValue::new_local(None::<(f64, f64)>);
    let content_height = RwSignal::new(0.0_f64);
    let viewport = RwSignal::new(viewport_size());
    let reduced = reduced_motion_signal();
    // Whether the morphing surface exists at all. Distinct from
    // `popover_open`: during processing the stroke IS the UI, and after the
    // outro morph the surface unmounts while the gloss stays "open" on its
    // mark.
    let surface_visible = RwSignal::new(false);
    // Answers already fetched this session, keyed by mark id. Re-opening a
    // stroke is recall, not a rescan.
    let cache = StoredValue::new_local(HashMap::<String, WordInfo>::new());

    // ONE shared, page-aware anchor: follows scroll/zoom/mode/page, and flags
    // `exited` once the origin passes CARD_EXIT_FRAC of the viewport height
    // (or leaves the top, or its page unmounts).
    let watch = watch_page_anchor(
        Signal::derive(move || mark_sig.get().map(|m| PageAnchor::from_mark(&m))),
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.mode.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        CARD_EXIT_FRAC,
    );
    let anchor = watch.screen;

    // Every open (stroke click OR Info pill) arrives as a CustomEvent that
    // carries the mark and bumps the nonce. Tracking open_req is what makes
    // a second open of an already-open popover re-run the effect.
    let open_handle = window_event_listener(
        leptos::ev::Custom::new(GLOSS_OPEN_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(m) = serde_wasm_bindgen::from_value::<GlossMark>(ev.detail()) else {
                return;
            };
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
            pending_mark.set(Some(m));
            open_req.update(|n| *n += 1);
            popover_open.set(true);
        },
    );
    on_cleanup(move || open_handle.remove());

    // Chunks arrive via the app-lifetime Tauri bridge as a window event.
    // Listening here (not on Tauri directly) means unmount cleans up a plain
    // window listener and never stacks/drops dead Tauri handlers across
    // document switches. The surface is born ON the first chunk (or an
    // error) — never on a timer — so it can never pop open empty.
    let chunk_handle = window_event_listener(
        leptos::ev::Custom::new(AI_CHUNK_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(chunk) = serde_wasm_bindgen::from_value::<AiChunkEvent>(ev.detail()) else {
                return;
            };
            match chunk {
                AiChunkEvent::Snapshot(info) => {
                    if let Some(m) = mark_sig.get_untracked() {
                        cache.update_value(|c| {
                            c.insert(m.id.clone(), info.clone());
                        });
                    }
                    processing_id.set(None);
                    if phase.get_untracked() == AiPhase::Processing {
                        phase.set(AiPhase::Streaming);
                        if gphase.get_untracked() == GlossPhase::Processing {
                            gphase.set(GlossPhase::Expanded);
                            surface_visible.set(true);
                        }
                    }
                    word_info.set(Some(info));
                }
                AiChunkEvent::Done => phase.set(AiPhase::Done),
                AiChunkEvent::Error(msg) => {
                    error_msg.set(Some(msg));
                    phase.set(AiPhase::Error);
                    processing_id.set(None);
                    if gphase.get_untracked() == GlossPhase::Processing {
                        gphase.set(GlossPhase::Expanded);
                    }
                    surface_visible.set(true);
                }
            }
        },
    );
    on_cleanup(move || chunk_handle.remove());

    // Full dismiss back to Idle. NOTE: the mark itself is intentionally kept —
    // the highlight is the point, and it is what reopens this card later.
    let reset = move || {
        popover_open.set(false);
        phase.set(AiPhase::Idle);
        word.set(String::new());
        word_info.set(None);
        error_msg.set(None);
        gphase.set(GlossPhase::Processing);
        surface_visible.set(false);
        processing_id.set(None);
        mark_sig.set(None);
        drag_offset.set(None);
        dragging.set(false);
        grab.set_value(None);
    };

    // The outro: fold the expanded card back down onto the word. Every close
    // path funnels through here, and the settle watcher below unmounts the
    // surface once the spring has actually landed on the stroke.
    let collapse_to_mark = move || {
        if gphase.get_untracked() != GlossPhase::Expanded || dragging.get_untracked() {
            return;
        }
        drag_offset.set(None);
        gphase.set(GlossPhase::Compact);
    };

    // Record + persist a freshly captured mark, and hand back the CANONICAL
    // one: re-explaining the same word at the same spot reuses the existing
    // mark rather than stacking a second stroke on it. Returning it matters —
    // the id is what keys the processing glow and the answer cache, so the
    // caller must not go on holding the discarded duplicate.
    let add_mark = move |m: GlossMark| -> GlossMark {
        let existing = marks.with_untracked(|v| {
            v.iter()
                .find(|o| {
                    o.page == m.page
                        && o.word == m.word
                        && (o.rect.x - m.rect.x).abs() < 1.0
                        && (o.rect.y - m.rect.y).abs() < 1.0
                })
                .cloned()
        });
        if let Some(existing) = existing {
            return existing;
        }
        marks.update(|v| {
            v.push(m.clone());
            if v.len() > MARK_CAP {
                v.remove(0);
            }
        });
        if let Some(path) = state.reader.document.path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
        m
    };

    // Side-aware placement: put the card on whichever side of the highlight
    // has more free space, never covering the stroke. Vertically centered on
    // the mark and clamped into the viewport margin.
    let expanded_target = Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        let w = CARD_WIDTH.min((vw - CARD_MARGIN * 2.0).max(260.0));
        let h = (content_height.get() + CARD_CHROME_H).clamp(140.0, vh * 0.8);
        let space_right = vw - (a.x + a.w);
        let x = if space_right >= a.x {
            a.x + a.w + CARD_GAP
        } else {
            a.x - CARD_GAP - w
        };
        let x = x.clamp(CARD_MARGIN, (vw - w - CARD_MARGIN).max(CARD_MARGIN));
        let y = (a.y + a.h * 0.5 - h * 0.5)
            .clamp(CARD_MARGIN, (vh - h - CARD_MARGIN).max(CARD_MARGIN));
        Some(GlossBox {
            x,
            y,
            w,
            h,
            r: CARD_RADIUS,
        })
    });

    // Expanded box is always f(live_anchor) + stored_offset, so a dragged card
    // still glides with the page on scroll. Compact/processing hug the mark.
    let target = Memo::new(move |_| {
        let a = anchor.get()?;
        match gphase.get() {
            GlossPhase::Expanded => {
                let mut e = expanded_target.get().unwrap_or(a);
                if let Some((dx, dy)) = drag_offset.get() {
                    let (vw, vh) = viewport.get();
                    e.x = (e.x + dx).clamp(CARD_MARGIN, (vw - e.w - CARD_MARGIN).max(CARD_MARGIN));
                    e.y = (e.y + dy).clamp(CARD_MARGIN, (vh - e.h - CARD_MARGIN).max(CARD_MARGIN));
                }
                Some(e)
            }
            _ => Some(a),
        }
    });

    // Snapping while compact was what made closing read as a cut: the spring
    // teleported the surface onto the anchor instead of morphing down to it.
    // Only the processing phase (where no surface exists anyway) snaps.
    let snap = Signal::derive(move || {
        dragging.get() || reduced.get() || gphase.get() == GlossPhase::Processing
    });

    let spring = use_spring_box(target.into(), snap);
    let sprung = spring.value;

    let progress = Memo::new(move |_| {
        let (Some(b), Some(a), Some(e)) = (sprung.get(), anchor.get(), expanded_target.get())
        else {
            return if gphase.get() == GlossPhase::Expanded {
                1.0
            } else {
                0.0
            };
        };
        ((b.w - a.w) / (e.w - a.w).max(1.0)).clamp(0.0, 1.0)
    });

    // ---- open effect: re-runs on EVERY request (nonce). The mark always
    // arrives via pending_mark (Info pill and stroke click both dispatch
    // GLOSS_OPEN_EVENT); the bare-selection arm is a defensive fallback.
    Effect::new(move |_| {
        let _ = open_req.get(); // tracked nonce
        if !popover_open.get() {
            return;
        }
        let clicked = pending_mark.get_untracked();
        let sel = detail.get_untracked();
        let mark = match (clicked, sel) {
            // Self-contained open: mark is already in hand (Info pill or
            // stroke click). Persist it so re-open/re-explain reuse the id.
            (Some(m), _) => add_mark(m),
            // Defensive fallback: a bare popover_open=true with a live
            // selection but no pending mark (should not happen after the
            // Info pill switched to request_gloss_open).
            (None, Some(s)) => {
                let scale = state.reader.viewer.zoom.display.get_untracked();
                let page_now = state.reader.viewer.page.get_untracked();
                let stored = state.reader.ai_selection.anchor.get_untracked().map(|pa| {
                    GlossMark {
                        id: format!("g{}-{}", pa.page, js_sys::Date::now() as u64),
                        page: pa.page,
                        word: s.text.clone(),
                        context: s.context.clone(),
                        rect: pa.rect,
                    }
                });
                match stored.or_else(|| {
                    crate::components::ai::anchor::capture_selection_mark(
                        page_now,
                        scale,
                        s.text.clone(),
                        s.context.clone(),
                    )
                }) {
                    Some(m) => add_mark(m),
                    None => {
                        popover_open.set(false);
                        return;
                    }
                }
            }
            (None, None) => {
                // Remount after a document switch (or any open with no
                // pending mark/selection) must clear the flag, not sit on it.
                popover_open.set(false);
                return;
            }
        };

        pending_mark.set(None);
        detail.set(None);
        state.reader.ai_selection.anchor.set(None);
        mark_sig.set(Some(mark.clone()));
        // Re-derive the anchor NOW (same tick) and re-anchor the spring to
        // THIS word: every open morphs out of its own mark, never out of the
        // previous card's resting place.
        watch.refresh.run(());
        if let Some(a) = anchor.get_untracked() {
            spring.reset_to.run(a);
        }
        word.set(mark.word.clone());
        viewport.set(viewport_size());

        drag_offset.set(None);
        dragging.set(false);

        // Exactly one highlighter: the native tint goes the moment the stroke
        // takes over (it would also fight the card's own text selection).
        if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
            let _ = s.remove_all_ranges();
        }

        // Recall, not rescan: a stroke whose answer is already cached morphs
        // straight back open, with no request and no shimmer.
        if let Some(info) = cache.with_value(|c| c.get(&mark.id).cloned()) {
            word_info.set(Some(info));
            error_msg.set(None);
            phase.set(AiPhase::Done);
            processing_id.set(None);
            gphase.set(GlossPhase::Expanded);
            surface_visible.set(true);
            return;
        }

        word_info.set(None);
        error_msg.set(None);
        phase.set(AiPhase::Processing);
        gphase.set(GlossPhase::Processing);
        // No surface while thinking: the highlighter stroke is the only
        // processing UI, so nothing is stacked over the word.
        surface_visible.set(false);
        processing_id.set(Some(mark.id.clone()));

        if !pdf_engine::has_tauri() {
            error_msg.set(Some(
                "AI explanations are only available in the desktop app.".into(),
            ));
            phase.set(AiPhase::Error);
            processing_id.set(None);
            gphase.set(GlossPhase::Expanded);
            surface_visible.set(true);
        } else {
            invoke_explain_word(mark.word, mark.context);
        }
    });

    // ── Effect: the outro's hand-off ─────────────────────────────────────
    // Once the collapsing surface has morphed down onto the anchor, unmount it
    // and let the in-page stroke take over. Doing this on SETTLE rather than on
    // a timer is what keeps the two from being visible at once (the stroke is
    // drawn on the same exact-fit box the surface lands on).
    Effect::new(move |_| {
        if !surface_visible.get() || gphase.get() != GlossPhase::Compact {
            return;
        }
        let Some(a) = anchor.get() else {
            // The mark's page unmounted mid-morph: there is nothing left to
            // land on, so drop the surface now.
            surface_visible.set(false);
            return;
        };
        if sprung.get().is_some_and(|b| boxes_close(b, a, 0.5)) {
            surface_visible.set(false);
        }
    });

    // ── Effect: page-aware auto-collapse ─────────────────────────────────
    // Scrolling no longer kills the card instantly: the card tracks its anchor
    // (the spring chases the moving expanded target) until the origin crosses
    // CARD_EXIT_FRAC of the viewport height, leaves the top, or its page is
    // virtualized away — then it collapses back onto the mark as before.
    Effect::new(move |_| {
        if !surface_visible.get() {
            return;
        }
        if watch.exited.get() && gphase.get() == GlossPhase::Expanded {
            collapse_to_mark();
        }
    });

    // ── Effect: Escape / outside-tap ─────────────────────────────────────
    Effect::new(move |_| {
        if !surface_visible.get() {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() != "Escape" {
                return;
            }
            match gphase.get_untracked() {
                // First Escape closes the card (with the outro); a second one
                // on the bare chip gives up on the gloss entirely.
                GlossPhase::Expanded => collapse_to_mark(),
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
                drag_offset.set(None);
                gphase.set(GlossPhase::Compact);
            }
        });
        on_cleanup(move || {
            key.remove();
            pd.remove();
        });
    });

    // ── Effect: keep viewport size fresh while visible ───────────────────
    Effect::new(move |_| {
        if !surface_visible.get() {
            return;
        }
        let h = window_event_listener_untyped("resize", move |_| {
            viewport.set(viewport_size());
        });
        on_cleanup(move || h.remove());
    });

    // ── Effect: dragging the expanded card ───────────────────────────────
    // Writes an anchor-relative offset so the card keeps gliding with the
    // page on scroll (target = f(live_anchor) + offset).
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
            let x = (me.client_x() as f64 - dx).clamp(CARD_MARGIN, vw - e.w - CARD_MARGIN);
            let y = (me.client_y() as f64 - dy).clamp(CARD_MARGIN, vh - e.h - CARD_MARGIN);
            drag_offset.set(Some((x - e.x, y - e.y)));
        });
        let end = move || {
            grab.set_value(None);
            dragging.set(false);
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
        if surface_visible.get_untracked() {
            collapse_to_mark();
        }
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

    // Surface prop signals (unwrapped — the surface only renders while open).
    let phase_sig = Signal::derive(move || gphase.get());
    let box_sig = Signal::derive(move || sprung.get().unwrap_or_default());
    let expanded_sig = Signal::derive(move || expanded_target.get().unwrap_or_default());
    let progress_sig = Signal::derive(move || progress.get());
    let word_sig = Signal::derive(move || word.get());
    let on_dismiss = Callback::new(move |_: ()| collapse_to_mark());
    let on_drag_start = Callback::new(move |(cx, cy, origin): (f64, f64, GlossBox)| {
        grab.set_value(Some((cx - origin.x, cy - origin.y)));
        dragging.set(true);
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

        <Show when=move || surface_visible.get() && phase.get() != AiPhase::Idle>
            <GlossSurface
                phase=phase_sig
                box_=box_sig
                expanded=expanded_sig
                progress=progress_sig
                word=word_sig
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
