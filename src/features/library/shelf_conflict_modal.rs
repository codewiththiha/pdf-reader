//! The folder's question: a level already holds the name.
//!
//! A folder arriving under a name its level holds is the shelf's own spelling
//! of the collision the book sheet asks about — two doors of one name on one
//! level are two doors a reader cannot tell apart — and it is asked BEFORE the
//! walk rather than after it, because the answer decides what the walk is for:
//! a shelf of its own under the next free name, a pointer at the shelf that is
//! already here, or the shelf that is here itself, with the folder's books
//! joining it. Two of the three answers start the import run again with a plan
//! (`crate::services::library::import::RootPlan`); the third walks away with a
//! pointer row and no import at all.
//!
//! A folder colliding with its OWN previous shelf asks too — a re-import that
//! ended on "Imported 0 books" with no sheet in between was the silent
//! nothing this exists to stop — and gets the same three answers, worded as
//! the continuation it is: the counter-named tree holds the folder's books as
//! memberships of the rows the library already holds, and the merge is the
//! reconcile a re-import asks for, its per-file questions asked one by one.
//!
//! A drag never asks this: nesting a shelf writes a parent rather than a
//! membership, so nothing arrives on a level for a name to collide with — the
//! rule `crate::services::library::arrange` gives. And a watched folder's own
//! rescan never asks either: staying quiet is a rescan's whole job.

use leptos::prelude::*;

use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::features::library::conflict_modal::ChoiceRow;
use crate::services::library::conflict::{self, ShelfAnswer};
use crate::state::AppState;

#[component]
pub(crate) fn ShelfConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_conflict_open;

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question goes with it, so the sheet
    // can never reopen onto a folder somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.shelf_conflict.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="A shelf of that name is already here"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.shelf_conflict.get()?;
                // A folder colliding with its OWN previous shelf is a
                // continuation, and the sheet WORDS it as one — but the three
                // answers are the three answers either way: an *as new* tree
                // of one folder holds that folder's books as memberships of
                // the rows the library already holds, which is a second
                // arrangement and never a second copy.
                let own = ask.own;
                // The name *as new* would mint, counted against the level's own
                // shelves at the click — the row promises the counter rather
                // than asking the reader to take "the next free name" on
                // faith.
                let new_name = state.library.shelves.with_untracked(|shelves| {
                    library_core::conflict::next_shelf_name(shelves, None, &ask.incoming_name)
                });
                let heading = ask.incoming_name.clone();
                let tooltip = heading.clone();
                let subtitle = if own {
                    format!("Already in the library as “{}”", ask.existing_name)
                } else {
                    format!("A shelf called “{}” is already here", ask.existing_name)
                };
                let question = if own {
                    format!(
                        "This folder is already in the library — “{}” is the shelf its last \
                         import made. Continue the import into it, give it a shelf of the next \
                         free name, or leave a pointer here instead.",
                        ask.existing_name
                    )
                } else {
                    format!(
                        "You are importing the folder “{}”, and this level already has a shelf \
                         called that. Give the arriving folder the next free name, leave a \
                         pointer to the shelf that is here, or file the folder's books into it.",
                        ask.incoming_name
                    )
                };
                let new_note =
                    format!("Import as “{new_name}” — its own shelf, its own tree");
                const LINK_NOTE: &str = "A pointer row, not a second shelf: nothing is \
                                         imported, and tapping it lights the folder where it is";
                let merge_note = format!(
                    "The folder's books join “{}”; a book whose name it already holds is \
                     asked one by one",
                    ask.existing_name
                );
                Some(view! {
                    <>
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
                                on_click=move || conflict::cancel_shelf(state)
                            />
                        </header>

                        <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
                            <p class="text-xs text-muted">{question}</p>
                            <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                                <ChoiceRow
                                    label="Add as new"
                                    note=new_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, ShelfAnswer::AsNew)
                                    })
                                />
                                <ChoiceRow
                                    label="Make link"
                                    note=LINK_NOTE.to_string()
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, ShelfAnswer::Link)
                                    })
                                />
                                <ChoiceRow
                                    label="Merge into it"
                                    note=merge_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_shelf(state, ShelfAnswer::Merge)
                                    })
                                />
                            </div>
                        </div>

                        <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                            <Button
                                on_click=move |_| conflict::cancel_shelf(state)
                                variant=ButtonVariant::Ghost
                                title="Import nothing"
                            >
                                <span>"Cancel"</span>
                            </Button>
                        </footer>
                    </>
                })
            }}
        </ModalShell>
    }
}
