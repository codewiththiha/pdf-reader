//! The collision sheet: the level already holds a book of this name, and the
//! question is which of three things the reader meant.
//!
//! One sheet, one question, three rows — and WHICH three is the arrival's own
//! fact, because an import and a move are different questions. A file arriving
//! has nothing of its own yet, so its answers are about what to put here:
//! *already imported* places nothing and takes the reader to the row that is
//! already there, *add as new* keeps both under the next free name, and *make
//! link* puts a pointer here instead of a copy. A row being moved is two books
//! the reader already has, so its answers are about which of them the level
//! keeps: *merge* folds the moved one into the one that is here, *replace*
//! sends the one that is here out of the library and seats the arrival in its
//! place, and *as new* keeps both under the next free name.
//!
//! Neither set gets a second ask. An import's answers cannot destroy anything,
//! so there is nothing to warn about; a move's Replace can, so its row says
//! what goes before the click — the name of the row and how many highlights
//! leave with it — which is the promise-on-the-row idiom the rest of the sheet
//! already keeps.
//!
//! One more shape wears this sheet's chrome without wearing its question: a
//! loose import of a file that sits inside a folder the library reads in
//! place asks about the FILE'S GROUND rather than the level's name — the
//! library's own stored copy here, or the book the folder holds, lit — because
//! a second link of one read-at-place file is the one thing the folder rule
//! never makes.
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

use library_core::shelf::ALL_SHELF;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict::{self, ConflictAsk, CoveredAnswer, FolderMergeAnswer};
use crate::services::library::folder_label;
use library_core::book::find_row;
use library_core::conflict::{Answer, MoveAnswer, next_name};
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

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question and the ones waiting
    // behind it go with it, so the sheet can never reopen onto a question
    // somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() {
            state.library.conflict.set(None);
            state.library.conflict_waiting.set(Vec::new());
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="The library already holds a book of that name here"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.conflict.get()?;
                // A folder merge's file asks wear the compact sheet: the
                // shelf's question is already answered, and what is left is a
                // run of files with the same three doors each.
                if ask.folder_merge {
                    return Some(
                        view! { <FolderMergeSheet state=state ask=ask /> }.into_any(),
                    );
                }
                // A covered file's ask is about the file's own ground rather
                // than the level's name, and its sheet is the two answers the
                // read-at-place rule leaves.
                if ask.covered {
                    return Some(
                        view! { <CoveredSheet state=state ask=ask /> }.into_any(),
                    );
                }
                let info = Info::of(state, &ask);
                Some(view! { <Sheet state=state info=info /> }.into_any())
            }}
        </ModalShell>
    }
}

/// Everything the sheet prints, read once per answer — the remove receipt's
/// rule: a `view!` body is a builder, not a place to compute, and a heading and
/// two buttons that need the same name must not each derive their own.
struct Info {
    /// The name arriving — the heading.
    incoming: String,
    /// Whether the arrival is a file with no row of its own yet, which is the
    /// fact that decides which three rows the sheet offers.
    import: bool,
    /// The name already on the level, which *already imported* goes to and
    /// *make link* points at.
    existing_name: String,
    /// How many highlights the row already here holds — what a Replace takes
    /// with it, promised on its own row rather than asked about afterwards.
    marks: usize,
    /// The name *add as new* would mint, promised on its own row: "keep both"
    /// without the name is an answer the reader has to take on faith.
    new_name: String,
    /// Where the row that collided is: a shelf the sentence can name, or the
    /// library's own unfiled list, which has no name but "your library".
    where_line: String,
    /// How many questions wait behind this one.
    waiting: usize,
    /// Whether the move is the pointer shape: the row being dragged is a
    /// read-at-place book an in-place folder placed, and the row on the level
    /// is one of the library's own stored copies. Neither side is the
    /// reader's to destroy, so the sheet offers *make link* in place of the
    /// destructive *replace*: reach the copy from here, and keep both the
    /// file on disk and the bytes in the store exactly as they are.
    link_offer: bool,
}

impl Info {
    fn of(state: AppState, ask: &ConflictAsk) -> Self {
        let where_line = if ask.arrival.shelf_id == ALL_SHELF {
            "in your library".to_string()
        } else {
            // Empty means the shelf went while the sheet was up, which is
            // the same answer as the root's: a level with no name to speak.
            // `sanitize` drops a shelf whose name is blank, so nothing on a
            // loaded list answers with one.
            match state.library.shelf_name(&ask.arrival.shelf_id) {
                name if !name.is_empty() => format!("on “{name}”"),
                _ => "on this shelf".to_string(),
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
        // The row's OWN key, not its address: a book of its own keeps its
        // marks under a key of its id, and a count taken from the address would
        // promise a loss the removal cannot make.
        let marks = find_row(&rows, &ask.existing_id)
            .and_then(|row| row.book())
            .map(|book| {
                crate::storage::load_gloss()
                    .get(&book.gloss_key())
                    .map(Vec::len)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        // The pointer shape is a fact about the two ROWS, read off the same
        // snapshot the rest of the sheet counts against.
        let link_offer = ask.arrival.moving.as_ref().is_some_and(|moved_id| {
            find_row(&rows, &ask.existing_id)
                .and_then(|row| row.book())
                .is_some_and(|book| book.origin.is_stored())
                && crate::services::library::arrange::converts_on_move_to(
                    state,
                    moved_id,
                    &ask.arrival.shelf_id,
                )
        });
        Self {
            incoming: ask.arrival.name.clone(),
            import: ask.arrival.is_import(),
            existing_name: ask.existing_name.clone(),
            marks,
            new_name,
            where_line,
            waiting: state.library.conflict_waiting.with_untracked(|w| w.len()),
            link_offer,
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

/// One of the three answers: the name of it, and the one line that says what it
/// does. No icons — the rows are a sentence each, and a glyph beside a sentence
/// is decoration the reader has to look past. Shared with the folder sheet,
/// whose rows are the same shape of promise.
#[component]
pub(crate) fn ChoiceRow(label: &'static str, note: String, on_click: Callback<()>) -> impl IntoView {
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
fn FolderMergeSheet(state: AppState, ask: ConflictAsk) -> impl IntoView {
    // The switch starts off on every question: "apply to all" is the reader's
    // answer per sheet, not a preference the first click leaves behind.
    let apply_all = RwSignal::new(false);
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.folder_merge).count());
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
    let twin = ask.in_place
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
fn CoveredSheet(state: AppState, ask: ConflictAsk) -> impl IntoView {
    let apply_all = RwSignal::new(false);
    let waiting = state
        .library
        .conflict_waiting
        .with_untracked(|w| w.iter().filter(|each| each.covered).count());
    let incoming = ask.arrival.name.clone();
    let heading = incoming.clone();
    let folder_name = ask
        .folder_id
        .as_deref()
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
