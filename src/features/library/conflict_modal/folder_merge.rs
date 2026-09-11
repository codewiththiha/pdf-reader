//! The compact per-file sheet a folder merge asks: two names, three answers,
//! and the switch that gives every waiting question the same answer in one
//! click.

use leptos::prelude::*;

use library_core::book::find_row;
use library_core::conflict::next_name;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::choice_row::ChoiceRow;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict::{ConflictAsk, FolderMergeAnswer, self};
use crate::state::AppState;


/// The compact per-file sheet a folder merge asks: two names, three answers,
/// and — behind the row of them — the switch that gives every waiting question
/// the same answer in one click.
///
/// The move sheet's three, re-spelled for an arrival with no row of its own:
/// there is nothing to fold INTO the shelf's row yet, so *merge* is the file
/// handing the row its measurement rather than two rows becoming one. Smaller
/// than the import sheet on purpose: the shelf's question is already answered,
/// and a sheet that re-explained the whole situation per file would be a
/// sentence the reader has to re-read forty times.
#[component]
pub(super) fn FolderMergeSheet(state: AppState, ask: ConflictAsk) -> impl IntoView {
    // The switch starts off on every question: "apply to all" is the reader's
    // answer per sheet, not a preference the first click leaves behind.
    let apply_all = RwSignal::new(false);
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.kind.is_folder_merge()).count());
    let incoming = ask.arrival.name.clone();
    let existing = ask.existing_name.clone();
    let heading = incoming.clone();
    let subtitle = if waiting > 0 {
        format!("Into “{existing}” · {} more waiting", waiting)
    } else {
        format!("Into “{existing}”")
    };
    // The file arriving is the very file the row on the shelf reads — a
    // re-import of a read-at-place folder's own book. *As new* of it would be
    // a second row of one linked file, which the library does not make, so
    // the sheet offers the two answers that add no copy. A different file
    // wearing the same name keeps all three, and a STORED folder keeps all
    // three too: its *as new* is a second copy in the store, a book of its
    // own bytes rather than a second door on one file.
    let twin = ask.kind.in_place()
        && state.library.books.with_untracked(|rows| {
            ask.arrival.file.as_ref().is_some_and(|file| {
                find_row(rows, &ask.existing_id)
                    .and_then(|row| row.book())
                    .is_some_and(|book| book.path() == file.path)
            })
        });
    let question = if twin {
        format!(
            "“{incoming}” is arriving, and “{existing}” on this shelf reads this very file. \
             Keep the one that is here, or seat this file in its place."
        )
    } else {
        format!(
            "“{incoming}” is arriving, and “{existing}” is already on this shelf. \
             Keep the one that is here, seat this file in its place, or keep both \
             under a name of its own."
        )
    };
    let merge_note =
        format!("One book — “{existing}” stays, and takes this file's measurement");
    const REPLACE_NOTE: &str =
        "The row on the shelf leaves the library; this file takes its slot";
    let new_name = {
        let (rows, shelves) = state.library.snapshot_rows();
        next_name(&rows, &shelves, &ask.arrival.shelf_id, &incoming)
    };
    let new_note = format!("Keep both — this file becomes “{new_name}”");

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
                        label="Merge"
                        note=merge_note
                        on_click=Callback::new(move |_| {
                            conflict::answer_folder_merge(
                                state,
                                FolderMergeAnswer::Merge,
                                apply_all.get_untracked(),
                            )
                        })
                    />
                    <ChoiceRow
                        label="Replace"
                        note=REPLACE_NOTE.to_string()
                        on_click=Callback::new(move |_| {
                            conflict::answer_folder_merge(
                                state,
                                FolderMergeAnswer::Replace,
                                apply_all.get_untracked(),
                            )
                        })
                    />
                    {(!twin).then(move || {
                        view! {
                            <ChoiceRow
                                label="As new"
                                note=new_note.clone()
                                on_click=Callback::new(move |_| {
                                    conflict::answer_folder_merge(
                                        state,
                                        FolderMergeAnswer::AsNew,
                                        apply_all.get_untracked(),
                                    )
                                })
                            />
                        }
                    })}
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
