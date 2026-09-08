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
//! There is no undo toast, deliberately. The sheet IS the safety, and the real undo
//! path is the folder's import menu, which keeps a tombstone per removal and can
//! offer the book back; a toast would promise a second mechanism and then have to
//! expire.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::book::Book;
use library_core::text::human_size;

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::lanes::{OverlayPolicy, use_overlay_lane};
use crate::components::settings::common::Row;
use crate::services::library::{PurgeOpts, memberships, purge_books};
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
}

impl RemoveSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            books: RwSignal::new(Vec::new()),
        };
        provide_context(sheet);
        sheet
    }

    /// Ask about one book.
    pub fn ask(&self, book_id: &str) {
        self.books.set(vec![book_id.to_string()]);
        self.open.set(true);
    }

    /// Ask about a selection. An empty selection is not a question, and opening
    /// onto one would show a receipt for nothing.
    pub fn ask_many(&self, book_ids: Vec<String>) {
        if book_ids.is_empty() {
            return;
        }
        self.books.set(book_ids);
        self.open.set(true);
    }
}

/// Everything the receipt lines are made of, read once per open.
struct Receipt {
    books: Vec<Book>,
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
}

impl Receipt {
    fn many(&self) -> bool {
        self.books.len() != 1
    }

    /// The heading: one book's title, or a count.
    fn heading(&self) -> String {
        match self.books.first() {
            Some(book) if !self.many() => book.title(),
            _ => {
                let n = self.books.len();
                if n == 1 {
                    "1 book".to_string()
                } else {
                    format!("{n} books")
                }
            }
        }
    }

    /// The line under the heading: the format, and a size the library has actually
    /// measured. A book never measured has no honest size, and a placeholder's
    /// "size" is the length of its path — a number on a receipt that would mean
    /// nothing.
    fn subtitle(&self) -> String {
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

/// Build the receipt. `None` when none of the books are there any more, which is
/// what makes a sheet left open across a removal harmless rather than a panic.
fn receipt(state: AppState, ids: &[String]) -> Option<Receipt> {
    let gloss = crate::storage::load_gloss();
    let books = state.library.books.with_untracked(|books| {
        books
            .iter()
            .filter(|b| ids.contains(&b.id))
            .cloned()
            .collect::<Vec<Book>>()
    });
    if books.is_empty() {
        return None;
    }
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
        books,
        marks,
        covers,
        placements: placement_names,
        watched,
        stored_count,
        stored_bytes,
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

    // Books removed by any other route while the sheet is open close it. Done in an
    // effect rather than in the view, because a view that writes a signal is a view
    // that can be asked to render and mutate in the same pass.
    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let ids = sheet.books.get();
        let alive = !ids.is_empty()
            && state
                .library
                .books
                .with(|books| books.iter().any(|b| ids.contains(&b.id)));
        if !alive {
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
                    let info = receipt(state, &ids)?;
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
                            ids=ids
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
/// answers "is there still anything to talk about?" on every run, and this one is
/// built once per open with an answer it can keep.
#[component]
fn Sheet(
    state: AppState,
    sheet: RemoveSheet,
    delete_copy: RwSignal<bool>,
    info: Receipt,
    ids: Vec<String>,
    cover_path: String,
    alt: String,
) -> impl IntoView {
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
    let remove_label = if many {
        format!("Remove {} books", ids.len())
    } else {
        "Remove everything".to_string()
    };
    let show_cover = !many;

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
                        purge_books(
                            state,
                            &ids,
                            PurgeOpts {
                                delete_store_copy: delete_copy.get_untracked(),
                            },
                        );
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
