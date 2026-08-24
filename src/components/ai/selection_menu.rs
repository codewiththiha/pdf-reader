use leptos::prelude::*;

use crate::components::ai::anchor::{watch_page_anchor, MENU_EXIT_FRAC};
use crate::components::primitives::icon::{Icon, IconName};
use crate::state::AppState;

/// A small floating pill that appears near the user's text selection.
/// Contains the "Info" button that opens the AI popover.
///
/// Position is re-derived from a page-space anchor on every scroll/zoom/mode
/// change, so the pill travels with the word and disappears once the origin
/// fully leaves the viewport.
///
/// The root carries `data-ai-popover`: the engine's selection tracker
/// treats mousedowns inside that attribute as AI-UI interaction and does
/// NOT clear the selection detail — otherwise the button would swallow
/// its own click (the press collapses the selection before click fires).
#[component]
pub fn SelectionMenu(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    let watch = watch_page_anchor(
        Signal::derive(move || state.reader.ai_selection.anchor.get()),
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.mode.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        MENU_EXIT_FRAC,
    );

    // Once the selection's origin leaves the viewport, the menu is gone for
    // good (same as before: the next selection replaces it).
    Effect::new(move |_| {
        if watch.exited.get() && detail.get().is_some() {
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
        }
    });

    // Live position: re-derived from the page host, so it travels with scroll.
    let style = Signal::derive(move || {
        let Some(b) = watch.screen.get() else {
            return String::new();
        };
        let left = b.x + b.w / 2.0;
        let top = b.y + b.h + 8.0;
        format!(
            "position:fixed; left:{left}px; top:{top}px; \
             transform:translateX(-50%); z-index:80;"
        )
    });
    let visible = Signal::derive(move || {
        detail.get().is_some()
            && !popover_open.get()
            && !watch.exited.get()
            && watch.screen.get().is_some()
    });

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
