//! The bottom-right selection action bar: count + This page / All /
//! Remove (n) / Done. Rendered by the popover's `SelectMode` (its `undo`
//! signal is the undo pipeline); visible exactly while selection mode is
//! active. Position + elevation come from the `ActionBar` primitive; this
//! file keeps only the actions.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::selection_mode::{exit_selection, park_undo};
use crate::components::primitives::overlay::action_bar::ActionBar;
use crate::state::AppState;

/// Shared button styling for the quiet actions.
const BAR_BTN: &str = "rounded-full px-3 py-1.5 text-xs font-medium text-ink \
                       transition-colors hover:bg-line \
                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";

#[component]
pub fn GlossSelectBar(
    state: AppState,
    ctrl: GlossController,
    /// Where removals are parked for undo.
    undo: RwSignal<Option<crate::components::ai::gloss::selection_mode::UndoBatch>>,
) -> impl IntoView {
    let selecting = state.reader.gloss.selection_active;
    let selected = state.reader.gloss.selected_marks;
    let marks = state.reader.gloss.marks;
    let page = state.reader.viewer.page;

    let count = Signal::derive(move || selected.with(|s| s.len()));

    let select_page = move |_| {
        let p = page.get_untracked();
        selected.update(|s| {
            marks.with_untracked(|v| {
                for m in v.iter().filter(|m| m.page == p) {
                    s.insert(m.id.clone());
                }
            });
        });
    };

    let select_all = move |_| {
        selected.update(|s| {
            marks.with_untracked(|v| {
                for m in v {
                    s.insert(m.id.clone());
                }
            });
        });
    };

    let remove = move |_| {
        let ids: Vec<String> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        let removed = ctrl.remove_marks.run(ids);
        let path = state.reader.document.path.get_untracked();
        park_undo(undo, removed, path);
        exit_selection(state);
    };

    let done = move |_| exit_selection(state);

    view! {
        <ActionBar
            visible=Signal::derive(move || selecting.get())
            role="toolbar"
            aria_label="Gloss mark selection"
            class="gloss-select-bar"
        >
            <span class="mr-1.5 text-xs font-medium tabular-nums text-muted">
                {move || format!("{} selected", count.get())}
            </span>
            <button type="button" class=BAR_BTN on:click=select_page>
                "This page"
            </button>
            <button type="button" class=BAR_BTN on:click=select_all>
                "All"
            </button>
            <button
                type="button"
                disabled=move || count.get() == 0
                aria-label=move || format!("Remove {} selected highlights", count.get())
                class="rounded-full px-3 py-1.5 text-xs font-semibold text-red-400 \
                       transition-colors hover:bg-line \
                       disabled:cursor-not-allowed disabled:opacity-40 \
                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                on:click=remove
            >
                {move || format!("Remove ({})", count.get())}
            </button>
            <button
                type="button"
                class=BAR_BTN
                aria-label="Done selecting"
                on:click=done
            >
                "Done"
            </button>
        </ActionBar>
    }
}
