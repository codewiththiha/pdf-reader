//! The name question: an arrival whose name a row on this level already
//! carries, and the three answers — the import's or the move's, decided by
//! what is arriving.

use leptos::prelude::*;

use library_core::conflict::Placement;

use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::question_sheet::QuestionSheet;
use crate::services::library::conflict;
use crate::state::AppState;

use super::info::{more_waiting, NameSheetInfo};

/// The sheet's body, split out so it takes the facts by value: the outer view
/// answers "is there still a question?" on every run, and this one is built
/// once per answer with an answer it can keep.
#[component]
pub(super) fn NameSheet(state: AppState, info: NameSheetInfo) -> impl IntoView {
    let subtitle = more_waiting(format!("Already {}", info.where_line), info.waiting);
    let import = info.import;
    // Which answers this arrival gets, in the order the sheet shows them — the
    // apply's own list rather than a condition the sheet re-derives, so a button
    // the sheet renders is a button the answer will take.
    let offers = info.offers;
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
    } else if offers.contains(&Placement::LinkOnly) {
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
        <QuestionSheet
            heading=heading
            subtitle=subtitle
            question=question
            on_close=Callback::new(move |_| conflict::cancel(state))
        >
                    // One row per answer the ask offers, in its order. An
                    // import's *keep both* is a stored copy of the library's own
                    // and its *make link* is a pointer; a move's are the same two
                    // answers about a row the reader is holding — which is why the
                    // labels differ and the answers do not.
                    //
                    // A `match` that builds the row rather than a tuple of
                    // strings: each row's `on_click` captures the note by move,
                    // and a leptos view is not `Clone`, so the sentence has to be
                    // built in the arm that uses it.
                    {move || {
                        offers
                            .iter()
                            .map(|choice| match choice {
                                Placement::Open => view! {
                                    <ChoiceRow
                                        label="Already imported"
                                        note=go_to_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::Open)
                                        })
                                    />
                                }
                                    .into_any(),
                                Placement::KeepBoth if import => view! {
                                    <ChoiceRow
                                        label="Add as new"
                                        note=new_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::KeepBoth)
                                        })
                                    />
                                }
                                    .into_any(),
                                Placement::KeepBoth => view! {
                                    <ChoiceRow
                                        label="As new"
                                        note=move_new_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::KeepBoth)
                                        })
                                    />
                                }
                                    .into_any(),
                                // The pointer's own sentence is the move's when
                                // the shape keeps both sides, and the import's
                                // otherwise: one promises a dragged book becomes a
                                // pointer, the other promises a row that is not a
                                // copy.
                                Placement::LinkOnly if import => view! {
                                    <ChoiceRow
                                        label="Make link"
                                        note=LINK_NOTE.to_string()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::LinkOnly)
                                        })
                                    />
                                }
                                    .into_any(),
                                Placement::LinkOnly => view! {
                                    <ChoiceRow
                                        label="Make link"
                                        note=link_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::LinkOnly)
                                        })
                                    />
                                }
                                    .into_any(),
                                Placement::Merge => view! {
                                    <ChoiceRow
                                        label="Merge"
                                        note=merge_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::Merge)
                                        })
                                    />
                                }
                                    .into_any(),
                                Placement::Replace => view! {
                                    <ChoiceRow
                                        label="Replace"
                                        note=replace_note.clone()
                                        on_click=Callback::new(move |_| {
                                            conflict::answer_placement(state, Placement::Replace)
                                        })
                                    />
                                }
                                    .into_any(),
                            })
                            .collect::<Vec<_>>()
                    }}
        </QuestionSheet>
    }
}
