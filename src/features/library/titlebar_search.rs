//! The library's search bar, in the title bar's centre slot.
//!
//! The reader's floating search is an overlay over a document and behaves like
//! one (it opens on a shortcut, it dismisses on an outside press, it owns a
//! results list). This is a filter over a shelf, so it borrows the LOOK and
//! nothing else: an always-present pill in the bar's centre slot, typing into it
//! narrows the grid on the spot, and `Escape` clears it rather than closing it —
//! a control that is always on the page cannot be dismissed (though it peels the
//! suggestions off first, one layer per press).
//!
//! Since the filter, the bar also ANSWERS: while a query is in it, a panel
//! under the pill offers the books that query is probably about — ranked and
//! fuzzy-matched by `library_core::query`, drawn by
//! `crate::features::library::search_suggest` — and taking one goes to the book
//! on its shelf rather than opening it, because the shelf behind the panel is
//! already the filtered answer and the suggestion is the shortcut through it.
//!
//! The suggestions are computed at the keystroke, untracked, and stored — not
//! held in a standing derivation. Nothing re-ranks while the reader is not
//! typing: a books-changed signal (an import finishing mid-search) does not
//! owe a closed panel a pass over the library, and an open one gets the fresh
//! ranking with the next keystroke anyway. One pass per keystroke, top rows
//! kept, no index to keep in step.
//!
//! The grammar, all of it on the input: typing opens the panel when there is
//! something to show; the arrows move through it and leave it at its ends;
//! `Enter` takes the chosen row to its shelf (and, with the panel shut, is
//! nobody's business but the shelf's); `Escape` peels one layer — the panel
//! first, the text second. A press anywhere else, or the input losing focus,
//! closes the panel without touching the text: the filter and the suggestions
//! are two answers, and dismissing one is not dismissing the other.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use library_core::query::{self, Suggestion, SUGGEST_LIMIT};
use library_core::text::plural;

use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::events::FOCUS_LIBRARY_SEARCH_EVENT;
use crate::features::library::search_suggest::SearchSuggestions;
use crate::services::library::reveal_book;
use crate::state::AppState;

/// The bar's placeholder, which is also its only claim about the library: how
/// many books a query is being run against.
fn placeholder(state: AppState) -> Signal<String> {
    Signal::derive(move || {
        let books = state
            .library
            .books
            .with(|rows| library_core::book::book_rows(rows).count());
        match books {
            0 => "Search the library".to_string(),
            n => format!("Search {}", plural(n, "book", "books")),
        }
    })
}

#[component]
pub(crate) fn TitlebarSearch(state: AppState) -> impl IntoView {
    let hint = placeholder(state);
    let has_query = Signal::derive(move || state.library.query.with(|q| !q.is_empty()));
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    // The popover anchors to the pill's box, not the input: a panel the width
    // of the pill reads as the pill's own continuation.
    let anchor: NodeRef<html::Div> = NodeRef::new();

    // The panel's three facts. `open` is the popover's own signal — its
    // dismissal writes it too — so everything the bar does with suggestions
    // goes through it, and a closed panel renders and computes nothing.
    let open = RwSignal::new(false);
    let active = RwSignal::new(0usize);
    let suggestions: RwSignal<Vec<Suggestion>> = RwSignal::new(Vec::new());
    // The pill's measured width, taken at the open rather than tracked: the
    // panel is as wide as the bar it hangs from, and a resize mid-search
    // re-measures at the next one.
    let panel_width = RwSignal::new(320u32);

    // One keystroke, one pass: rank the library against the text as it now
    // stands, untracked, and let the panel show the answer or not open at all.
    let show = move || {
        let q = state.library.query.get_untracked();
        let rows = if query::is_active(&q) {
            state
                .library
                .books
                .with_untracked(|rows| query::suggest(rows, &q, SUGGEST_LIMIT))
        } else {
            Vec::new()
        };
        let opening = !rows.is_empty() && !open.get_untracked();
        if opening {
            if let Some(node) = anchor.get_untracked() {
                let wide = node.get_bounding_client_rect().width();
                if wide > 0.0 {
                    panel_width.set(wide as u32);
                }
            }
        }
        suggestions.set(rows);
        active.set(0);
        open.set(!suggestions.with_untracked(|s| s.is_empty()));
    };

    // What a row means, on the pointer and on the keyboard alike: the book's
    // shelf, then the book itself — and the panel shuts, because an answer
    // taken is an answer finished.
    let pick = move |id: String| {
        reveal_book(state, &id);
        open.set(false);
    };

    // Cmd/Ctrl+F lands here while the shelf is what is on screen. The shortcut
    // layer dispatches and forgets: it has no business knowing that the library's
    // bar owns an input node, and the bar has no business owning a key binding.
    let focus_handle =
        window_event_listener(
            leptos::ev::Custom::new(FOCUS_LIBRARY_SEARCH_EVENT),
            move |_: web_sys::CustomEvent| {
                if let Some(node) = input_ref.get_untracked() {
                    _ = node.focus();
                    node.select();
                }
            },
        );
    on_cleanup(move || focus_handle.remove());

    view! {
        <div node_ref=anchor class="relative w-full max-w-xl">
            <div
                class="pointer-events-auto flex w-full items-center gap-2 rounded-full \
                       border border-line bg-surface/70 px-3 py-1.5 backdrop-blur \
                       focus-within:border-accent"
            >
                <Icon name=IconName::Search size=15 class="shrink-0 text-muted" />
                <input
                    node_ref=input_ref
                    type="text"
                    role="combobox"
                    aria-label="Search the library"
                    aria-expanded=move || open.get()
                    aria-controls="lib-suggest-list"
                    aria-autocomplete="list"
                    aria-activedescendant=move || {
                        if open.get() {
                            format!("lib-sug-{}", active.get())
                        } else {
                            String::new()
                        }
                    }
                    autocomplete="off"
                    spellcheck="false"
                    placeholder=move || hint.get()
                    prop:value=move || state.library.query.get()
                    on:input=move |ev| {
                        state.library.query.set(event_target_value(&ev));
                        show();
                    }
                    on:focus=move |_| show()
                    on:blur=move |_| open.set(false)
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        // The global shortcuts stand down inside a form control,
                        // so this bar owns its own keys.
                        match ev.key().as_str() {
                            "ArrowDown" => {
                                ev.prevent_default();
                                if open.get_untracked() {
                                    let len = suggestions.with_untracked(|s| s.len());
                                    if len > 0 {
                                        active.update(|a| *a = (*a + 1).min(len - 1));
                                    }
                                } else {
                                    // The arrow is also an ask: a shut panel with
                                    // something to say says it.
                                    show();
                                }
                            }
                            "ArrowUp" => {
                                ev.prevent_default();
                                if open.get_untracked() {
                                    // Off the top, the arrow leaves the panel and
                                    // returns the reader to the text they typed.
                                    if active.get_untracked() == 0 {
                                        open.set(false);
                                    } else {
                                        active.update(|a| *a -= 1);
                                    }
                                }
                            }
                            "Enter" => {
                                if open.get_untracked() {
                                    let chosen = suggestions.with_untracked(|s| {
                                        s.get(active.get_untracked().min(s.len().saturating_sub(1)))
                                            .map(|row| row.book.id.clone())
                                    });
                                    if let Some(id) = chosen {
                                        ev.prevent_default();
                                        pick(id);
                                    }
                                }
                            }
                            "Escape" => {
                                // One layer per press: the suggestions first, the
                                // text second. Clearing both at once would make
                                // the panel's dismissal destroy work.
                                if open.get_untracked() {
                                    ev.stop_propagation();
                                    open.set(false);
                                } else if has_query.get_untracked() {
                                    ev.stop_propagation();
                                    state.library.query.set(String::new());
                                    suggestions.set(Vec::new());
                                }
                            }
                            _ => {}
                        }
                    }
                    class="w-full min-w-0 bg-transparent text-sm text-ink placeholder:text-muted \
                           focus:outline-none"
                />
                {move || {
                    has_query.get().then(|| {
                        view! {
                            <button
                                type="button"
                                aria-label="Clear the search"
                                title="Clear the search"
                                on:click=move |_| {
                                    state.library.query.set(String::new());
                                    suggestions.set(Vec::new());
                                    open.set(false);
                                }
                                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full \
                                       text-muted transition-colors hover:bg-line hover:text-ink \
                                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            >
                                <Icon name=IconName::Close size=12 />
                            </button>
                        }
                    })
                }}
            </div>
            // The same anchored popover every bar menu hangs from — fixed, so
            // the centre slot's clipping cannot reach it, in the popover lane,
            // so an opening menu or modal replaces it instead of stacking over
            // it. `pointer-events-auto` because the slot it is born in hands
            // pointer events to nothing.
            <MenuPopover
                open=open
                anchor=anchor
                width=Signal::derive(move || panel_width.get())
                coordinate_space="toolbar-row"
                class="pointer-events-auto p-1.5"
            >
                <SearchSuggestions
                    state=state
                    suggestions=suggestions.read_only()
                    active=active
                    pick=Callback::new(pick)
                />
            </MenuPopover>
        </div>
    }
}
