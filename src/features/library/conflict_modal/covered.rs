//! The covered file's sheet: a loose import of a file that sits inside a folder
//! the library reads in place, where that folder's book for it is alive and
//! standing. Two answers, because the third a name collision offers — a second
//! row of one linked file — is the one thing a read-at-place folder never makes.

use leptos::prelude::*;

use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::question_sheet::QuestionSheet;
use crate::services::library::conflict::{ConflictAsk, CoveredAnswer, self};
use crate::services::library::folder_label;

use super::info::{more_waiting, where_line};
use crate::state::AppState;


/// The covered-file sheet: a loose import of a file that sits inside a
/// folder the library reads in place, where the folder's book for it is
/// alive and standing.
///
/// The folder's rule has already answered the questions a name collision
/// would ask — a second linked row of one read-at-place file is a duplicate
/// the library does not make — so what is left is two: the library's own
/// stored copy on this level, a book of its own bytes with its own
/// highlights and its own place in it, unbound from the folder's tree; or
/// the book the folder holds, gone to and lit. The switch gives every other
/// waiting file of the folder the same answer, because what one file of a
/// folder says is usually what forty of it say.
#[component]
pub(super) fn CoveredSheet(state: AppState, ask: ConflictAsk) -> impl IntoView {
    let apply_all = RwSignal::new(false);
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_covered()).count());
    let incoming = ask.arrival.name.clone();
    let heading = incoming.clone();
    let folder_name = ask
        .kind
        .folder_id()
        .and_then(|folder_id| state.library.folder(folder_id).map(|f| folder_label(&f.root)))
        .unwrap_or_else(|| "a folder read in place".to_string());
    let book_name = ask.existing_name.clone();
    let subtitle = more_waiting(format!("Inside “{folder_name}”"), waiting);
    let where_line = where_line(state, &ask.arrival.shelf_id);
    let question = format!(
        "“{incoming}” is inside “{folder_name}”, which the library reads in place — one \
         book per file, never a second link. Import your own copy {where_line}, or go to \
         the book the folder holds."
    );
    let import_note = format!(
        "The library's own copy — its own book {where_line}, its own highlights, its own \
         place in it"
    );
    let show_note =
        format!("Add nothing — go to “{book_name}” inside “{folder_name}” and light it up");

    view! {
        <QuestionSheet
            heading=heading
            subtitle=subtitle
            question=question
            on_close=Callback::new(move |_| conflict::cancel(state))
            apply_all=Some((waiting, apply_all))
        >
                    <ChoiceRow
                        label="Import a copy here"
                        note=import_note
                        on_click=Callback::new(move |_| {
                            conflict::answer_covered(
                                state,
                                CoveredAnswer::ImportHere,
                                apply_all.get_untracked(),
                            )
                        })
                    />
                    <ChoiceRow
                        label="Show the imported one"
                        note=show_note
                        on_click=Callback::new(move |_| {
                            conflict::answer_covered(
                                state,
                                CoveredAnswer::GoToExisting,
                                apply_all.get_untracked(),
                            )
                        })
                    />
        </QuestionSheet>
    }
}
