//! Multi-select management for gloss marks: the shared state helpers, the
//! long-press gesture constants (the gesture itself lives in [`super::marks`]),
//! the exit paths (Escape / clean tap outside), the right-click context-menu
//! listener, and the undo pipeline that every removal path parks through.
//!
//! Selection state itself lives on `state.reader.gloss` so every page's
//! `GlossMarkLayer` and the reader-level bar share one source of truth.
//! Marks mutate it directly (toggling is high-frequency); only the context
//! menu travels as a CustomEvent, mirroring `GLOSS_OPEN_EVENT`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use leptos::prelude::*;
use pdf_core::gloss::GlossMark;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::util::viewport_size;
use crate::state::AppState;

/// Name of the "right-clicked a mark" event (marks.rs → popover).
pub const GLOSS_CONTEXT_EVENT: &str = "pdfreader:gloss-context";

/// How long a press must hold before it becomes a selection gesture.
pub const LONG_PRESS_MS: i32 = 450;

/// Pointer may drift this far (px) during a long-press without cancelling it.
pub const LONG_PRESS_SLOP_PX: f64 = 8.0;

/// How long the undo toast stays up before the removal is final.
pub const UNDO_WINDOW_MS: i32 = 6000;

/// Payload of [`GLOSS_CONTEXT_EVENT`]: where the menu should open and which
/// mark it acts on. Client coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextTarget {
    pub x: f64,
    pub y: f64,
    pub id: String,
}

/// A removed batch parked for undo. `path` pins the batch to the document it
/// came from: undoing after a document switch would resurrect marks into the
/// wrong file, so a stale batch is dropped instead.
#[derive(Debug, Clone)]
pub struct UndoBatch {
    pub generation: u64,
    pub path: Option<String>,
    pub marks: Vec<GlossMark>,
}

/// Monotonic batch id: the auto-dismiss timer only clears ITS batch, so a
/// quick second removal can never be eaten by the first one's timer.
static UNDO_GEN: AtomicU64 = AtomicU64::new(1);

/// Dispatch [`GLOSS_CONTEXT_EVENT`] (fired by a mark's contextmenu handler).
pub fn dispatch_gloss_context(x: f64, y: f64, id: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(detail) = serde_wasm_bindgen::to_value(&ContextTarget { x, y, id: id.into() })
    else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(GLOSS_CONTEXT_EVENT, &init) {
        let _ = win.dispatch_event(&ev);
    }
}

/// Toggle one id in the selection set.
pub fn toggle_selected(selected: RwSignal<HashSet<String>>, id: &str) {
    selected.update(|s| {
        if !s.remove(id) {
            s.insert(id.to_string());
        }
    });
}

/// Exit selection mode and drop the selection.
pub fn exit_selection(state: AppState) {
    state.reader.gloss.selection_active.set(false);
    state.reader.gloss.selected_marks.set(HashSet::new());
}

/// Park a removed batch for undo and arm the auto-dismiss timer.
pub fn park_undo(
    undo: RwSignal<Option<UndoBatch>>,
    marks: Vec<GlossMark>,
    path: Option<String>,
) {
    if marks.is_empty() {
        return;
    }
    let generation = UNDO_GEN.fetch_add(1, Ordering::Relaxed);
    undo.set(Some(UndoBatch { generation, path, marks }));
}

/// Handles owned by [`use_select_mode`].
pub struct SelectMode {
    /// The right-click context menu target (client coords + mark id).
    pub menu: RwSignal<Option<ContextTarget>>,
    /// The batch currently parked for undo, if any.
    pub undo: RwSignal<Option<UndoBatch>>,
}

/// Selection-mode wiring that lives at the popover level: entry guard, exit
/// paths, the context-menu listener + dismissal, and the undo signal.
pub fn use_select_mode(state: AppState, ctrl: GlossController) -> SelectMode {
    let selecting = state.reader.gloss.selection_active;
    let menu = RwSignal::new(None::<ContextTarget>);
    let undo = RwSignal::new(None::<UndoBatch>);

    // Replacing the parked batch replaces its timer. Cleanup cancels the old
    // handle, while the generation guard makes a stale callback harmless even
    // if it was already queued when replacement happened.
    Effect::new(move |_| {
        let Some(generation) = undo.get().map(|batch| batch.generation) else {
            return;
        };
        let handle = set_timeout_with_handle(
            move || {
                undo.update(|current| {
                    if current.as_ref().is_some_and(|batch| batch.generation == generation) {
                        *current = None;
                    }
                });
            },
            Duration::from_millis(UNDO_WINDOW_MS as u64),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });

    // Entering selection mode folds any open card: selection is about the
    // strokes, and the bar wants its corner of the screen to itself.
    Effect::new(move |_| {
        if selecting.get() {
            ctrl.collapse_to_mark.run(());
            menu.set(None);
        }
    });

    // Escape exits selection mode.
    Effect::new(move |_| {
        if !selecting.get() {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() == "Escape" {
                exit_selection(state);
            }
        });
        on_cleanup(move || key.remove());
    });

    // A clean tap anywhere that is not a mark, the bar, or a menu exits.
    // (Mark clicks stop propagation; drag-scrolls never synthesize clicks.)
    Effect::new(move |_| {
        if !selecting.get() {
            return;
        }
        let h = window_event_listener_untyped("click", move |ev: web_sys::Event| {
            let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };
            if el.closest(".gloss-mark, .gloss-select-bar, .gloss-context-menu")
                .ok()
                .flatten()
                .is_some()
            {
                return;
            }
            exit_selection(state);
        });
        on_cleanup(move || h.remove());
    });

    // Right-click on a mark asks for the remove menu — only outside
    // selection mode; inside it, right-click toggles selection (marks.rs).
    let ctx = window_event_listener(
        leptos::ev::Custom::new(GLOSS_CONTEXT_EVENT),
        move |ev: web_sys::CustomEvent| {
            if selecting.get_untracked() {
                return;
            }
            let Ok(t) = serde_wasm_bindgen::from_value::<ContextTarget>(ev.detail()) else {
                return;
            };
            // Clamp into the viewport with room for the menu's own size.
            let (vw, vh) = viewport_size();
            menu.set(Some(ContextTarget {
                x: t.x.clamp(8.0, (vw - 190.0).max(8.0)),
                y: t.y.clamp(8.0, (vh - 60.0).max(8.0)),
                id: t.id,
            }));
        },
    );
    on_cleanup(move || ctx.remove());

    // The menu's own dismissal: Escape or a press anywhere outside it.
    Effect::new(move |_| {
        if menu.with(|m| m.is_none()) {
            return;
        }
        let key = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
            if ke.key() == "Escape" {
                menu.set(None);
            }
        });
        let pd = window_event_listener_untyped("pointerdown", move |ev: web_sys::Event| {
            if let Some(el) = ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                && el.closest(".gloss-context-menu").ok().flatten().is_some()
            {
                return;
            }
            menu.set(None);
        });
        on_cleanup(move || {
            key.remove();
            pd.remove();
        });
    });

    SelectMode { menu, undo }
}
