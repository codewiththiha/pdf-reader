//! The covered file's sheet: a loose import of a file that sits inside a folder
//! the library reads in place, where that folder's book for it is alive and
//! standing. Two answers, because the third a name collision offers — a second
//! row of one linked file — is the one thing a read-at-place folder never makes.

use leptos::prelude::*;

use library_core::shelf::ALL_SHELF;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict::{ConflictAsk, CoveredAnswer, self};
use crate::services::library::folder_label;
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
    let subtitle = if waiting > 0 {
        format!("Inside “{folder_name}” · {waiting} more waiting")
    } else {
        format!("Inside “{folder_name}”")
    };
    let where_line = if ask.arrival.shelf_id == ALL_SHELF {
        "in your library".to_string()
    } else {
        // Empty means the shelf went while the sheet was up, which reads the
        // same as the root's: a level with no name to speak.
        match state.library.shelf_name(&ask.arrival.shelf_id) {
            name if !name.is_empty() => format!("on “{name}”"),
            _ => "on this shelf".to_string(),
        }
    };
    let question = format!(
        "“{incoming}” is inside “{folder_name}”, a folder the library reads in place, and the \
         library already holds the book it is. A folder read in place holds one book per file \
         and never a second link — so import your own copy {where_line}, stored and owned by \
         the library, or go to the book the folder holds."
    );
    let import_note = format!(
        "Copy the file into the library — its own book {where_line}, with its own highlights \
         and its own place in it"
    );
    let show_note =
        format!("Add nothing — go to “{book_name}” inside “{folder_name}” and light it up");

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
                </div>
                {(waiting > 0).then(|| {
                    let label = format!("Apply to all {}", waiting + 1);
                    view! {
                        <div class="mt-3 flex items-center justify-between gap-3 rounded-xl border border-line px-3 py-2">
                            <span class="text-xs text-muted">{label}</span>
                            <Switch
                                checked=Signal::derive(move || apply_all.get())
                                on_change=Callback::new(move |on| apply_all.set(on))
                                title="Give every waiting question this same answer"
                                    .to_string()
                            />
                        </div>
                    }
                })}
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
