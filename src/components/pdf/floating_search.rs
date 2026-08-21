//! Floating search overlay (Chrome/VS-Code style). Phase 2 of the redesign:
//! search is a transient task, so it lives in a floating bar over the viewer
//! rather than in the docked sidebar. Mounted by the coordinator inside
//! `main#viewer-slot` (which is `relative`); the bar positions itself at the
//! slot's top-right, just below the toolbar.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use pdf_core::search::SearchMatch;
use leptos::task::spawn_local;

use crate::components::shared::icon::{Icon, IconName};
use crate::state::ReaderState;
use crate::effects::search_effects::{
    activate_match, clear_search, dismiss_search, run_search, search_navigate,
};


/// How long typing must pause before the query runs.
///
/// Results appear while the reader types, so this is the whole latency budget:
/// long enough that a burst of keystrokes costs one search instead of one per
/// character, short enough to feel immediate. Matches the 180 ms the appearance
/// controls use to coalesce slider input.
const SEARCH_DEBOUNCE_MS: u64 = 180;

/// Compact icon-button class shared by the bar's raw `<button>`s.
const ICON_BTN: &str = "inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";

/// Small up/down chevron used by the prev/next result buttons.
#[component]
fn Chevron(up: bool) -> impl IntoView {
    let paths = if up {
        "<path d='m18 15-6-6-6 6'/>"
    } else {
        "<path d='m6 9 6 6 6-6'/>"
    };
    view! {
        <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=paths
        />
    }
}

#[component]
pub fn FloatingSearch(state: ReaderState) -> impl IntoView {
    let last_query = RwSignal::new(String::new());
    let show_results = RwSignal::new(false);
    // Monotonic id for the newest search; guards against out-of-order
    // completions so only the latest query lands its results and its jump.
    let search_gen = RwSignal::new(0u64);
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    let container_ref: NodeRef<html::Div> = NodeRef::new();

    // Autofocus the query input whenever the overlay becomes visible. Deferred
    // to a microtask so the input node exists by the time it runs (the Show
    // mounts the bar in the same flush the visible flag flips).
    Effect::new(move |_| {
        if state.search.visible.get() {
            queue_microtask(move || {
                if let Some(node) = input_ref.get() {
                    _ = node.focus();
                }
            });
        }
    });

    // Outside-click dismiss: while visible, any pointerdown landing outside the
    // bar's DOM node closes it. The listener is re-registered on each
    // visible-flip and removed on cleanup. Hiding the bar also closes an open
    // results dropdown so reopening starts from a clean slate.
    Effect::new(move |_| {
        if state.search.visible.get() {
            let handle = window_event_listener(
                leptos::ev::pointerdown,
                move |ev: leptos::ev::PointerEvent| {
                    let target: web_sys::Node = event_target(&ev);
                    let contains = container_ref
                        .get()
                        .as_ref()
                        .is_some_and(|c| c.contains(Some(&target)));
                    if !contains {
                        // Leaves the muted highlights behind, like Escape.
                        // The same pointerdown then ends the grace period
                        // unless it landed on the search chrome.
                        dismiss_search(state);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        } else {
            show_results.set(false);
        }
    });

    // --- Live search --------------------------------------------------
    // Typing schedules a search; each keystroke cancels the pending one, so a
    // burst costs a single query. Highlights therefore appear as the reader
    // types, with no Enter required.
    //
    // The debounced pass deliberately does NOT scroll: moving the page under
    // someone who is still typing is disorienting, and the hits they want are
    // usually already on screen. It only paints. Enter (or the next/prev
    // buttons) is what commits to a match and moves the view.
    let debounce_timer = StoredValue::new_local(None::<TimeoutHandle>);
    let cancel_pending = move || {
        debounce_timer.update_value(|t| {
            if let Some(h) = t.take() {
                h.clear();
            }
        });
    };

    // Run the current query now, cancelling anything pending. `then` runs once
    // the results land, and only if this call is still the newest one — a
    // slower earlier search must not steal the view from a newer query.
    let run_now = move |then: Option<Box<dyn Fn()>>| {
        cancel_pending();
        let q = state.search.query.get_untracked();
        if q.trim().is_empty() {
            clear_search(state);
            last_query.set(String::new());
            return;
        }
        search_gen.update(|g| *g += 1);
        let started = search_gen.get_untracked();
        spawn_local(async move {
            run_search(state).await;
            if search_gen.get_untracked() != started || state.search.query.get_untracked() != q {
                return;
            }
            last_query.set(q);
            if let Some(f) = then {
                f();
            }
        });
    };

    let schedule = move || {
        cancel_pending();
        let handle = set_timeout_with_handle(
            move || run_now(None),
            Duration::from_millis(SEARCH_DEBOUNCE_MS),
        )
        .ok();
        debounce_timer.set_value(handle);
    };

    // A pending search must not fire into a torn-down view.
    on_cleanup(cancel_pending);

    let on_input = move |ev: leptos::ev::Event| {
        state.search.query.set(event_target_value(&ev));
        schedule();
    };

    // Enter commits: it selects a match and scrolls it into view. When the
    // results for this query are already in hand it advances to the next match;
    // on a query that has not run yet it searches first, then lands on the
    // first hit (the last one, for Shift+Enter).
    let commit = move |dir: i32| {
        let q = state.search.query.get_untracked();
        let fresh =
            last_query.get_untracked() != q || state.search.matches.with_untracked(Vec::is_empty);
        if !fresh {
            search_navigate(state, dir);
            return;
        }
        run_now(Some(Box::new(move || {
            let len = state.search.matches.with_untracked(Vec::len);
            if len > 0 {
                activate_match(state, if dir < 0 { len - 1 } else { 0 });
            }
        })));
    };

    // Enter = next match, Shift+Enter = previous, Escape = close.
    // stopPropagation on Escape keeps the global shortcut from ALSO closing the
    // sidebar in the same keystroke (the bar is the first dismiss step).
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();
        if key == "Enter" {
            ev.prevent_default();
            commit(if ev.shift_key() { -1 } else { 1 });
        } else if key == "Escape" {
            ev.stop_propagation();
            dismiss_search(state);
        }
    };

    view! {
        <Show when=move || state.search.visible.get()>
            <div
                node_ref=container_ref
                // Opt out of discard-on-interaction: clicks inside the bar are
                // part of searching, not the reader moving on.
                data-search-chrome="true"
                // top-14 == TOOLBAR_H (48px) + the old top-2 (8px) gap. The
                // offset parent is now the full-height content row, which
                // starts at the window top so pages can travel under the glass
                // toolbar; without this the panel would render *behind* the
                // z-50 header and be unclickable.
                class="floating-search-enter absolute right-4 top-14 z-40 w-[min(560px,90vw)] rounded-xl border border-line bg-surface/90 shadow-xl backdrop-blur-md"
            >
                <div class="flex items-center gap-1.5 p-1.5">
                    <button
                        type="button"
                        title="Search (Enter)"
                        on:click=move |_| commit(1)
                        class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-transparent text-muted transition-colors hover:bg-line hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=16 />
                    </button>
                    <input
                        node_ref=input_ref
                        type="text"
                        placeholder="Search in document…"
                        class="h-9 min-w-0 flex-1 rounded-lg border border-line bg-paper px-2.5 text-sm text-ink placeholder:text-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        prop:value=move || state.search.query.get()
                        on:input=on_input
                        on:keydown=on_keydown
                    />
                    // Counter. Before the reader commits to a match there is no
                    // "current" one, but the total still tells them whether the
                    // query hit anything — so show a bare count until then.
                    <span class="whitespace-nowrap px-1 text-xs text-muted tabular-nums">
                        {move || {
                            let total = state.search.total.get();
                            match (state.search.active.get(), total) {
                                (_, 0) if state.search.query.get().trim().is_empty() => String::new(),
                                (_, 0) => "0/0".to_string(),
                                (Some(i), n) => format!("{}/{}", i + 1, n),
                                (None, n) => format!("{n}"),
                            }
                        }}
                    </span>
                    <button
                        type="button"
                        title="Previous match (Shift+Enter)"
                        on:click=move |_| commit(-1)
                        class=ICON_BTN
                    >
                        <Chevron up=true />
                    </button>
                    <button
                        type="button"
                        title="Next match (Enter)"
                        on:click=move |_| commit(1)
                        class=ICON_BTN
                    >
                        <Chevron up=false />
                    </button>
                    <button
                        type="button"
                        title="Close (Esc)"
                        on:click=move |_| dismiss_search(state)
                        class=ICON_BTN
                    >
                        <Icon name=IconName::Close size=16 />
                    </button>
                    <button
                        type="button"
                        title="Toggle results list"
                        on:click=move |_| show_results.update(|v| *v = !*v)
                        class=ICON_BTN
                    >
                        <svg
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                            inner_html=move || {
                                if show_results.get() {
                                    "<path d='m18 15-6-6-6 6'/>"
                                } else {
                                    "<path d='m6 9 6 6 6-6'/>"
                                }
                            }
                        />
                    </button>
                </div>

                <Show when=move || show_results.get()>
                    <div
                        class="absolute right-0 top-full z-50 mt-1 w-full overflow-hidden rounded-xl border border-line bg-surface shadow-xl"
                        on:click=move |_| show_results.set(false)
                    >
                        <div class="max-h-72 overflow-y-auto">
                            <ResultList state=state />
                        </div>
                    </div>
                </Show>
            </div>
        </Show>
    }
}

// Search results list. Used by FloatingSearch overlay.



fn snippet(text: &str) -> String {
    if text.chars().count() > 80 {
        let s: String = text.chars().take(80).collect();
        format!("{s}…")
    } else {
        text.to_string()
    }
}

/// Stable `For` key for a match. The list index is unique on its own (matches
/// are a flat, ordered list), and including page + ordinal keeps the key
/// meaningful when the list is rebuilt for the same query.
pub fn result_key(m: &SearchMatch, index: usize) -> String {
    format!("{}-{}-{}", m.page, m.index, index)
}

/// One result row: page badge + snippet, with the current match highlighted.
#[component]
pub fn ResultRow(state: ReaderState, result: SearchMatch, index: usize) -> impl IntoView {
    let page = result.page;
    let snippet_text = snippet(&result.text);
    // Compare by list index: `active` indexes `matches`, so this stays exact
    // even when a page holds several identical snippets.
    let is_active = move || state.search.active.get() == Some(index);
    // Selecting a row scrolls that MATCH into view (not its page top) and moves
    // the current-match marker onto it.
    let on_click = move |_| activate_match(state, index);
    view! {
        <button
            type="button"
            on:click=on_click
            class=move || {
                let base = "flex w-full flex-col gap-0.5 border-l-2 px-3 py-2 text-left text-sm transition-colors";
                if is_active() {
                    format!("{base} border-accent bg-accent-soft text-ink")
                } else {
                    format!("{base} border-transparent text-muted hover:bg-line hover:text-ink")
                }
            }
        >
            <span class="inline-block w-fit rounded bg-line px-1.5 py-0.5 text-[10px] font-medium text-muted">
                {format!("p. {}", page)}
            </span>
            <span class="block truncate">{snippet_text}</span>
        </button>
    }
}

/// Scrollable list of matches (or a "No results" empty state). Shared by the
/// sidebar SearchPanel and the floating-search results dropdown so the list
/// markup lives in exactly one place.
#[component]
pub fn ResultList(state: ReaderState) -> impl IntoView {
    view! {
        {move || {
            if state.search.matches.get().is_empty() {
                view! {
                    <div class="p-4 text-sm text-muted">No results</div>
                }
                .into_any()
            } else {
                view! {
                    <For
                        each=move || {
                            state
                                .search
                                .matches
                                .get()
                                .into_iter()
                                .enumerate()
                                .collect::<Vec<_>>()
                        }
                        key=move |(index, m): &(usize, SearchMatch)| result_key(m, *index)
                        children=move |(index, m): (usize, SearchMatch)| {
                            view! { <ResultRow state=state result=m index=index /> }
                        }
                    />
                }
                .into_any()
            }
        }}
    }
}
