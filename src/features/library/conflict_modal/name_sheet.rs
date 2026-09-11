//! The name question: an arrival whose name a row on this level already
//! carries, and the three answers — the import's or the move's, decided by
//! what is arriving.

use leptos::prelude::*;

use library_core::conflict::{Answer, MoveAnswer};

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict;
use crate::state::AppState;

use super::info::Info;

/// The sheet's body, split out so it takes the facts by value: the outer view
/// answers "is there still a question?" on every run, and this one is built
/// once per answer with an answer it can keep.
#[component]
pub(super) fn NameSheet(state: AppState, info: Info) -> impl IntoView {
    let subtitle = if info.waiting > 0 {
        format!("Already {} · {} more waiting", info.where_line, info.waiting)
    } else {
        format!("Already {}", info.where_line)
    };
    let import = info.import;
    let link_offer = info.link_offer;
    let question = if import {
        // An import's *add as new* is a stored copy of the library's own — a
        // book of its own bytes, whatever the level's twin reads — so no
        // arrival ever has the row withheld: nothing a file's answers can do
        // is a second door on one linked file.
        format!(
            "“{}” is already {}. Add a second book of its own, put a link here \
             instead, or go to the one you have.",
            info.incoming, info.where_line
        )
    } else if link_offer {
        format!(
            "A book called “{}” is already {}, and it is one of the library's own copies. \
             Keep one book, reach the copy from here, or keep both under a new name.",
            info.existing_name, info.where_line
        )
    } else {
        format!(
            "A book called “{}” is already {}. Keep one book, keep this one \
             instead, or keep both under a new name.",
            info.existing_name, info.where_line
        )
    };
    let heading = info.incoming.clone();
    let go_to_note = format!(
        "Add nothing — go to “{}” where it already is",
        info.existing_name
    );
    let new_note = format!(
        "Keeps both, under the next free name — “{}”",
        info.new_name
    );
    const LINK_NOTE: &str = "A pointer row, not a copy: tapping it goes to the book where it lives";
    // The move's own three, and Replace is the one that says what it takes:
    // the row it names leaves the library, and a highlight count the reader can
    // see before the click is the difference between a receipt and a surprise.
    let merge_note = format!(
        "One book — “{}” stays, and takes this one's shelves, its highlights, \
         and the further place in it",
        info.existing_name
    );
    let replace_note = if info.marks > 0 {
        format!(
            "“{}” leaves the library, with its {} — this one takes its place \
             on every shelf it was on",
            info.existing_name,
            library_core::text::plural(info.marks, "highlight", "highlights")
        )
    } else {
        format!(
            "“{}” leaves the library — this one takes its place on every shelf \
             it was on",
            info.existing_name
        )
    };
    let move_new_note = format!("Keeps both — this one becomes “{}”", info.new_name);
    let link_note = format!(
        "The book you dragged becomes a pointer here — “{}” stays, the file on disk stays, \
         and nothing is destroyed",
        info.existing_name
    );

    view! {
        <>
            <SheetHeader
                heading=heading
                subtitle=subtitle
                on_close=Callback::new(move |_| conflict::cancel(state))
            />

            <SheetBody>
                <p class="text-xs text-muted">{question}</p>
                <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                    {if import {
                        // A file arriving: what to put on this level.
                        view! {
                            <>
                                <ChoiceRow
                                    label="Already imported"
                                    note=go_to_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer(state, Answer::GoToExisting)
                                    })
                                />
                                <ChoiceRow
                                    label="Add as new"
                                    note=new_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer(state, Answer::AsNew)
                                    })
                                />
                                <ChoiceRow
                                    label="Make link"
                                    note=LINK_NOTE.to_string()
                                    on_click=Callback::new(move |_| {
                                        conflict::answer(state, Answer::AsLink)
                                    })
                                />
                            </>
                        }
                            .into_any()
                    } else {
                        // A row being moved: which of the two books this level
                        // keeps.
                        view! {
                            <>
                                <ChoiceRow
                                    label="Merge"
                                    note=merge_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_move(state, MoveAnswer::Merge)
                                    })
                                />
                                {if link_offer {
                                    // The pointer shape: the destructive answer is
                                    // replaced by the one that keeps both sides.
                                    view! {
                                        <ChoiceRow
                                            label="Make link"
                                            note=link_note
                                            on_click=Callback::new(move |_| {
                                                conflict::answer_move(state, MoveAnswer::Link)
                                            })
                                        />
                                    }
                                        .into_any()
                                } else {
                                    view! {
                                        <ChoiceRow
                                            label="Replace"
                                            note=replace_note
                                            on_click=Callback::new(move |_| {
                                                conflict::answer_move(state, MoveAnswer::Replace)
                                            })
                                        />
                                    }
                                        .into_any()
                                }}
                                <ChoiceRow
                                    label="As new"
                                    note=move_new_note
                                    on_click=Callback::new(move |_| {
                                        conflict::answer_move(state, MoveAnswer::AsNew)
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
                    on_click=move |_| conflict::cancel(state)
                    variant=ButtonVariant::Ghost
                    title="Leave the shelf as it is"
                >
                    <span>"Cancel"</span>
                </Button>
            </SheetFooter>
        </>
    }
}
