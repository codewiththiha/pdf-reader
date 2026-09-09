//! The remove sheet: what a removal costs, itemised.
//!
//! A removal here is not a dismissal. It takes the resume point, every shelf
//! placement, the cached cover and the highlights with it, and for a book the app
//! copied it can take the bytes too — so the sheet reads as a receipt of what is
//! about to go rather than as a warning, and a row for something the books do not
//! have is simply not there. A reader who removed a book they never opened sees one
//! line, not four empty ones.
//!
//! One sheet for one book and for a selection, because the alternative is a bulk
//! delete that skips the itemisation — the one place in the app where "remove"
//! would not tell you what it takes. The rows aggregate; the questions do not
//! change.
//!
//! Shelves come through the same sheet, because a selection holds both kinds and
//! one removal gesture owes the reader one receipt. A shelf's row is shorter than
//! a book's — it is a list of ids and never held a byte — but it is not empty of
//! consequences: the books stay in the library, the shelves inside it move up a
//! level, and a shelf cut from a watched folder says that the folder keeps
//! watching and the shelf returns if the folder places a book in it again.
//!
//! A shelf can also take everything inside it with it, and that is a switch on the
//! sheet rather than a second sheet, because it is a question about the SAME
//! removal: what a shelf holds is part of what removing it costs. Off, the books
//! inside stay in the library and the shelves inside move up a level — the shelf was
//! a list of ids and never held a byte. On, the books inside are purged by the same
//! receipt as a selected book (store copy, highlights, cover, tombstone) and the
//! shelves inside are taken apart instead of lifted, deepest first so nothing is
//! moved up a level on the way to being deleted. The switch is offered only when
//! there is something inside to decide about, and the purge switch only when the
//! books the removal will actually take include a copy the app made — a control
//! that appears with nothing for it to decide is a control the reader has to read
//! and then ignore.
//!
//! There is no undo toast, deliberately. The sheet IS the safety, and the real undo
//! path is the folder's import menu, which keeps a tombstone per removal and can
//! offer the book back; a toast would promise a second mechanism and then have to
//! expire.


mod receipt;

use leptos::prelude::*;

use app_chrome::floating::dismiss::use_modal_escape;
use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::text::{human_size, plural};

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::components::settings::common::Row;
use crate::services::library::{PurgeOpts, delete_shelf, purge_books};
use crate::state::AppState;

use receipt::{Receipt, deepest_first, receipt as build_receipt};

/// The sheet's two handles, provided by the library page: whether it is open and
/// which books it is asking about.
///
/// A context for the same reason the import sheet is one — a remove affordance
/// lives on a grid card, on a list row and on the selection bar, and threading two
/// signals from the page down through the grid to reach a card would put the
/// sheet's plumbing in every component between.
#[derive(Clone, Copy)]
pub(crate) struct RemoveSheet {
    pub open: RwSignal<bool>,
    /// The books under question. One id for a card's ✕, several for a selection;
    /// empty means the sheet has nothing to ask about and closes itself.
    pub books: RwSignal<Vec<String>>,
    /// The shelves under question, from a selection. Separate from [`Self::books`]
    /// because the two are different operations with one confirmation: a purge
    /// itemises what a book takes with it, a shelf is taken apart and keeps every
    /// book in the library.
    pub shelves: RwSignal<Vec<String>>,
    /// Whether a shelf removal takes everything inside it with it.
    ///
    /// Reset by every ask rather than remembered, because a cascade is a decision
    /// about ONE removal: a reader who took a deep shelf and all of its contents
    /// apart did not thereby ask for the next removal to do the same, and a switch
    /// that persisted would be a preference the sheet never offered as one.
    pub cascade: RwSignal<bool>,
}

impl RemoveSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            books: RwSignal::new(Vec::new()),
            shelves: RwSignal::new(Vec::new()),
            cascade: RwSignal::new(false),
        };
        provide_context(sheet);
        sheet
    }

    /// Ask about one book. A card's ✕ is never a question about a shelf, so the
    /// shelf half is cleared rather than left over from the last selection.
    pub fn ask(&self, book_id: &str) {
        self.books.set(vec![book_id.to_string()]);
        self.shelves.set(Vec::new());
        self.cascade.set(false);
        self.open.set(true);
    }

    /// Ask about a selection, which may hold both kinds. An empty selection is
    /// not a question, and opening onto one would show a receipt for nothing.
    pub fn ask_many(&self, book_ids: Vec<String>, shelf_ids: Vec<String>) {
        if book_ids.is_empty() && shelf_ids.is_empty() {
            return;
        }
        self.books.set(book_ids);
        self.shelves.set(shelf_ids);
        self.cascade.set(false);
        self.open.set(true);
    }
}


#[component]
pub(crate) fn RemoveBookModal(state: AppState, sheet: RemoveSheet) -> impl IntoView {
    use_overlay_lane(sheet.open, OverlayPolicy::MODAL);
    // On by default: copies the app made for books that are leaving the library are
    // files nothing will ever read again, and this switch is where a reader says
    // otherwise.
    let delete_copy = RwSignal::new(true);

    // A popover opened inside the sheet owns the press; the shared rule peels
    // one layer at a time.
    use_modal_escape(sheet.open);

    // Books and shelves removed by any other route while the sheet is open close
    // it, once neither half has anything left to talk about. Done in an effect
    // rather than in the view, because a view that writes a signal is a view that
    // can be asked to render and mutate in the same pass.
    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let ids = sheet.books.get();
        let shelf_ids = sheet.shelves.get();
        let books_alive = !ids.is_empty()
            && state
                .library
                .books
                .with(|books| books.iter().any(|b| ids.contains(&b.id)));
        let shelves_alive = !shelf_ids.is_empty()
            && state
                .library
                .shelves
                .with(|shelves| shelves.iter().any(|s| shelf_ids.contains(&s.id)));
        if !books_alive && !shelves_alive {
            sheet.open.set(false);
        }
    });

    view! {
        <Show when=move || sheet.open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| sheet.open.set(false)
            >
                {move || {
                    let ids = sheet.books.get();
                    let shelf_ids = sheet.shelves.get();
                    // Read here rather than inside the sheet, so flipping the
                    // switch rebuilds the receipt and the sheet together: every
                    // row, the store-copy switch and the button's own wording are
                    // all answers about ONE set of books, and a sheet that
                    // recomputed some of them and not others would be a receipt
                    // disagreeing with itself.
                    let cascade = sheet.cascade.get();
                    let info = build_receipt(state, &ids, &shelf_ids, cascade)?;
                    let cover_path = info
                        .books
                        .first()
                        .map(|b| b.path().to_string())
                        .unwrap_or_default();
                    let alt = info.heading();
                    Some(view! {
                        <Sheet
                            state=state
                            sheet=sheet
                            delete_copy=delete_copy
                            info=info
                            cover_path=cover_path
                            alt=alt
                        />
                    })
                }}
            </div>
        </Show>
    }
}

/// The sheet's body, split out so it can take the receipt by value: the outer view
/// answers "is there still anything to talk about?" — and "what would this cost with
/// the cascade on?" — on every run, and this one is built once per answer with an
/// answer it can keep.
///
/// The sets the confirm button acts on come off the receipt and not off separate
/// props, because the receipt is the thing the reader just read: a button handed the
/// ids it was asked about while the rows described the ids plus a shelf's contents
/// would confirm one removal and perform another.
#[component]
fn Sheet(
    state: AppState,
    sheet: RemoveSheet,
    delete_copy: RwSignal<bool>,
    info: Receipt,
    cover_path: String,
    alt: String,
) -> impl IntoView {
    let cascade = info.cascade;
    let purge_ids = info.book_ids.clone();
    let delete_ids = info.shelf_ids.clone();
    let inside_books = info.inside_books;
    let inside_shelves = info.inside_shelves;
    let offers_cascade = inside_books > 0 || inside_shelves > 0;
    // Everything the view prints, worked out once. A `view!` body is a builder, not
    // a place to compute: an attribute and a child that need the same string each
    // need their own copy, and learning that from a compiler is a slow way to learn
    // it.
    let heading = info.heading();
    let tooltip = heading.clone();
    let subtitle = info.subtitle();
    let many = info.many();
    let marks = info.marks;
    let covers = info.covers;
    let placements = info.placements.clone();
    let placements_line = placements.join(", ");
    let has_placements = !placements.is_empty();
    let watched = info.watched;
    let stored_count = info.stored_count;
    let stored_bytes = info.stored_bytes;
    let page_line = match info.books.first() {
        Some(book) if !many && book.num_pages > 0 => {
            format!("Page {} of {}", book.page, book.num_pages)
        }
        Some(book) if !many => format!("Page {}", book.page),
        _ => String::new(),
    };
    let started = !many
        && info
            .books
            .first()
            .is_some_and(|b| b.page > 1 || b.fraction.is_some());
    let marks_line = plural(marks, "mark", "marks");
    // Hoisted out of the view: an `if` in attribute position is an expression the
    // macro has to guess the end of, and a label is a string either way.
    let covers_label = if many { "Cached covers" } else { "Cached cover" };
    let covers_line = plural(covers, "image", "images");
    let books_line = plural(info.books.len(), "book", "books");
    let copy_label = match stored_count {
        1 => format!("Delete the app's own copy ({})", human_size(stored_bytes)),
        n => format!("Delete the app's {n} copies ({})", human_size(stored_bytes)),
    };
    let copy_note_on = match stored_count {
        1 => format!(
            "The copy the app made ({}) goes with the book. The file it was copied from is never touched.",
            human_size(stored_bytes)
        ),
        n => format!(
            "The {n} copies the app made ({}) go with the books. The files they were copied from are never touched.",
            human_size(stored_bytes)
        ),
    };
    // The button names both halves when the selection held both kinds, because
    // a confirmation that only mentioned the books would be a confirmation the
    // reader did not read before the shelves went.
    // A cascade is already counted in both numbers — the books inside are in
    // `books` and the shelves inside are in `shelves` — except in the one case
    // where a shelf holds no books at all and only empty folders, and there the
    // label has to say so or it promises less than the click does.
    let remove_label = match (info.books.len(), info.shelves.len()) {
        (1, 0) => "Remove everything".to_string(),
        (0, 1) if cascade && inside_shelves > 0 => {
            "Remove the shelf and the ones inside".to_string()
        }
        (0, 1) => "Remove the shelf".to_string(),
        (0, n) if cascade && inside_shelves > 0 => {
            format!("Remove {n} shelves and the ones inside")
        }
        (0, n) => format!("Remove {n} shelves"),
        (b, 0) => format!("Remove {b} books"),
        (b, s) => format!("Remove {b} books and {s} shelves"),
    };
    let show_cover = !many && !info.books.is_empty();
    // One row per shelf. Without the cascade the row says what SURVIVES it — a
    // shelf is a list of ids, and the books stay while the shelves inside move up.
    // With the cascade on it says what GOES, because that is now the honest answer
    // and the same words would mean the opposite thing: `lifted` is zero there by
    // construction, so nothing is described as moving up on its way to being
    // deleted.
    let shelf_rows: Vec<(String, String)> = info
        .shelves
        .iter()
        .map(|s| {
            let mut detail = match (s.books, cascade) {
                (0, _) => "empty".to_string(),
                (n, true) => format!("{} go with it", plural(n, "book", "books")),
                (n, false) => plural(n, "book", "books"),
            };
            if s.lifted > 0 {
                detail.push_str(&format!(
                    " · {} move up",
                    plural(s.lifted, "shelf", "shelves")
                ));
            }
            (s.name.clone(), detail)
        })
        .collect();
    let has_shelves = !shelf_rows.is_empty();
    let shelf_watched = info.shelves.iter().any(|s| s.watched);
    // What the switch is offering, in the numbers the receipt has just counted.
    // Spelled out rather than left to the label, because "remove everything
    // inside" is a sentence whose size the reader is about to find out the hard
    // way, and this sheet exists so that they do not.
    let cascade_note = if cascade {
        match (inside_books, inside_shelves) {
            (0, shelves) => format!(
                "{} inside are taken apart with it, instead of moving up a level.",
                plural(shelves, "shelf", "shelves")
            ),
            (books, 0) => format!(
                "{} inside go with the shelf, and are itemised above.",
                plural(books, "book", "books")
            ),
            (books, shelves) => format!(
                "{} inside go with the shelf and {} inside are taken apart too.",
                plural(books, "book", "books"),
                plural(shelves, "shelf", "shelves")
            ),
        }
    } else {
        "Off: the books inside stay in the library and the shelves inside move up a level."
            .to_string()
    };

    view! {
        <div
            class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
            style="width:min(92vw, 420px)"
            on:click=move |ev| ev.stop_propagation()
            role="dialog"
            aria-label="Remove from the library"
        >
            <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
                {show_cover.then(|| {
                    view! {
                        <span class="remove-cover">
                            {move || {
                                state
                                    .library
                                    .covers
                                    .with(|covers| covers.get(&cover_path).cloned())
                                    .map(|cover| {
                                        view! {
                                            <img
                                                class="remove-cover-img"
                                                src=cover.data_url.clone()
                                                alt=alt.clone()
                                            />
                                        }
                                    })
                            }}
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
                    on_click=move || sheet.open.set(false)
                />
            </header>

            <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
                <div class="divide-y divide-line rounded-xl border border-line">
                    {many.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Library
                                label="Books"
                                value=books_line.clone()
                            />
                        }
                    })}
                    {started.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Library
                                label="Reading position"
                                value=page_line.clone()
                            />
                        }
                    })}
                    {(marks > 0).then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Type
                                label="Highlights"
                                value=marks_line.clone()
                            />
                        }
                    })}
                    {(covers > 0).then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Thumbs
                                label=covers_label
                                value=covers_line.clone()
                            />
                        }
                    })}
                    {has_placements.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Outline
                                label="Shelf placements"
                                value=placements_line.clone()
                            />
                        }
                    })}
                </div>

                {has_shelves.then(|| {
                    view! {
                        <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                            {shelf_rows
                                .iter()
                                .map(|(name, detail)| {
                                    view! {
                                        <ReceiptRow
                                            icon=IconName::Outline
                                            label="Shelf taken apart"
                                            value=format!("{name} — {detail}")
                                        />
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                })}

                // Offered only when there is something inside to decide about.
                // An empty leaf shelf has no cascade and gets no switch; a shelf
                // holding nothing but empty folders does, because there the switch
                // is the whole difference between "these move up and clutter the
                // level above" and "these go too".
                {offers_cascade.then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Remove everything inside">
                                <Switch
                                    checked=Signal::derive(move || sheet.cascade.get())
                                    on_change=Callback::new(move |on| sheet.cascade.set(on))
                                    title="Take the books and the shelves inside with this shelf"
                                        .to_string()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">{cascade_note.clone()}</p>
                        </div>
                    }
                })}

                {shelf_watched.then(|| {
                    view! {
                        <p class="mt-3 text-xs text-muted">
                            "A shelf here was cut from a watched folder. Removing takes it off the
                             list; the folder keeps watching, and the shelf returns if the folder
                             places a book in it again."
                        </p>
                    }
                })}

                {(stored_count > 0).then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Delete the copied files">
                                <Switch
                                    checked=Signal::derive(move || delete_copy.get())
                                    on_change=Callback::new(move |on| delete_copy.set(on))
                                    title=copy_label.clone()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">
                                {move || {
                                    if delete_copy.get() {
                                        copy_note_on.clone()
                                    } else {
                                        "The copies stay in the app's store, with nothing left to read them."
                                            .to_string()
                                    }
                                }}
                            </p>
                        </div>
                    }
                })}

                {watched.then(|| {
                    view! {
                        <p class="mt-3 text-xs text-muted">
                            "A watched folder placed at least one of these. Removing keeps them out of
                             future auto-imports; the folder's import menu can offer them back."
                        </p>
                    }
                })}
            </div>

            <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                <Button
                    on_click=move |_| sheet.open.set(false)
                    variant=ButtonVariant::Ghost
                    title="Keep these books"
                >
                    <span>"Cancel"</span>
                </Button>
                <Button
                    on_click=move |_| {
                        sheet.open.set(false);
                        // The receipt's own sets, not the ones the click was
                        // handed: under a cascade these are bigger, and this is the
                        // one place where the difference is a book that survives or
                        // does not.
                        if !purge_ids.is_empty() {
                            purge_books(
                                state,
                                &purge_ids,
                                PurgeOpts {
                                    delete_store_copy: delete_copy.get_untracked(),
                                },
                            );
                        }
                        // Shelves after the books: a purge sweeps every shelf's
                        // member list, and a shelf dissolved first would be swept
                        // by nobody. Deepest first, so a cascade never lifts a
                        // shelf to the level it was on moments before deleting it.
                        let shelves_now = state.library.shelves.get_untracked();
                        for shelf_id in deepest_first(&shelves_now, &delete_ids) {
                            delete_shelf(state, &shelf_id);
                        }
                    }
                    variant=ButtonVariant::Toolbar
                    tone=ButtonTone::Danger
                    title="Remove these books and everything the library holds about them"
                >
                    <Icon name=IconName::Close size=16 />
                    <span>{remove_label}</span>
                </Button>
            </footer>
        </div>
    }
}

/// One line of the receipt: an icon, what is going, and how much of it there is.
#[component]
fn ReceiptRow(icon: IconName, label: &'static str, value: String) -> impl IntoView {
    let tooltip = value.clone();
    view! {
        <div class="flex items-center gap-2.5 px-3.5 py-2.5">
            <Icon name=icon size=14 class="shrink-0 text-muted" />
            <span class="shrink-0 text-xs text-ink">{label}</span>
            <span class="ml-auto min-w-0 truncate text-xs tabular-nums text-muted" title=tooltip>
                {value}
            </span>
        </div>
    }
}

