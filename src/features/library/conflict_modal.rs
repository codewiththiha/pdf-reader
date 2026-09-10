//! The "already imported?" sheet: the level already holds a book of this name,
//! and the question is which of three things the reader meant.
//!
//! One sheet, one question, three rows, and no second ask anywhere in it —
//! nothing on offer here is destructive, so there is nothing to warn twice
//! about. *Already imported* places nothing and takes the reader to the row
//! that is already there. *Add as new* keeps both, under the next free name.
//! *Make link* puts a pointer on this level instead of a copy, which is the
//! answer for a reader who wants the book reachable from here and does not
//! want a second one.
//!
//! The service half — what a collision is, what each answer writes — is
//! `crate::services::library::conflict` and the rule itself is
//! `library_core::conflict`; this file is the ask.
//!
//! Cancel — the button, the backdrop and the Escape key — drops the question on
//! screen and every one waiting behind it, which is what a file manager's copy
//! dialog has always meant by Cancel: the placements already answered keep
//! their answers and the ones not asked simply do not land.

use leptos::prelude::*;

use app_chrome::floating::dismiss::use_modal_escape;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use library_core::shelf::ALL_SHELF;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::services::library::conflict::{self, ConflictAsk};
use library_core::conflict::Answer;
use crate::state::AppState;

/// The sheet, mounted once by the library page.
///
/// The open flag lives on the library state rather than in a provided handle
/// (the remove sheet's shape) because the raisers are services: an import asks
/// from inside a spawned future no component owns, and a signal on the state is
/// the one door every raiser and this view already share.
#[component]
pub(crate) fn ConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.conflict_open;
    use_overlay_lane(open, OverlayPolicy::MODAL);
    // A popover opened inside the sheet owns the press; the shared rule peels
    // one layer at a time.
    use_modal_escape(open);

    // A close that came from the lane registry or the Escape key wrote only the
    // boolean; the question and the ones waiting behind it go with it, so the
    // sheet can never reopen onto a question somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.conflict.set(None);
            state.library.conflict_waiting.set(Vec::new());
        }
    });

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| conflict::cancel(state)
            >
                {move || {
                    let ask = state.library.conflict.get()?;
                    let info = Info::of(state, &ask);
                    Some(view! { <Sheet state=state info=info /> })
                }}
            </div>
        </Show>
    }
}

/// Everything the sheet prints, read once per answer — the remove receipt's
/// rule: a `view!` body is a builder, not a place to compute, and a heading and
/// two buttons that need the same name must not each derive their own.
struct Info {
    /// The name arriving — the heading.
    incoming: String,
    /// The name already on the level, which *already imported* goes to and
    /// *make link* points at.
    existing_name: String,
    /// The name *add as new* would mint, promised on its own row: "keep both"
    /// without the name is an answer the reader has to take on faith.
    new_name: String,
    /// Where the row that collided is: a shelf the sentence can name, or the
    /// library's own unfiled list, which has no name but "your library".
    where_line: String,
    /// How many questions wait behind this one.
    waiting: usize,
}

impl Info {
    fn of(state: AppState, ask: &ConflictAsk) -> Self {
        let where_line = if ask.arrival.shelf_id == ALL_SHELF {
            "in your library".to_string()
        } else {
            let name = state.library.shelves.with_untracked(|shelves| {
                shelves
                    .iter()
                    .find(|s| s.id == ask.arrival.shelf_id)
                    .map(|s| s.name.clone())
            });
            match name {
                Some(name) => format!("on “{name}”"),
                None => "on this shelf".to_string(),
            }
        };
        // One read of both lists, so the promise on the row and the answer the
        // click gives are counted against the same library.
        let (rows, shelves) = state.library.snapshot_rows();
        let new_name = library_core::conflict::next_name(
            &rows,
            &shelves,
            &ask.arrival.shelf_id,
            &ask.arrival.name,
        );
        Self {
            incoming: ask.arrival.name.clone(),
            existing_name: ask.existing_name.clone(),
            new_name,
            where_line,
            waiting: state.library.conflict_waiting.with_untracked(|w| w.len()),
        }
    }
}

/// The sheet's body, split out so it takes the facts by value: the outer view
/// answers "is there still a question?" on every run, and this one is built
/// once per answer with an answer it can keep.
#[component]
fn Sheet(state: AppState, info: Info) -> impl IntoView {
    let subtitle = if info.waiting > 0 {
        format!("Already {} · {} more waiting", info.where_line, info.waiting)
    } else {
        format!("Already {}", info.where_line)
    };
    let question = format!(
        "“{}” is already {}. Add a second book of its own, put a link here \
         instead, or go to the one you have.",
        info.incoming, info.where_line
    );
    let heading = info.incoming.clone();
    let tooltip = heading.clone();
    let go_to_note = format!(
        "Add nothing — go to “{}” where it already is",
        info.existing_name
    );
    let new_note = format!(
        "Keeps both, under the next free name — “{}”",
        info.new_name
    );
    const LINK_NOTE: &str = "A pointer row, not a copy: tapping it goes to the book where it lives";

    view! {
        <div
            class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
            style="width:min(92vw, 420px)"
            on:click=move |ev| ev.stop_propagation()
            role="dialog"
            aria-label="The library already holds a book of that name"
        >
            <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
                <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-semibold text-ink" title=tooltip>
                        {heading}
                    </span>
                    <span class="mt-0.5 block text-xs text-muted">{subtitle}</span>
                </span>
                <IconButton
                    icon=IconName::Close
                    title="Close"
                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                    on_click=move || conflict::cancel(state)
                />
            </header>

            <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
                <p class="text-xs text-muted">{question}</p>
                <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                    <ChoiceRow
                        label="Already imported"
                        note=go_to_note
                        on_click=Callback::new(move |_| conflict::answer(state, Answer::GoToExisting))
                    />
                    <ChoiceRow
                        label="Add as new"
                        note=new_note
                        on_click=Callback::new(move |_| conflict::answer(state, Answer::AsNew))
                    />
                    <ChoiceRow
                        label="Make link"
                        note=LINK_NOTE.to_string()
                        on_click=Callback::new(move |_| conflict::answer(state, Answer::AsLink))
                    />
                </div>
            </div>

            <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                <Button
                    on_click=move |_| conflict::cancel(state)
                    variant=ButtonVariant::Ghost
                    title="Leave the shelf as it is"
                >
                    <span>"Cancel"</span>
                </Button>
            </footer>
        </div>
    }
}

/// One of the three answers: the name of it, and the one line that says what it
/// does. No icons — the rows are a sentence each, and a glyph beside a sentence
/// is decoration the reader has to look past.
#[component]
fn ChoiceRow(label: &'static str, note: String, on_click: Callback<()>) -> impl IntoView {
    view! {
        <button
            type="button"
            class="flex w-full flex-col gap-0.5 px-3.5 py-2.5 text-left transition-colors \
                   hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            on:click=move |_| on_click.run(())
        >
            <span class="text-sm text-ink">{label}</span>
            <span class="text-xs text-muted">{note}</span>
        </button>
    }
}
