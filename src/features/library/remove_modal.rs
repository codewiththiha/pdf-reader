//! The remove sheet: what a removal costs, itemised.
//!
//! A removal here is not a dismissal. It takes the resume point, every shelf
//! placement, the cached cover and the highlights with it, and for a book the app
//! copied it can take the bytes too — so the sheet reads as a receipt of what is
//! about to go rather than as a warning, and a row for something the book does not
//! have is simply not there. A reader who removed a book they never opened sees
//! one line, not four empty ones.
//!
//! There is no undo toast, deliberately. The sheet IS the safety, and the real
//! undo path is the folder's import menu, which keeps a tombstone and can offer
//! the book back; a toast would promise a second mechanism and then have to
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
use crate::services::library::{PurgeOpts, memberships, purge_book};
use crate::state::AppState;

/// The sheet's two handles, provided by the library page: whether it is open and
/// which book it is asking about.
///
/// A context for the same reason the import sheet is one — a remove affordance
/// lives on a grid card and on a list row, and threading two signals from the page
/// down through the grid to reach a card would put the sheet's plumbing in every
/// component between.
#[derive(Clone, Copy)]
pub(crate) struct RemoveSheet {
    pub open: RwSignal<bool>,
    pub book: RwSignal<Option<String>>,
}

impl RemoveSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            book: RwSignal::new(None),
        };
        provide_context(sheet);
        sheet
    }

    /// Ask about a book. The sheet shows the receipt and waits.
    pub fn ask(&self, book_id: &str) {
        self.book.set(Some(book_id.to_string()));
        self.open.set(true);
    }
}

/// Everything the receipt lines are made of, read once per open.
struct Receipt {
    book: Book,
    /// Highlight marks stored against this address.
    marks: usize,
    has_cover: bool,
    /// The names of the shelves this book is filed on.
    placements: Vec<String>,
    /// The book came from a folder that is still being watched, so the removal has
    /// a consequence worth one sentence: it will stay out.
    watched: bool,
}

/// Build the receipt. `None` when the book is already gone, which is what makes a
/// sheet left open across a removal harmless rather than a panic.
fn receipt(state: AppState, book_id: &str) -> Option<Receipt> {
    let book = state
        .library
        .books
        .with_untracked(|books| books.iter().find(|b| b.id == book_id).cloned())?;
    let path = book.path().to_string();
    let marks = crate::storage::load_gloss()
        .get(&path)
        .map(Vec::len)
        .unwrap_or(0);
    let has_cover = state
        .library
        .covers
        .with_untracked(|covers| covers.contains_key(&path));
    let placements = memberships(state, book_id)
        .into_iter()
        .map(|(_, name)| name)
        .collect();
    let fingerprint = book.fp;
    let measured = !book.fp_pending;
    let watched = measured
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| {
                f.opts.watch && (f.placed.contains(&fingerprint) || f.is_ignored(&fingerprint))
            })
        });
    Some(Receipt {
        book,
        marks,
        has_cover,
        placements,
        watched,
    })
}

#[component]
pub(crate) fn RemoveBookModal(state: AppState, sheet: RemoveSheet) -> impl IntoView {
    use_overlay_lane(sheet.open, OverlayPolicy::MODAL);
    // On by default: a copy the app made for a book that is leaving the library is
    // a file nothing will ever read again, and this switch is where a reader says
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

    // A book removed by any other route while the sheet is open closes it. Done in
    // an effect rather than in the view, because a view that writes a signal is a
    // view that can be asked to render and mutate in the same pass.
    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let alive = match sheet.book.get() {
            None => false,
            Some(id) => state
                .library
                .books
                .with(|books| books.iter().any(|b| b.id == id)),
        };
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
                    sheet.book.get().and_then(|id| {
                        let info = receipt(state, &id)?;
                        Some(view! {
                            <Sheet
                                state=state
                                sheet=sheet
                                delete_copy=delete_copy
                                info=info
                                remove_id=id
                            />
                        })
                    })
                }}
            </div>
        </Show>
    }
}

/// The sheet's body, split out so it can take the book by value: the outer view
/// answers "is there still a book to talk about?" on every run, and this one is
/// built once per open with an answer it can keep.
#[component]
fn Sheet(
    state: AppState,
    sheet: RemoveSheet,
    delete_copy: RwSignal<bool>,
    info: Receipt,
    remove_id: String,
) -> impl IntoView {
    // Everything the view prints, worked out once. A `view!` body is a builder, not
    // a place to compute: an attribute and a child that need the same string each
    // need their own copy, and finding that out from a compiler is a slow way to
    // learn it.
    let title = info.book.title();
    let tooltip = info.book.title();
    let alt = info.book.title();
    let cover_path = info.book.path().to_string();
    let format_line = match (!info.book.fp_pending).then(|| human_size(info.book.fp.size)) {
        // A book the library has never measured has no honest size to show, and a
        // placeholder's "size" is the length of its path — a number on a receipt
        // that would mean nothing.
        Some(size) => format!("{} · {}", info.book.format.label(), size),
        None => info.book.format.label().to_string(),
    };
    let page_line = if info.book.num_pages > 0 {
        format!("Page {} of {}", info.book.page, info.book.num_pages)
    } else {
        format!("Page {}", info.book.page)
    };
    let started = info.book.page > 1 || info.book.fraction.is_some();
    let marks_line = match info.marks {
        1 => "1 mark".to_string(),
        n => format!("{n} marks"),
    };
    let placements_line = info.placements.join(", ");
    let stored = info.book.origin.is_stored();
    let copy_size = human_size(info.book.fp.size);
    let copy_label = format!("Delete the app's own copy ({copy_size})");
    let marks = info.marks;
    let has_cover = info.has_cover;
    let has_placements = !info.placements.is_empty();
    let watched = info.watched;

    view! {
        <div
            class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
            style="width:min(92vw, 420px)"
            on:click=move |ev| ev.stop_propagation()
            role="dialog"
            aria-label="Remove this book"
        >
            <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
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
                <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-semibold text-ink" title=tooltip>
                        {title}
                    </span>
                    <span class="mt-0.5 block text-xs text-muted">{format_line}</span>
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
                    {has_cover.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Thumbs
                                label="Cached cover"
                                value="1 image".to_string()
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

                {stored.then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Delete the copied file">
                                <Switch
                                    checked=Signal::derive(move || delete_copy.get())
                                    on_change=Callback::new(move |on| delete_copy.set(on))
                                    title=copy_label.clone()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">
                                {move || {
                                    if delete_copy.get() {
                                        format!(
                                            "The copy the app made ({copy_size}) goes with the book. \
                                             The file it was copied from is never touched."
                                        )
                                    } else {
                                        "The copy stays in the app's store, with nothing left to read it."
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
                            "This folder is watched. Removing keeps the book out of future
                             auto-imports; the folder's import menu can offer it back."
                        </p>
                    }
                })}
            </div>

            <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                <Button
                    on_click=move |_| sheet.open.set(false)
                    variant=ButtonVariant::Ghost
                    title="Keep this book"
                >
                    <span>"Cancel"</span>
                </Button>
                <Button
                    on_click=move |_| {
                        sheet.open.set(false);
                        purge_book(
                            state,
                            &remove_id,
                            PurgeOpts {
                                delete_store_copy: delete_copy.get_untracked(),
                            },
                        );
                    }
                    variant=ButtonVariant::Toolbar
                    tone=ButtonTone::Danger
                    title="Remove this book and everything the library holds about it"
                >
                    <Icon name=IconName::Close size=16 />
                    <span>"Remove everything"</span>
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
