use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};
use crate::state::AppState;

/// A small floating button that appears near the user's text selection.
/// Currently contains only the "Info" button; clicking it opens the AI
/// explanation popover (rendered in a later step).
///
/// The root carries `data-ai-popover`: the engine's selection tracker treats
/// mousedowns inside that attribute as AI-UI interaction and does NOT clear
/// the selection detail — otherwise the button would swallow its own click
/// (the press collapses the document selection before the click fires).
#[component]
pub fn SelectionMenu(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // Just below the selection, centered horizontally; `position:fixed` keeps
    // it in the same viewport coordinate space as the engine's rect. The
    // translateX(-50%) centering is mirrored in the selection-menu-in
    // keyframes (dropping it there would make the button jump sideways).
    let style = Signal::derive(move || {
        let Some(sel) = detail.get() else {
            return String::new();
        };
        let rect = &sel.rect;
        let left = rect.x + rect.width / 2.0;
        let top = rect.y + rect.height + 6.0;
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
                class="selection-menu-enter"
            >
                <button
                    type="button"
                    title="Explain with AI"
                    aria-label="Explain selected text with AI"
                    on:click=move |_| {
                        popover_open.set(true);
                    }
                    class="flex items-center gap-1.5 rounded-full border border-line \
                           bg-surface px-3 py-1.5 text-xs font-medium text-ink \
                           shadow-lg backdrop-blur-sm \
                           transition-colors hover:bg-line \
                           focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                    <Icon name=IconName::More size=13 />
                    <span>"Info"</span>
                </button>
            </div>
        </Show>
    }
}
