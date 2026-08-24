//! The "Removed n highlights — Undo" toast. Parking through
//! [`super::select_mode::park_undo`] means EVERY removal path (context menu,
//! bar) gets undo for free; the batch is pinned to its document path so an
//! undo after a document switch drops instead of resurrecting marks into the
//! wrong file.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::select_mode::UndoBatch;
use crate::state::AppState;

#[component]
pub fn GlossUndoToast(
    state: AppState,
    ctrl: GlossController,
    undo: RwSignal<Option<UndoBatch>>,
) -> impl IntoView {
    view! {
        <Show when=move || undo.with(|u| u.is_some())>
            {move || {
                undo.get().map(|batch| {
                    let n = batch.marks.len();
                    let restored = batch.marks.clone();
                    let batch_path = batch.path.clone();
                    view! {
                        <div
                            class="gloss-undo-toast fixed bottom-5 left-1/2 z-[70] flex \
                                   items-center gap-3 rounded-full border border-line \
                                   bg-surface py-2 pl-4 pr-2 text-sm text-ink \
                                   shadow-[var(--gloss-shadow-menu)]"
                            role="status"
                        >
                            <span>
                                {format!("Removed {n} highlight{}", if n == 1 { "" } else { "s" })}
                            </span>
                            <button
                                type="button"
                                class="rounded-full px-3 py-1 text-sm font-semibold text-accent \
                                       transition-colors hover:bg-line \
                                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                on:click=move |_| {
                                    // A batch belongs to the document it came
                                    // from; after a switch, drop it instead of
                                    // resurrecting marks into the wrong file.
                                    if state.reader.document.path.get_untracked() == batch_path {
                                        ctrl.restore_marks.run(restored.clone());
                                    }
                                    undo.set(None);
                                }
                            >
                                "Undo"
                            </button>
                        </div>
                    }
                })
            }}
        </Show>
    }
}
