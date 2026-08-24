//! The right-click menu for a single mark. One action by design — Remove
//! highlight — parked at the cursor (clamped into the viewport by
//! [`super::select_mode::use_select_mode`]).

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::select_mode::{park_undo, ContextTarget, UndoBatch};
use crate::state::AppState;

#[component]
pub fn GlossContextMenu(
    state: AppState,
    ctrl: GlossController,
    menu: RwSignal<Option<ContextTarget>>,
    undo: RwSignal<Option<UndoBatch>>,
) -> impl IntoView {
    view! {
        <Show when=move || menu.with(|m| m.is_some())>
            {move || {
                menu.get().map(|t| {
                    let id = t.id.clone();
                    view! {
                        <div
                            class="gloss-context-menu fixed z-[70] min-w-[176px] rounded-xl \
                                   border border-line bg-surface p-1 \
                                   shadow-[var(--gloss-shadow-menu)]"
                            role="menu"
                            style=format!("left:{}px;top:{}px", t.x, t.y)
                        >
                            <button
                                type="button"
                                role="menuitem"
                                class="flex w-full items-center gap-2 rounded-lg px-3 py-2 \
                                       text-left text-sm text-red-400 transition-colors \
                                       hover:bg-line \
                                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                on:click=move |_| {
                                    let removed = ctrl.remove_marks.run(vec![id.clone()]);
                                    let path = state.reader.document.path.get_untracked();
                                    park_undo(undo, removed, path);
                                    menu.set(None);
                                }
                            >
                                "Remove highlight"
                            </button>
                        </div>
                    }
                })
            }}
        </Show>
    }
}
