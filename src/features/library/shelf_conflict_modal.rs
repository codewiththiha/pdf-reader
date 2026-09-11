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
//! nothing this exists to stop — and gets the same answers, worded as the
//! continuation it is. The gate in front of all of it is about read-at-place,
//! and it turns one shape away while sending another to this sheet: a pick of
//! a RUNG inside a tree the library reads in place never reaches here at all
//! — a linked shelf is the OS folder, and one folder is one shelf, so the
//! import answers with the "already in the library" modal
//! (`already_imported_modal`) and a highlight instead. The tree's own root
//! re-picked AS COPIES is an arrival, and it wears this sheet's chrome with a
//! question of its own — the MODE SWITCH, whose three answers are about how
//! the folder is held from here on: *as new* mints a second shelf whose books
//! are copies of their own and leaves the old tree reading the folder; *merge
//! into it* keeps the shelf that is here and turns every book on it into the
//! library's copy where it stands, name, highlights and place in it all
//! surviving the flip; *replace* sends the read-at-place books out of the
//! library, highlights and all — promised on the row, counted at the render —
//! and seats the copies on the shelf they left. No *make link*: a pointer at
//! the folder's own shelf, from an import of that very folder, would point at
//! the thing being imported. A read-at-place arrival keeps the ordinary sheet
//! only when a DIFFERENT folder holds the name, and then it offers two
//! answers, not three — *as new* of a linked folder is the second instance
//! the gate exists to prevent. A stored arrival keeps all three: its copies
//! are the library's own.
//!
//! A drag never asks this: nesting a shelf writes a parent rather than a
//! membership, so nothing arrives on a level for a name to collide with — the
//! rule `crate::services::library::arrange` gives. And a watched folder's own
//! rescan never asks either: staying quiet is a rescan's whole job.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
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
                // The mode switch is the continuation's louder cousin: the
                // library reads this folder where it stands, and the import
                // asks for copies of its own — so the answers are about how
                // the folder is HELD from here on, and the sheet says what
                // each holding does to the books that are here.
                let mode_switch = ask.mode_switch;
                // A read-at-place import never offers *as new*: the folder's
                // shelf is the OS folder, and a counter-named twin of it would
                // be a second door onto the same ground. The mode switch
                // needs no such rule — its *as new* twin holds copies, books
                // of their own bytes, and the switch's branch renders it
                // whatever the toggle below says.
                let in_place = ask.opts.in_place;
                // The name *as new* would mint, counted against the level's own
                // shelves at the click — the row promises the counter rather
                // than asking the reader to take "the next free name" on
                // faith.
                let new_name = state.library.shelves.with_untracked(|shelves| {
                    library_core::conflict::next_shelf_name(shelves, None, &ask.incoming_name)
                });
                let heading = ask.incoming_name.clone();
                let subtitle = if mode_switch {
                    format!("Read in place as “{}”", ask.existing_name)
                } else if own {
                    format!("Already in the library as “{}”", ask.existing_name)
                } else {
                    format!("A shelf called “{}” is already here", ask.existing_name)
                };
                // The replace row's promise, counted at the render rather
                // than taken on faith: how many read-at-place books the
                // answer sends out of the library.
                let replace_rows = if mode_switch {
                    crate::services::library::import::mode_switch_replace_rows(state, &ask.root)
                        .len()
                } else {
                    0
                };
                let question = if mode_switch {
                    format!(
                        "The library already reads “{}” where it stands, and this import asks \
                         for copies the library stores itself. Give the copies a shelf of the \
                         next free name and leave the tree that is here reading the folder, \
                         switch the shelf that is here over to copies — every book keeping its \
                         name, its highlights and its place in it — or replace its books with \
                         copies.",
                        ask.existing_name
                    )
                } else if own {
                    format!(
                        "This folder is already in the library — “{}” is the shelf its last \
                         import made. Continue the import into it, give it a shelf of the next \
                         free name, or leave a pointer here instead.",
                        ask.existing_name
                    )
                } else if in_place {
                    format!(
                        "You are importing the folder “{}”, and this level already has a shelf \
                         called that. A folder read at its place cannot mint a second shelf of \
                         itself — leave a pointer to the shelf that is here, or file the \
                         folder's books into it.",
                        ask.incoming_name
                    )
                } else {
                    format!(
                        "You are importing the folder “{}”, and this level already has a shelf \
                         called that. Give the arriving folder the next free name, leave a \
                         pointer to the shelf that is here, or file the folder's books into it.",
                        ask.incoming_name
                    )
                };
                let new_note = if mode_switch {
                    format!(
                        "Import as “{new_name}” — its own shelf of the library's copies; the \
                         tree that is here keeps reading the folder"
                    )
                } else {
                    format!("Import as “{new_name}” — its own shelf, its own tree")
                };
                const LINK_NOTE: &str = "A pointer row, not a second shelf: nothing is \
                                         imported, and tapping it lights the folder where it is";
                let merge_note = if mode_switch {
                    format!(
                        "“{}” keeps standing — every book on it becomes the library's copy, \
                         keeping its name, its highlights and its place in it; files the \
                         folder gained join as copies",
                        ask.existing_name
                    )
                } else {
                    format!(
                        "The folder's books join “{}”; a book whose name it already holds is \
                         asked one by one",
                        ask.existing_name
                    )
                };
                let replace_note = match replace_rows {
                    0 => format!(
                        "Nothing is left to remove — the library's copies simply take “{}”",
                        ask.existing_name
                    ),
                    1 => format!(
                        "One read-at-place book leaves the library, with its highlights — a \
                         copy takes its place on “{}”",
                        ask.existing_name
                    ),
                    n => format!(
                        "{n} read-at-place books leave the library, with their highlights — \
                         copies take “{}”",
                        ask.existing_name
                    ),
                };
                Some(view! {
                    <>
                        <SheetHeader
                            heading=heading
                            subtitle=subtitle
                            on_close=Callback::new(move |_| conflict::cancel_shelf(state))
                        />

                        <SheetBody>
                            <p class="text-xs text-muted">{question}</p>
                            <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                                {if mode_switch {
                                    // The switch's three: a second shelf of
                                    // copies, the shelf that is here switched
                                    // over, or copies in place of the books
                                    // that are here. No link — a pointer at
                                    // the folder's own shelf, from an import
                                    // of that very folder, points at the thing
                                    // being imported.
                                    view! {
                                        <>
                                            <ChoiceRow
                                                label="Add as new"
                                                note=new_note
                                                on_click=Callback::new(move |_| {
                                                    conflict::answer_shelf(state, ShelfAnswer::AsNew)
                                                })
                                            />
                                            <ChoiceRow
                                                label="Merge into it"
                                                note=merge_note
                                                on_click=Callback::new(move |_| {
                                                    conflict::answer_shelf(state, ShelfAnswer::Merge)
                                                })
                                            />
                                            <ChoiceRow
                                                label="Replace"
                                                note=replace_note
                                                on_click=Callback::new(move |_| {
                                                    conflict::answer_shelf(state, ShelfAnswer::Replace)
                                                })
                                            />
                                        </>
                                    }
                                        .into_any()
                                } else {
                                    view! {
                                        <>
                                            {(!in_place).then(move || {
                                                view! {
                                                    <ChoiceRow
                                                        label="Add as new"
                                                        note=new_note.clone()
                                                        on_click=Callback::new(move |_| {
                                                            conflict::answer_shelf(state, ShelfAnswer::AsNew)
                                                        })
                                                    />
                                                }
                                            })}
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
                                        </>
                                    }
                                        .into_any()
                                }}
                            </div>
                        </SheetBody>

                        <SheetFooter>
                            <Button
                                on_click=move |_| conflict::cancel_shelf(state)
                                variant=ButtonVariant::Ghost
                                title="Import nothing"
                            >
                                <span>"Cancel"</span>
                            </Button>
                        </SheetFooter>
                    </>
                })
            }}
        </ModalShell>
    }
}
