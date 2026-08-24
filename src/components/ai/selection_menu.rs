use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};
use crate::state::AppState;

/// A small floating pill that appears near the user's text selection.
/// Contains the "Info" button that opens the AI popover.
///
/// The root carries `data-ai-popover`: the engine's selection tracker
/// treats mousedowns inside that attribute as AI-UI interaction and does
/// NOT clear the selection detail — otherwise the button would swallow
/// its own click (the press collapses the selection before click fires).
#[component]
pub fn SelectionMenu(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // Just below the selection, centered horizontally; `position:fixed`
    // keeps it in the same viewport coordinate space as the engine's rect.
    let style = Signal::derive(move || {
        let Some(sel) = detail.get() else {
            return String::new();
        };
        let rect = &sel.rect;
        let left = rect.x + rect.width / 2.0;
        let top = rect.y + rect.height + 8.0;
        format!(
            "position:fixed; left:{left}px; top:{top}px; \
             transform:translateX(-50%); z-index:80;"
        )
    });

    let visible = Signal::derive(move || detail.get().is_some() && !popover_open.get());

    view! {
        <Show when=move || visible.get()>
            <div
                data-ai-popover=""
                style=move || style.get()
                class="ai-selection-menu-enter"
            >
                <button
                    type="button"
                    title="Explain with AI"
                    aria-label="Explain selected text with AI"
                    // Preventing the mousedown default keeps the document
                    // selection (and focus) alive, so the highlight stays
                    // visible behind the card this button opens — and the
                    // button can never be unmounted by its own press.
                    on:mousedown=move |ev| ev.prevent_default()
                    on:click=move |_| {
                        popover_open.set(true);
                    }
                    class="flex min-h-11 items-center gap-1.5 rounded-full border border-line \
                           bg-surface px-5 text-sm font-medium tracking-wide text-ink \
                           shadow-[var(--gloss-shadow-menu)] \
                           transition-[transform,background-color] duration-150 ease-out \
                           active:scale-[0.96] \
                           focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                    <Icon name=IconName::More size=13 />
                    <span>"Info"</span>
                </button>
            </div>
        </Show>
    }
}
