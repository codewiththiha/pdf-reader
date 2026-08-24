//! The gloss state-machine hub: every signal the popover juggles plus the
//! behaviours all paths share — `reset` (full dismiss), `collapse_to_mark`
//! (the outro), `add_mark` (dedup + persist) and `retry` (re-run a failed
//! lookup). Also owns the open-event listener and the open effect, so
//! [`super::popover`] reads as wiring + view.
//!
//! Open path is deterministic: both the Info pill and a saved stroke dispatch
//! `pdfreader:gloss-open` with the mark in the event detail. The listener
//! sets `pending_mark` and bumps `open_req` (which the open effect tracks),
//! so the effect always runs with a mark in hand — never a race against
//! `detail` being cleared or a stale `popover_open = true` no-op.

use std::collections::HashMap;

use leptos::prelude::*;
use pdf_core::gloss::GlossMark;

use crate::components::ai::anchor::AnchorWatch;
use crate::components::ai::gloss::marks::GLOSS_OPEN_EVENT;
use crate::components::ai::gloss::spring::SpringBox;
use crate::components::ai::gloss::util::viewport_size;
use crate::components::ai::types::{AiError, AiErrorKind, AiPhase, GlossPhase, WordInfo};
use crate::services::ai::invoke_explain_word;
use crate::state::AppState;

/// Per-document cap on persisted marks (oldest evicted). A reading session's
/// worth of looked-up words, bounded so localStorage can't grow without end.
pub const MARK_CAP: usize = 200;

/// All gloss state + the shared behaviours. Every field is `Copy`, so hooks
/// can take the controller by value without lifetime gymnastics.
#[derive(Clone, Copy)]
pub struct GlossController {
    // ── Data phase + content ──────────────────────────────────────────
    pub phase: RwSignal<AiPhase>,
    pub word: RwSignal<String>,
    pub word_info: RwSignal<Option<WordInfo>>,
    /// The typed failure behind `AiPhase::Error`, if any. Drives both the
    /// friendly message and the retry affordance in the surface.
    pub error: RwSignal<Option<AiError>>,

    // ── Geometry phase ────────────────────────────────────────────────
    pub gphase: RwSignal<GlossPhase>,

    // ── Anchor: the persisted mark this card belongs to ───────────────
    pub mark_sig: RwSignal<Option<GlossMark>>,
    pub pending_mark: RwSignal<Option<GlossMark>>,
    pub open_req: RwSignal<u64>,

    /// Whether the morphing surface exists at all. Distinct from
    /// `popover_open`: during processing the stroke IS the UI, and after the
    /// outro morph the surface unmounts while the gloss stays "open" on its
    /// mark.
    pub surface_visible: RwSignal<bool>,

    // ── Drag state (the pointer physics live in [`super::drag`]) ──────
    pub drag_offset: RwSignal<Option<(f64, f64)>>,
    pub dragging: RwSignal<bool>,
    pub grab: StoredValue<Option<(f64, f64)>, LocalStorage>,

    /// Answers already fetched this session, keyed by mark id. Re-opening a
    /// stroke is recall, not a rescan.
    pub cache: StoredValue<HashMap<String, WordInfo>, LocalStorage>,

    // ── Behaviours ────────────────────────────────────────────────────
    pub reset: Callback<()>,
    pub collapse_to_mark: Callback<()>,
    pub add_mark: Callback<GlossMark, GlossMark>,
    pub retry: Callback<()>,
}

pub fn use_gloss_controller(state: AppState) -> GlossController {
    let popover_open = state.reader.ai_selection.popover_open;
    let processing_id = state.reader.gloss.processing_id;
    let marks = state.reader.gloss.marks;
    let doc_path = state.reader.document.path;

    let phase = RwSignal::new(AiPhase::Idle);
    let word = RwSignal::new(String::new());
    let word_info = RwSignal::new(None::<WordInfo>);
    let error = RwSignal::new(None::<AiError>);
    let gphase = RwSignal::new(GlossPhase::Processing);
    let mark_sig = RwSignal::new(None::<GlossMark>);
    let pending_mark = RwSignal::new(None::<GlossMark>);
    let open_req = RwSignal::new(0u64);
    let surface_visible = RwSignal::new(false);
    let drag_offset = RwSignal::new(None::<(f64, f64)>);
    let dragging = RwSignal::new(false);
    let grab = StoredValue::new_local(None::<(f64, f64)>);
    let cache = StoredValue::new_local(HashMap::<String, WordInfo>::new());

    // Full dismiss back to Idle. NOTE: the mark itself is intentionally kept
    // — the highlight is the point, and it is what reopens this card later.
    let reset = Callback::new(move |_| {
        popover_open.set(false);
        phase.set(AiPhase::Idle);
        word.set(String::new());
        word_info.set(None);
        error.set(None);
        gphase.set(GlossPhase::Processing);
        surface_visible.set(false);
        processing_id.set(None);
        mark_sig.set(None);
        drag_offset.set(None);
        dragging.set(false);
        grab.set_value(None);
    });

    // The outro: fold the expanded card back down onto the word. Every close
    // path funnels through here, and the popover's settle watcher unmounts
    // the surface once the spring has actually landed on the stroke.
    let collapse_to_mark = Callback::new(move |_| {
        if gphase.get_untracked() != GlossPhase::Expanded || dragging.get_untracked() {
            return;
        }
        drag_offset.set(None);
        gphase.set(GlossPhase::Compact);
    });

    // Record + persist a freshly captured mark, and hand back the CANONICAL
    // one: re-explaining the same word at the same spot reuses the existing
    // mark rather than stacking a second stroke on it. Returning it matters —
    // the id is what keys the processing glow and the answer cache, so the
    // caller must not go on holding the discarded duplicate.
    let add_mark = Callback::new(move |m: GlossMark| -> GlossMark {
        let existing = marks.with_untracked(|v| {
            v.iter().find(|o| o.same_spot(&m)).cloned()
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
        if let Some(path) = doc_path.get_untracked() {
            crate::storage::persist_gloss(&path, &marks.get_untracked());
        }
        m
    });

    // Retry the current mark after a retryable failure: the same opening
    // ritual minus persistence (the mark is already canonical), so the stroke
    // thinks again and the surface is reborn on the first fresh chunk.
    let retry = Callback::new(move |_| {
        let Some(mark) = mark_sig.get_untracked() else {
            return;
        };
        if !pdf_engine::has_tauri() {
            return; // the environment cannot change mid-session
        }
        error.set(None);
        word_info.set(None);
        phase.set(AiPhase::Processing);
        gphase.set(GlossPhase::Processing);
        surface_visible.set(false);
        processing_id.set(Some(mark.id.clone()));
        invoke_explain_word(mark.word, mark.context);
    });

    GlossController {
        phase,
        word,
        word_info,
        error,
        gphase,
        mark_sig,
        pending_mark,
        open_req,
        surface_visible,
        drag_offset,
        dragging,
        grab,
        cache,
        reset,
        collapse_to_mark,
        add_mark,
        retry,
    }
}

/// Every open (stroke click OR Info pill) arrives as a CustomEvent that
/// carries the mark and bumps the nonce. Tracking `open_req` is what makes a
/// second open of an already-open popover re-run the open effect.
pub fn use_open_listener(state: AppState, ctrl: GlossController) {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    let handle = window_event_listener(
        leptos::ev::Custom::new(GLOSS_OPEN_EVENT),
        move |ev: web_sys::CustomEvent| {
            let Ok(m) = serde_wasm_bindgen::from_value::<GlossMark>(ev.detail()) else {
                return;
            };
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
            ctrl.pending_mark.set(Some(m));
            ctrl.open_req.update(|n| *n += 1);
            popover_open.set(true);
        },
    );
    on_cleanup(move || handle.remove());
}

/// The open effect: re-runs on EVERY request (nonce). The mark always arrives
/// via `pending_mark` — both entry points (Info pill and stroke click)
/// dispatch `GLOSS_OPEN_EVENT`, so an open with no pending mark can only mean
/// a stale flag (e.g. a remount after a document switch), which is cleared.
pub fn use_open_effect(
    state: AppState,
    ctrl: GlossController,
    watch: AnchorWatch,
    spring: SpringBox,
    viewport: RwSignal<(f64, f64)>,
) {
    let popover_open = state.reader.ai_selection.popover_open;
    let detail = state.reader.ai_selection.detail;
    let processing_id = state.reader.gloss.processing_id;

    Effect::new(move |_| {
        let _ = ctrl.open_req.get(); // tracked nonce
        if !popover_open.get() {
            return;
        }

        let Some(mark) = ctrl.pending_mark.get_untracked() else {
            // Remount after a document switch (or any open with no pending
            // mark) must clear the flag, not sit on it.
            popover_open.set(false);
            return;
        };

        // Toggle knowledge lives here (and only here): marks stay dumb open
        // dispatchers. A re-click on the active expanded card folds it down;
        // a click while its model run is still processing is ignored. Compact
        // or mid-outro re-clicks deliberately fall through to recall/reopen.
        let same_spot = ctrl
            .mark_sig
            .with_untracked(|m| m.as_ref().is_some_and(|m| m.same_spot(&mark)));
        if same_spot {
            match (
                ctrl.gphase.get_untracked(),
                ctrl.surface_visible.get_untracked(),
            ) {
                (GlossPhase::Processing, _) => {
                    ctrl.pending_mark.set(None);
                    return;
                }
                (GlossPhase::Expanded, true) => {
                    ctrl.pending_mark.set(None);
                    ctrl.collapse_to_mark.run(());
                    return;
                }
                _ => {}
            }
        }

        // Self-contained open: mark is already in hand (Info pill or stroke
        // click). Persist it so re-open/re-explain reuse the id.
        let mark = ctrl.add_mark.run(mark);

        ctrl.pending_mark.set(None);
        detail.set(None);
        state.reader.ai_selection.anchor.set(None);
        ctrl.mark_sig.set(Some(mark.clone()));

        // Re-derive the anchor NOW (same tick) and re-anchor the spring to
        // THIS word: every open morphs out of its own mark, never out of the
        // previous card's resting place.
        watch.refresh.run(());
        if let Some(a) = watch.screen.get_untracked() {
            spring.reset_to.run(a);
        }

        ctrl.word.set(mark.word.clone());
        viewport.set(viewport_size());
        ctrl.drag_offset.set(None);
        ctrl.dragging.set(false);

        // Exactly one highlighter: the native tint goes the moment the stroke
        // takes over (it would also fight the card's own text selection).
        if let Some(Some(s)) = web_sys::window().and_then(|w| w.get_selection().ok()) {
            let _ = s.remove_all_ranges();
        }

        // Recall, not rescan: a stroke whose answer is already cached morphs
        // straight back open, with no request and no shimmer.
        if let Some(info) = ctrl.cache.with_value(|c| c.get(&mark.id).cloned()) {
            ctrl.word_info.set(Some(info));
            ctrl.error.set(None);
            ctrl.phase.set(AiPhase::Done);
            processing_id.set(None);
            ctrl.gphase.set(GlossPhase::Expanded);
            ctrl.surface_visible.set(true);
            return;
        }

        ctrl.word_info.set(None);
        ctrl.error.set(None);
        ctrl.phase.set(AiPhase::Processing);
        ctrl.gphase.set(GlossPhase::Processing);
        // No surface while thinking: the highlighter stroke is the only
        // processing UI, so nothing is stacked over the word.
        ctrl.surface_visible.set(false);
        processing_id.set(Some(mark.id.clone()));

        if !pdf_engine::has_tauri() {
            ctrl.error.set(Some(AiError {
                kind: AiErrorKind::Other("desktop-only".into()),
                message: "AI explanations are only available in the desktop app.".into(),
                retryable: false,
            }));
            ctrl.phase.set(AiPhase::Error);
            processing_id.set(None);
            ctrl.gphase.set(GlossPhase::Expanded);
            ctrl.surface_visible.set(true);
        } else {
            invoke_explain_word(mark.word, mark.context);
        }
    });
}
