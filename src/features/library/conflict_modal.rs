//! The conflict sheet: the shelf already holds this book, and the question is
//! which of the three file-manager answers the reader means.
//!
//! One sheet for every placement that collides — a drag, a bulk filing, a
//! dropped folder of loose files — and one question at a time, with the queue
//! behind it: a drop of ten files with three collisions files seven at once
//! and asks three times, and the switch offers the same answer for the rest
//! because three identical questions in a row are two too many. The service
//! half (what a placement is, when it collides, what each answer does) is
//! `crate::services::library::conflict`; this file is the ask.
//!
//! The rows say what they DO in one line each — the duplicate's new name is
//! computed and promised on the row, because "keep both" without the name is
//! an answer the reader has to take on faith, and the merge's line is computed
//! and promised the same way: how many highlights the fold would keep and
//! where the survivor would resume, from a dry run of the fold itself.
//! Replace is the one destructive row, and it asks twice only when the shelf's
//! copy takes something with it — a resume point or highlights at an address
//! the arrival does not read from; the second step itemises exactly that, in
//! the remove sheet's own receipt idiom. Two copies of one file share their
//! highlights and their position (both are keyed by the address, and every
//! writer updates all the rows at it), so a replace between them loses a name
//! and a row and nothing else — the first sheet said that much, and resolves
//! on the spot.
//!
//! Cancel — the button, the backdrop and the Escape key — stops the remaining
//! questions rather than skipping one: the placements already answered keep
//! their answers and the rest simply do not land, which is what a file
//! manager's copy dialog has always meant by Cancel.

use leptos::prelude::*;

use app_chrome::floating::dismiss::use_modal_escape;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use library_core::book::stem_of;
use library_core::shelf::ALL_SHELF;
use library_core::text::plural;

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::services::library::conflict::{
    self, Choice, ConflictAsk, ConflictItem, Incoming, Step,
};
use crate::state::AppState;

/// The sheet, mounted once by the library page.
///
/// The open flag lives on the library state rather than in a provided handle
/// (the remove sheet's shape) because the raisers are services: an import asks
/// from inside a spawned future no component owns, and a signal on the state
/// is the one door every raiser and this view already share.
#[component]
pub(crate) fn ConflictModal(state: AppState) -> impl IntoView {
    let open = state.library.conflict_open;
    use_overlay_lane(open, OverlayPolicy::MODAL);
    // A popover opened inside the sheet owns the press; the shared rule peels
    // one layer at a time.
    use_modal_escape(open);

    // A close that came from the lane registry or the Escape key wrote only
    // the boolean; the queue goes with it, so the sheet can never reopen onto
    // a question somebody already dismissed.
    Effect::new(move |_| {
        if !open.get() && state.library.conflict.with_untracked(|ask| ask.is_some()) {
            state.library.conflict.set(None);
        }
    });

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| {
                    // The backdrop is Cancel, except on the second ask, where
                    // a stray click defuses instead of throwing the queue away.
                    let confirming = state.library.conflict.with_untracked(|ask| {
                        ask.as_ref().is_some_and(|a| a.step == Step::ConfirmReplace)
                    });
                    if confirming {
                        conflict::back_to_choices(state);
                    } else {
                        conflict::cancel(state);
                    }
                }
            >
                {move || {
                    let ask = state.library.conflict.get()?;
                    let item = ask.current()?.clone();
                    let info = Info::of(state, &ask, &item);
                    Some(view! { <Sheet state=state info=info /> })
                }}
            </div>
        </Show>
    }
}

/// Everything the sheet prints, read once per answer — the remove receipt's
/// rule: a `view!` body is a builder, not a place to compute, and a heading
/// and a button that need the same string must not each derive their own.
struct Info {
    step: Step,
    /// The arrival's name — the heading.
    incoming_name: String,
    /// The shelf copy's name — what a replace warns about.
    existing_name: String,
    shelf_name: String,
    /// Whether the placement is aimed at the library's root — the unfiled
    /// list — rather than a shelf. The sheet's sentences read "in your
    /// library" there, because "on this shelf" would name a shelf that does
    /// not exist.
    root: bool,
    /// The name a Duplicate would mint, promised on its own row.
    dup_name: String,
    /// What a Merge would keep, promised on its own row — the dry run of the
    /// fold, counted live (see `crate::services::library::conflict::merge_note`).
    merge_note: String,
    cover: Option<String>,
    rest: usize,
    apply_all: bool,
    /// The replace step's receipt: what the shelf's copy loses, and whether
    /// anything beyond the row itself is at stake (two copies of one address
    /// share their position and their marks).
    positions_differ: bool,
    started: bool,
    page_line: String,
    marks: usize,
}

impl Info {
    fn of(state: AppState, ask: &ConflictAsk, item: &ConflictItem) -> Self {
        let books = state.library.books.get();
        let existing = books.iter().find(|b| b.id == item.existing_id);
        let incoming_name = match &item.placement.incoming {
            Incoming::Move { book_id } => books
                .iter()
                .find(|b| &b.id == book_id)
                .map(|b| b.title())
                .unwrap_or_else(|| "This book".to_string()),
            Incoming::Import { file } => stem_of(&file.path),
        };
        // The shelf copy is the same content, so when its row cannot be read
        // (it went between the raise and the render) the arrival's own name is
        // the honest fallback rather than a hole in the sentence.
        let existing_name = existing
            .map(|b| b.title())
            .unwrap_or_else(|| incoming_name.clone());
        let root = item.placement.shelf_id == ALL_SHELF;
        let shelf_name = state
            .library
            .shelves
            .with(|shelves| {
                shelves
                    .iter()
                    .find(|s| s.id == item.placement.shelf_id)
                    .map(|s| s.name.clone())
            })
            .unwrap_or_else(|| "this shelf".to_string());
        let dup_name = conflict::duplicate_name(state, item);
        let merge_note = conflict::merge_note(state, item);
        let cover = existing.and_then(|b| {
            state
                .library
                .covers
                .with(|covers| covers.get(b.path()).map(|c| c.data_url.clone()))
        });
        let incoming_path = match &item.placement.incoming {
            Incoming::Move { book_id } => books
                .iter()
                .find(|b| &b.id == book_id)
                .map(|b| b.path().to_string()),
            Incoming::Import { file } => Some(file.path.clone()),
        };
        // Two rows of one ADDRESS share their reading position and their
        // highlights — every writer updates all the rows at a path, and the
        // marks are keyed by it — so a replace between them loses the row and
        // its name and nothing else. Two rows of one content at DIFFERENT
        // addresses are the case the receipt exists for.
        let positions_differ = match (&incoming_path, existing) {
            (Some(incoming), Some(book)) => incoming != book.path(),
            _ => false,
        };
        let started = existing.is_some_and(|b| b.page > 1 || b.fraction.is_some());
        let page_line = match existing {
            Some(book) if book.num_pages > 0 => {
                format!("Page {} of {}", book.page, book.num_pages)
            }
            Some(book) => format!("Page {}", book.page),
            None => String::new(),
        };
        let marks = existing
            .map(|b| {
                crate::storage::load_gloss()
                    .get(b.path())
                    .map(Vec::len)
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        Self {
            step: ask.step,
            incoming_name,
            existing_name,
            shelf_name,
            root,
            dup_name,
            merge_note,
            cover,
            rest: ask.rest(),
            apply_all: ask.apply_all,
            positions_differ,
            started,
            page_line,
            marks,
        }
    }
}

/// The sheet's body, split out so it takes the facts by value: the outer view
/// answers "is there still a question?" on every run, and this one is built
/// once per answer with an answer it can keep.
#[component]
fn Sheet(state: AppState, info: Info) -> impl IntoView {
    let confirm = info.step == Step::ConfirmReplace;
    // The second ask names what is about to GO — the first sheet named what
    // is arriving, and the warning is about the copy, not the arrival.
    let heading = if confirm {
        format!("Replace “{}”?", info.existing_name)
    } else {
        info.incoming_name.clone()
    };
    let tooltip = heading.clone();
    let cover_alt = heading.clone();
    // Where the copy already is: a shelf the sentence can name, or the
    // library's own unfiled list, which has no name but "your library".
    let where_line = if info.root {
        "in your library".to_string()
    } else {
        format!("on “{}”", info.shelf_name)
    };
    let subtitle = if info.rest > 0 {
        format!("Already {where_line} · {} more waiting", info.rest)
    } else {
        format!("Already {where_line}")
    };
    let question = format!("“{}” {where_line} is the same book.", info.existing_name);
    let dup_note = format!("Keep both — this one becomes “{}”", info.dup_name);
    let replace_note = if info.root {
        "The one in your library gives its place to this one".to_string()
    } else {
        "The one on the shelf gives its place to this one".to_string()
    };
    let merge_note = info.merge_note.clone();
    let offers_all = info.rest > 0 && !confirm;
    let switch_label = match info.rest {
        1 => "Do this for the other book too".to_string(),
        n => format!("Do this for the other {n} books too"),
    };
    let apply_all = info.apply_all;

    // The second ask's own words: what the shelf's copy loses, and — under
    // the switch — that the rest of the queue loses it the same way.
    let replace_line = if info.root {
        format!(
            "“{}” will be replaced by the book arriving. Its place in your \
             library goes to the arrival.",
            info.existing_name
        )
    } else {
        format!(
            "“{}” will be replaced by the book arriving. Its place on this shelf, \
             and on the others it is filed on, goes to the arrival.",
            info.existing_name
        )
    };
    let replace_losses = info.positions_differ && (info.started || info.marks > 0);
    let losses_line = "Only the shelf's copy held these; the arrival's own take their place:"
        .to_string();
    let marks_line = plural(info.marks, "mark", "marks");
    let page_line = info.page_line.clone();
    let started = info.started && info.positions_differ;
    let marks = if info.positions_differ { info.marks } else { 0 };
    let all_line = match (apply_all, info.rest) {
        (true, 1) => "The other waiting book is replaced the same way.".to_string(),
        (true, n) if n > 1 => format!("The other {n} waiting books are replaced the same way."),
        _ => String::new(),
    };
    let has_all_line = !all_line.is_empty();

    let cover = info.cover.clone();
    let has_cover = cover.is_some();

    view! {
        <div
            class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
            style="width:min(92vw, 420px)"
            on:click=move |ev| ev.stop_propagation()
            role="dialog"
            aria-label="The library already holds that book"
        >
            <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
                {has_cover.then(|| {
                    let src = cover.clone().unwrap_or_default();
                    view! {
                        <span class="remove-cover">
                            <img class="remove-cover-img" src=src alt=cover_alt.clone() />
                        </span>
                    }
                })}
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
                {if confirm {
                    view! {
                        <>
                            <p class="text-xs text-muted">{replace_line}</p>
                            {replace_losses.then(|| {
                                view! {
                                    <>
                                        <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                                            {started.then(|| {
                                                view! {
                                                    <LossRow
                                                        label="Reading position"
                                                        value=page_line.clone()
                                                    />
                                                }
                                            })}
                                            {(marks > 0).then(|| {
                                                view! {
                                                    <LossRow label="Highlights" value=marks_line.clone() />
                                                }
                                            })}
                                        </div>
                                        <p class="mt-2 text-xs text-muted">{losses_line}</p>
                                    </>
                                }
                            })}
                            {has_all_line.then(|| {
                                view! { <p class="mt-3 text-xs text-muted">{all_line.clone()}</p> }
                            })}
                        </>
                    }
                        .into_any()
                } else {
                    view! {
                        <>
                            <p class="text-xs text-muted">{question}</p>
                            <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                                <ChoiceRow
                                    label="Duplicate"
                                    note=dup_note
                                    on_click=Callback::new(move |_| {
                                        conflict::choose(state, Choice::Duplicate)
                                    })
                                />
                                <ChoiceRow
                                    label="Replace"
                                    note=replace_note
                                    on_click=Callback::new(move |_| {
                                        conflict::choose(state, Choice::Replace)
                                    })
                                />
                                <ChoiceRow
                                    label="Merge"
                                    note=merge_note
                                    on_click=Callback::new(move |_| {
                                        conflict::choose(state, Choice::Merge)
                                    })
                                />
                            </div>
                            {offers_all.then(|| {
                                view! {
                                    <div class="mt-3 flex items-center justify-between gap-3 rounded-xl border border-line px-4 py-3">
                                        <span class="text-sm text-ink">{switch_label}</span>
                                        <Switch
                                            checked=Signal::derive(move || {
                                                state.library.conflict.with(|ask| {
                                                    ask.as_ref().is_some_and(|a| a.apply_all)
                                                })
                                            })
                                            on_change=Callback::new(move |on| {
                                                conflict::set_apply_all(state, on)
                                            })
                                            title="Answer the rest of the queue the same way"
                                                .to_string()
                                        />
                                    </div>
                                }
                            })}
                        </>
                    }
                        .into_any()
                }}
            </div>

            <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                {if confirm {
                    view! {
                        <>
                            <Button
                                on_click=move |_| conflict::back_to_choices(state)
                                variant=ButtonVariant::Ghost
                                title="Back to the three answers"
                            >
                                <span>"Back"</span>
                            </Button>
                            <Button
                                on_click=move |_| conflict::confirm_replace(state)
                                variant=ButtonVariant::Toolbar
                                tone=ButtonTone::Danger
                                title="Replace the copy on the shelf"
                            >
                                <span>"Replace anyway"</span>
                            </Button>
                        </>
                    }
                        .into_any()
                } else {
                    view! {
                        <Button
                            on_click=move |_| conflict::cancel(state)
                            variant=ButtonVariant::Ghost
                            title="Leave the shelf as it is"
                        >
                            <span>"Cancel"</span>
                        </Button>
                    }
                        .into_any()
                }}
            </footer>
        </div>
    }
}

/// One of the three answers: the name of it, and the one line that says what
/// it does. No icons — the rows are a sentence each, and a glyph beside a
/// sentence is decoration the reader has to look past.
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

/// One line of the replace receipt: what the shelf's copy held, and is about
/// to stop holding.
#[component]
fn LossRow(label: &'static str, value: String) -> impl IntoView {
    let tooltip = value.clone();
    view! {
        <div class="flex items-center gap-2.5 px-3.5 py-2.5">
            <span class="shrink-0 text-xs text-ink">{label}</span>
            <span class="ml-auto min-w-0 truncate text-xs tabular-nums text-muted" title=tooltip>
                {value}
            </span>
        </div>
    }
}
