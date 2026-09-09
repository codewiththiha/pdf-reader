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

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::book::Book;
use library_core::shelf::{Shelf, ancestors, children_of};
use library_core::text::human_size;

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::components::settings::common::Row;
use crate::services::library::{PurgeOpts, delete_shelf, memberships, purge_books};
use crate::state::AppState;

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

/// Everything the receipt lines are made of, read once per open.
struct Receipt {
    books: Vec<Book>,
    /// The ids the confirm button will purge: what was asked, plus everything
    /// inside the asked shelves when the cascade is on. Separate from [`Self::books`]
    /// only because the button needs ids and the rows need the rows' own facts.
    book_ids: Vec<String>,
    /// The ids the confirm button will delete: what was asked, plus every
    /// descendant when the cascade is on.
    shelf_ids: Vec<String>,
    /// Whether this receipt was built with the cascade on, which is what the shelf
    /// rows and the button's own wording have to agree with.
    cascade: bool,
    /// The name of the one shelf asked about, when exactly one was. A cascade pulls
    /// books and further shelves into the receipt, and without this the heading
    /// would answer "3 books" to a reader who clicked a shelf — a heading about
    /// something they did not click, on the one sheet whose whole job is to say what
    /// the click means.
    asked_name: Option<String>,
    /// What sits inside the asked shelves whatever the switch says — the books
    /// anywhere inside them, and the shelves nested inside them at any depth. The
    /// switch's own visibility is decided by these, so it cannot depend on itself.
    inside_books: usize,
    inside_shelves: usize,
    /// Highlight marks stored against these addresses.
    marks: usize,
    covers: usize,
    /// The names of the shelves any of these books is filed on, deduped: ten books
    /// on one shelf is one placement to name, not ten.
    placements: Vec<String>,
    /// At least one of them came from a folder that is still being watched, so the
    /// removal has a consequence worth one sentence: it will stay out.
    watched: bool,
    /// The app's own copies among them, and what they occupy.
    stored_count: usize,
    stored_bytes: u64,
    /// The shelves being taken apart, and what survives each of them.
    shelves: Vec<ShelfLine>,
}

/// One shelf the removal takes apart. A shelf is a list of ids and never held a
/// byte, so its row is about what SURVIVES it rather than about what goes.
struct ShelfLine {
    name: String,
    books: usize,
    /// Shelves filed inside it, which move up to the level it was on. Always zero
    /// under a cascade, where nothing survives inside to be lifted.
    lifted: usize,
    /// Cut from a folder that is still watched, so the shelf returns if the
    /// folder ever places a book in it again. Worth one sentence on the receipt
    /// because it is the one consequence a reader cannot see coming.
    watched: bool,
}

impl Receipt {
    fn many(&self) -> bool {
        self.books.len() + self.shelves.len() != 1
    }

    /// The heading: one thing's own name, or a count.
    fn heading(&self) -> String {
        if let Some(name) = &self.asked_name {
            return name.clone();
        }
        if !self.books.is_empty() {
            match self.books.first() {
                Some(book) if !self.many() => book.title(),
                _ => count(self.books.len(), "book", "books"),
            }
        } else {
            match self.shelves.first() {
                Some(shelf) if !self.many() => shelf.name.clone(),
                _ => count(self.shelves.len(), "shelf", "shelves"),
            }
        }
    }

    /// The line under the heading: the format, and a size the library has actually
    /// measured. A book never measured has no honest size, and a placeholder's
    /// "size" is the length of its path — a number on a receipt that would mean
    /// nothing. A shelves-only receipt has no formats to name, so it says the one
    /// thing a reader worries about: that nothing else goes with them.
    fn subtitle(&self) -> String {
        if self.books.is_empty() {
            let kept: usize = self.shelves.iter().map(|s| s.books).sum();
            return if kept == 0 {
                "Nothing else goes with them".to_string()
            } else {
                format!("{} stay in the library", count(kept, "book", "books"))
            };
        }
        let measured: Vec<u64> = self
            .books
            .iter()
            .filter(|b| !b.fp_pending)
            .map(|b| b.fp.size)
            .collect();
        let formats: Vec<&str> = {
            let mut seen: Vec<&str> = Vec::new();
            for book in &self.books {
                let label = book.format.label();
                if !seen.contains(&label) {
                    seen.push(label);
                }
            }
            seen
        };
        let kinds = formats.join(" · ");
        if measured.is_empty() {
            kinds
        } else {
            let total: u64 = measured.iter().sum();
            format!("{kinds} · {}", human_size(total))
        }
    }
}

/// "3 books", "1 shelf". One helper because the receipt says this in four places
/// and the singular of "shelves" is easy to get wrong once.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Every shelf below any of `roots`, at any depth, in no particular order and
/// without repeats.
///
/// Walked with an explicit stack and a seen-set rather than recursively, for two
/// reasons. The forest is finite because `library_core::shelf::sanitize` cuts cycles
/// out of a loaded blob — but this reads a list that can be caught between two
/// writes, and a recursion over a graph with a loop in it is a stack that never
/// unwinds. A shelf inside itself is also a shelf that would otherwise be counted
/// twice on its own receipt, and two selected shelves can share a descendant, which
/// one removal takes apart once.
///
/// Pure over the shelf list rather than over the state, so the arithmetic a cascade
/// depends on is testable on the host: this is the function that decides which
/// shelves a removal deletes, and "which shelves go" is exactly the question that
/// should not need a browser to answer.
fn subtree(shelves: &[Shelf], roots: &[String]) -> Vec<Shelf> {
    let mut out: Vec<Shelf> = Vec::new();
    let mut stack: Vec<String> = roots.to_vec();
    while let Some(parent) = stack.pop() {
        for child in children_of(shelves, Some(parent.as_str())) {
            if roots.iter().any(|each| each == &child.id)
                || out.iter().any(|each| each.id == child.id)
            {
                continue;
            }
            stack.push(child.id.clone());
            out.push(child.clone());
        }
    }
    out
}

/// A cascade's delete order: deepest first.
///
/// `delete_shelf` lifts a shelf's children to the level it was on before removing
/// it, which is the right thing for one removal and the wrong thing for a cascade:
/// lifting a shelf that is next in line to be deleted moves it somewhere it is
/// about to leave anyway, and moves it past the reader on the way. Deepest first
/// means every lift finds nothing left to lift.
///
/// A stable sort on a reversed key, so two shelves at the same depth keep the
/// order the library stores them in and the receipt's rows match the order they
/// went in.
fn deepest_first(shelves: &[Shelf], ids: &[String]) -> Vec<String> {
    let mut with_depth: Vec<(String, usize)> = ids
        .iter()
        .map(|id| (id.clone(), ancestors(shelves, id).len()))
        .collect();
    with_depth.sort_by_key(|one| std::cmp::Reverse(one.1));
    with_depth.into_iter().map(|(id, _)| id).collect()
}

/// Build the receipt. `None` when none of the books or shelves are there any
/// more, which is what makes a sheet left open across a removal harmless rather
/// than a panic.
///
/// `cascade` decides which SET the receipt is of, and everything below — the books
/// row, the highlight and cover counts, the store copies, the placements, the
/// button's own wording — is measured over that set rather than over what was
/// clicked. A receipt that itemised the selection and then removed the selection
/// plus a shelf's contents would be a receipt for a different removal than the one
/// it confirmed.
fn receipt(
    state: AppState,
    ids: &[String],
    shelf_ids: &[String],
    cascade: bool,
) -> Option<Receipt> {
    let gloss = crate::storage::load_gloss();
    // One untracked read of the shelf list, and the tree arithmetic below is pure
    // over it: three nested reads of the same signal were three chances to see a
    // different library than the one the receipt is describing.
    let all: Vec<Shelf> = state.library.shelves.get_untracked();
    let asked: Vec<Shelf> = all
        .iter()
        .filter(|s| shelf_ids.contains(&s.id))
        .cloned()
        .collect();
    // Everything below the asked shelves, deduped against each other and against
    // the asked ones: two selected shelves can share a descendant, and a shelf
    // selected alongside its own parent is already in `asked`.
    let descendants = subtree(&all, shelf_ids);
    let delete_shelves: Vec<Shelf> = if cascade {
        asked.iter().chain(descendants.iter()).cloned().collect()
    } else {
        asked.clone()
    };
    // What is inside, counted whether or not the cascade is on: these two numbers
    // are what the switch's own visibility is decided by, and a switch that only
    // appeared once it was already on could never be turned on.
    let inside_ids: Vec<String> = {
        let mut acc: Vec<String> = Vec::new();
        for shelf in all.iter().filter(|s| {
            shelf_ids.contains(&s.id) || descendants.iter().any(|each| each.id == s.id)
        }) {
            for book in &shelf.books {
                if !acc.contains(book) {
                    acc.push(book.clone());
                }
            }
        }
        acc
    };
    // The set the removal will actually take: what was asked, plus what is inside
    // when the cascade is on. Deduped, because a book on the asked shelf and inside
    // the asked folder is one book and one tombstone.
    let mut effective: Vec<String> = Vec::new();
    for id in ids {
        if !effective.contains(id) {
            effective.push(id.clone());
        }
    }
    if cascade {
        for id in &inside_ids {
            if !effective.contains(id) {
                effective.push(id.clone());
            }
        }
    }
    let books: Vec<Book> = state.library.books.with_untracked(|all| {
        all.iter()
            .filter(|b| effective.contains(&b.id))
            .cloned()
            .collect()
    });
    if books.is_empty() && asked.is_empty() {
        return None;
    }
    let shelf_lines: Vec<ShelfLine> = delete_shelves
        .iter()
        .map(|s| ShelfLine {
            name: s.name.clone(),
            books: s.books.len(),
            lifted: if cascade {
                0
            } else {
                children_of(&all, Some(s.id.as_str())).len()
            },
            watched: s.kind.folder_id().is_some_and(|folder_id| {
                state.library.folders.with_untracked(|folders| {
                    folders.iter().any(|f| f.id == folder_id && f.opts.watch)
                })
            }),
        })
        .collect();
    let mut marks = 0usize;
    let mut covers = 0usize;
    let mut stored_count = 0usize;
    let mut stored_bytes = 0u64;
    let mut placement_names: Vec<String> = Vec::new();
    for book in &books {
        let path = book.path();
        marks += gloss.get(path).map(Vec::len).unwrap_or(0);
        if state
            .library
            .covers
            .with_untracked(|covers| covers.contains_key(path))
        {
            covers += 1;
        }
        if let library_core::book::Origin::Stored { .. } = &book.origin {
            stored_count += 1;
            if !book.fp_pending {
                stored_bytes += book.fp.size;
            }
        }
        for (_, name) in memberships(state, &book.id) {
            if !placement_names.contains(&name) {
                placement_names.push(name);
            }
        }
    }
    let fingerprints: Vec<_> = books.iter().map(|b| b.fp).collect();
    let measured = books.iter().all(|b| !b.fp_pending);
    let watched = measured
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| {
                f.opts.watch
                    && fingerprints
                        .iter()
                        .any(|fp| f.placed.contains(fp) || f.is_ignored(fp))
            })
        });
    Some(Receipt {
        book_ids: books.iter().map(|b| b.id.clone()).collect(),
        books,
        shelf_ids: delete_shelves.iter().map(|s| s.id.clone()).collect(),
        cascade,
        asked_name: match asked.as_slice() {
            [only] => Some(only.name.clone()),
            _ => None,
        },
        inside_books: inside_ids.len(),
        inside_shelves: descendants.len(),
        marks,
        covers,
        placements: placement_names,
        watched,
        stored_count,
        stored_bytes,
        shelves: shelf_lines,
    })
}

#[component]
pub(crate) fn RemoveBookModal(state: AppState, sheet: RemoveSheet) -> impl IntoView {
    use_overlay_lane(sheet.open, OverlayPolicy::MODAL);
    // On by default: copies the app made for books that are leaving the library are
    // files nothing will ever read again, and this switch is where a reader says
    // otherwise.
    let delete_copy = RwSignal::new(true);

    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let handle = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            if let Ok(key) = ev.dyn_into::<web_sys::KeyboardEvent>()
                && key.key() == "Escape"
            {
                // A popover opened inside the sheet owns this press.
                if app_chrome::floating::dismiss::has_open_dismissable() {
                    return;
                }
                sheet.open.set(false);
            }
        });
        on_cleanup(move || handle.remove());
    });

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
                    let info = receipt(state, &ids, &shelf_ids, cascade)?;
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
    let marks_line = match marks {
        1 => "1 mark".to_string(),
        n => format!("{n} marks"),
    };
    // Hoisted out of the view: an `if` in attribute position is an expression the
    // macro has to guess the end of, and a label is a string either way.
    let covers_label = if many { "Cached covers" } else { "Cached cover" };
    let covers_line = match covers {
        1 => "1 image".to_string(),
        n => format!("{n} images"),
    };
    let books_line = match info.books.len() {
        1 => "1 book".to_string(),
        n => format!("{n} books"),
    };
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
                (n, true) => format!("{} go with it", count(n, "book", "books")),
                (n, false) => count(n, "book", "books"),
            };
            if s.lifted > 0 {
                detail.push_str(&format!(
                    " · {} move up",
                    count(s.lifted, "shelf", "shelves")
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
                count(shelves, "shelf", "shelves")
            ),
            (books, 0) => format!(
                "{} inside go with the shelf, and are itemised above.",
                count(books, "book", "books")
            ),
            (books, shelves) => format!(
                "{} inside go with the shelf and {} inside are taken apart too.",
                count(books, "book", "books"),
                count(shelves, "shelf", "shelves")
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

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::shelf::ShelfKind;

    /// A virtual shelf, which is the kind a reader makes and the only kind a
    /// cascade can move.
    fn shelf(id: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|each| each.to_string()).collect(),
            parent: parent.map(str::to_string),
        }
    }

    fn ids(shelves: &[Shelf]) -> Vec<String> {
        shelves.iter().map(|each| each.id.clone()).collect()
    }

    /// `a` at the root, `b` inside it, `c` inside `b`, and an empty `d` beside `b`.
    fn tree() -> Vec<Shelf> {
        vec![
            shelf("a", None, &[]),
            shelf("b", Some("a"), &["b1", "b2"]),
            shelf("c", Some("b"), &["c1"]),
            shelf("d", Some("a"), &[]),
        ]
    }

    #[test]
    fn the_subtree_is_everything_below_and_never_the_root_itself() {
        let tree = tree();
        let mut under_a = ids(&subtree(&tree, &["a".to_string()]));
        under_a.sort();
        assert_eq!(under_a, ["b", "c", "d"], "the root is asked about, not inside");

        let under_b = ids(&subtree(&tree, &["b".to_string()]));
        assert_eq!(under_b, ["c"]);

        assert!(
            subtree(&tree, &["d".to_string()]).is_empty(),
            "an empty leaf has no subtree, which is why it gets no cascade switch"
        );
    }

    #[test]
    fn two_roots_sharing_a_descendant_count_it_once() {
        // One removal takes a shared shelf apart once, and a receipt that listed
        // it twice would be a receipt the reader could not reconcile with what
        // actually went.
        let tree = tree();
        let under_both = ids(&subtree(&tree, &["a".to_string(), "b".to_string()]));
        assert_eq!(under_both.len(), under_both.iter().collect::<std::collections::HashSet<_>>().len());
        assert!(under_both.iter().any(|id| id == "c"));
        assert!(
            !under_both.iter().any(|id| id == "b"),
            "a root is never reported as its own descendant"
        );
    }

    #[test]
    fn a_shelf_inside_itself_terminates_rather_than_repeating() {
        // `sanitize` cuts cycles out of a loaded blob, but the receipt reads a
        // signal that can be caught between two writes, and a walk that spun here
        // would hang the sheet rather than answer it.
        let looped = vec![
            shelf("x", Some("y"), &[]),
            shelf("y", Some("x"), &[]),
        ];
        let mut found = ids(&subtree(&looped, &["x".to_string()]));
        found.sort();
        assert_eq!(found, ["y"]);
    }

    #[test]
    fn a_cascade_deletes_deepest_first() {
        let tree = tree();
        let order = deepest_first(&tree, &["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(
            order.first().map(String::as_str),
            Some("c"),
            "the deepest goes first, so no lift moves a shelf that is next in line"
        );
        assert_eq!(
            order.last().map(String::as_str),
            Some("a"),
            "and the shelf the reader asked about goes last"
        );
    }

    #[test]
    fn shelves_at_one_depth_keep_the_libraries_own_order() {
        // A stable sort: the receipt's rows and the order the shelves went in are
        // the same order, so a reader can follow what happened.
        let tree = tree();
        let order = deepest_first(&tree, &["d".to_string(), "b".to_string()]);
        assert_eq!(order, ["d", "b"]);
    }
}
