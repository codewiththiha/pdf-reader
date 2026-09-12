//! The two ways books arrive, and — inside a watched folder's shelf — the way
//! one comes back.
//!
//! Both the shelf's `+` card and the empty state's button open this rather than
//! doing anything themselves: an import has exactly two sources and the reader
//! should see both from either, or the empty state becomes the only way to find
//! one of them.
//!
//! The third section only exists when the page is drilled into a folder's shelf,
//! and it is built without walking anything. A restore menu that had to rescan the
//! tree to open would take as long as the import it is offering an alternative to,
//! so it reads the folder's own ledger — its tombstones and what its last scan saw
//! — and measures only the single file a row is about to bring back.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use app_chrome::icon::{Icon, IconName};
use library_core::ledger::{Recovered, index_by_fp, recoverables};
use library_core::shelf::{ALL_SHELF, find};
use library_core::text::{human_age, human_size};

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::features::library::import_modal::ImportSheet;
use crate::services::library::{
    folder_label, import_files, pick_documents, pick_documents_in, restore_deleted_book,
};
use crate::state::AppState;

/// One row the folder section can offer back.
#[derive(Debug, Clone, PartialEq)]
struct RestoreRow {
    item: Recovered,
    /// A removed book whose file is no longer where it was removed from. Still
    /// listed, but disabled: a menu that quietly drops rows is a menu the reader
    /// cannot tell from one that never had them.
    gone: bool,
}

impl RestoreRow {
    fn label(&self) -> String {
        match &self.item {
            Recovered::Deleted(entry) => entry.label(),
            Recovered::Moved { title, path, .. } => {
                library_core::text::display_or_stem(title.as_deref(), path)
            }
        }
    }

    fn sublabel(&self, now_ms: u64) -> String {
        if self.gone {
            return "not there any more".to_string();
        }
        match &self.item {
            Recovered::Deleted(entry) => {
                let age = human_age(entry.removed_ms, now_ms);
                // A placeholder fingerprint's "size" is the length of its path,
                // which is a number that would mean nothing on a menu row.
                match entry.fp.mtime_ms {
                    0 => format!("removed {age}"),
                    _ => format!("removed {age} · {}", human_size(entry.fp.size)),
                }
            }
            Recovered::Moved { home_shelf, .. } => match home_shelf {
                Some(name) => format!("now on “{name}”"),
                None => "in the library, on no shelf".to_string(),
            },
        }
    }

    fn icon(&self) -> IconName {
        match self.item {
            Recovered::Deleted(_) => IconName::Undo,
            Recovered::Moved { .. } => IconName::Next,
        }
    }

    /// What a click on this row is for, in the words the tooltip uses. A restore
    /// is an explicit act, so it is worth saying out loud that it ignores the
    /// folder's filters — a reader who removed a 12 KB text file and asks for it
    /// back is not asking to be told it is too small.
    fn hint(&self) -> &'static str {
        match self.item {
            Recovered::Deleted(_) => {
                "Add this file back, even if it doesn't match the folder's filters"
            }
            Recovered::Moved { .. } => "This book moved to another shelf",
        }
    }
}

/// Build the rows from the folder's ledger. Synchronous and cheap: two lists the
/// last scan already wrote.
fn candidates(state: AppState, folder_id: &str) -> Vec<RestoreRow> {
    let Some(folder) = state.library.folder(folder_id) else {
        return Vec::new();
    };
    let rows = state.library.books.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    // A link has no fingerprint, so it is not in this index and a folder can
    // never offer one back: it is a pointer at a book, not a copy of a file.
    // Which shelves the folder owns and which a book is on are the ledger's
    // own questions, asked of the shelf list inside `recoverables`.
    let index = index_by_fp(&rows);
    recoverables(&folder, &index, &shelves)
        .into_iter()
        .map(|item| RestoreRow { item, gone: false })
        .collect()
}

/// Pick files and import them, read in place.
///
/// A cancel is not an error and raises nothing; anything else is worth a toast,
/// because the reader asked for this and got no books.
fn from_files(state: AppState, target: Option<String>) {
    spawn_local(async move {
        match pick_documents().await {
            Ok(paths) if paths.is_empty() => {}
            Ok(paths) => import_files(state, paths, target),
            Err(message) => crate::services::library::toast(state, message),
        }
    });
}

/// The same, rooted at a folder the library already knows.
fn from_files_in(state: AppState, root: String, target: Option<String>) {
    spawn_local(async move {
        match pick_documents_in(root).await {
            Ok(paths) if paths.is_empty() => {}
            Ok(paths) => import_files(state, paths, target),
            Err(message) => crate::services::library::toast(state, message),
        }
    });
}

/// Pick a folder and open the import sheet onto it. The sheet is the point: a
/// folder has options (which formats, how small is too small, whether to copy,
/// whether to watch), and importing one on the strength of a picker alone would
/// have to guess all of them.
fn from_directory(sheet: ImportSheet) {
    spawn_local(async move {
        match crate::services::library::pick_folder().await {
            Ok(Some(root)) => sheet.open_on(Some(root)),
            Ok(None) => {}
            Err(message) => sheet.toast(message),
        }
    });
}

/// The shelf a pick made from here lands on: the level the reader is looking at,
/// and nothing at the root — "All" is the library's own order, not a shelf to
/// file onto, so a pick from there leaves its books unfiled and that is the
/// honest answer for a handful of loose files.
///
/// One spelling for the two ways in, because the grid's add card and the list's
/// add row are one door in two shapes: a pick from one that filed somewhere the
/// other would not is a door that behaves differently depending on which layout
/// the reader happens to be looking at.
pub(crate) fn add_target(state: AppState) -> Signal<Option<String>> {
    Signal::derive(move || {
        let id = state.library.shelf.get();
        (id != ALL_SHELF).then_some(id)
    })
}

/// Which face the add trigger wears. The three doors to one menu are three
/// affordances in three layouts, and the wiring behind them — the open flag,
/// the anchor, the target the picks file onto — is one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AddFace {
    /// The grid's last cell: a cover-shaped dashed button. It is a card and
    /// not a toolbar button because the shelf is where the reader is looking
    /// when they decide to add to it, and a grid with a hole at the end reads
    /// as unfinished — the shape is the affordance: same box as a cover,
    /// dashed instead of painted, plus instead of art. (It wears the book
    /// card's classes for that shape and is not a book, so the selection
    /// dimming in `styles/components/library/select.css` names it as an
    /// exception.)
    Card,
    /// The list's last row.
    Row,
    /// The empty shelf's one call to action, with the drop hint under it.
    /// Files onto NO level: the empty state is about the library, not about
    /// wherever the reader happens to be standing.
    Empty,
}

/// The add trigger: the button, its anchor and the menu, wired once for the
/// three faces.
#[component]
pub(crate) fn AddMenuButton(state: AppState, face: AddFace) -> impl IntoView {
    let open = RwSignal::new(false);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    // A pick made from inside a shelf files onto it; one made from the root
    // has no shelf to file onto. The rule is `add_target`'s, and the empty
    // state's door is the one that files onto nothing on purpose.
    let target = match face {
        AddFace::Empty => Signal::derive(|| None),
        AddFace::Card | AddFace::Row => add_target(state),
    };
    let wrapper = match face {
        AddFace::Card => "book-card book-add",
        AddFace::Row => "relative",
        AddFace::Empty => "relative flex max-w-md flex-col items-center gap-4 text-center",
    };
    let has_tauri = tauri_bridge::has_tauri();

    view! {
        <div node_ref=anchor class=wrapper>
            {match face {
                AddFace::Card => {
                    view! {
                        <button
                            class="book-cover book-add-cover"
                            type="button"
                            aria-label="Add books"
                            aria-haspopup="menu"
                            aria-expanded=move || open.get().to_string()
                            title="Add books"
                            on:click=move |_| open.set(!open.get_untracked())
                        >
                            <Icon name=IconName::Plus size=32 class="text-muted" />
                        </button>
                    }
                        .into_any()
                }
                AddFace::Row => {
                    view! {
                        <button
                            type="button"
                            aria-label="Add books"
                            aria-haspopup="menu"
                            aria-expanded=move || open.get().to_string()
                            title="Add books"
                            on:click=move |_| open.set(!open.get_untracked())
                            class="lib-add-row"
                        >
                            <Icon name=IconName::Plus size=15 />
                            <span>"Add books"</span>
                        </button>
                    }
                        .into_any()
                }
                AddFace::Empty => {
                    view! {
                        <>
                            <Button
                                on_click=move |_| open.set(!open.get_untracked())
                                variant=ButtonVariant::Primary
                                active=Signal::derive(move || open.get())
                                title="Import books"
                            >
                                <Icon name=IconName::Plus size=17 />
                                <span>"Import books"</span>
                            </Button>
                            {has_tauri
                                .then(|| {
                                    view! {
                                        <p class="text-xs text-muted">
                                            {format!(
                                                "Or drop a {} file anywhere in the window",
                                                reader_core::format::kind_list()
                                            )}
                                        </p>
                                    }
                                })}
                        </>
                    }
                        .into_any()
                }
            }}
            <AddMenu state=state open=open anchor=anchor target=target />
        </div>
    }
}

#[component]
pub(crate) fn AddMenu(
    state: AppState,
    open: RwSignal<bool>,
    anchor: NodeRef<html::Div>,
    /// The shelf a pick lands on. `None` files onto no shelf, which leaves the
    /// books in "All" — the honest answer for a pick made from the root. Read at
    /// click time rather than at mount: the trigger outlives a drill in or out.
    target: Signal<Option<String>>,
) -> impl IntoView {
    let sheet = use_context::<ImportSheet>().expect("the library page provides the import sheet");

    // The folder this menu belongs to, if the page is drilled into one of its
    // shelves. Derived rather than passed: the trigger is mounted once and the
    // shelf underneath it changes.
    let folder_id = Signal::derive(move || {
        let id = state.library.shelf.get();
        if id == ALL_SHELF {
            return None;
        }
        state.library.shelves.with(|shelves| {
            find(shelves, &id).and_then(|s| s.kind.folder_id().map(str::to_string))
        })
    });

    let rows = RwSignal::new(Vec::<RestoreRow>::new());
    // A moved-book row does not act immediately: "also show it here" and "go and
    // look at where it went" are different answers, so the list swaps for a
    // two-choice confirm inside the same popover. No modal lane is involved, which
    // is the point — a confirm that evicted the menu it came from would close the
    // thing the reader was reading.
    let confirm = RwSignal::new(None::<Recovered>);

    // Build the rows when the menu opens, then measure the removed ones. A row
    // whose file is gone stays listed and goes quiet, rather than disappearing
    // between the click and the paint.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        confirm.set(None);
        let Some(folder_id) = folder_id.get() else {
            rows.set(Vec::new());
            return;
        };
        let built = candidates(state, &folder_id);
        rows.set(built.clone());
        let paths: Vec<String> = built
            .iter()
            .filter_map(|row| match &row.item {
                Recovered::Deleted(entry) => Some(entry.last_path.clone()),
                Recovered::Moved { .. } => None,
            })
            .collect();
        if paths.is_empty() {
            return;
        }
        spawn_local(async move {
            let Ok(checks) = crate::services::library::verify_paths(paths).await else {
                return;
            };
            rows.update(|rows| {
                for check in &checks {
                    if let Some(row) = rows
                        .iter_mut()
                        .find(|r| deleted_path(r).is_some_and(|p| p == check.path))
                    {
                        row.gone = !check.exists;
                    }
                }
            });
        });
    });

    let has_rows = Signal::derive(move || rows.with(|r| !r.is_empty()));

    view! {
        <MenuPopover
            open=open
            anchor=anchor
            width=264u32
            class="max-h-80 overflow-y-auto p-1".to_string()
        >
            {move || {
                if let Some(item) = confirm.get() {
                    return view! {
                        <Confirm
                            state=state
                            open=open
                            confirm=confirm
                            item=item
                            target=target
                        />
                    }
                        .into_any();
                }
                view! {
                    <>
                        <MenuItem
                            icon=IconName::Open
                            label="Choose files…"
                            on_click=move || {
                                open.set(false);
                                from_files(state, target.get_untracked());
                            }
                        />
                        <MenuItem
                            icon=IconName::Library
                            label="Choose a folder…"
                            on_click=move || {
                                open.set(false);
                                from_directory(sheet);
                            }
                        />
                        {move || {
                            folder_id.get().and_then(|id| state.library.folder(&id)).map(|folder| {
                                let root = folder.root.clone();
                                view! {
                                    <>
                                        <Separator spacing="my-1" />
                                        <MenuItem
                                            icon=IconName::Drop
                                            label="Choose files from this folder"
                                            sublabel=folder_label(&root)
                                            on_click=move || {
                                                open.set(false);
                                                from_files_in(
                                                    state,
                                                    root.clone(),
                                                    target.get_untracked(),
                                                );
                                            }
                                        />
                                    </>
                                }
                            })
                        }}
                        {move || {
                            has_rows.get().then(|| {
                                view! {
                                    <>
                                        <Separator spacing="my-1" />
                                        <SectionLabel text="Restore" />
                                        {move || {
                                            let now = crate::time::now_ms();
                                            rows.get()
                                                .into_iter()
                                                .map(|row| {
                                                    view! {
                                                        <RestoreItem
                                                            state=state
                                                            open=open
                                                            confirm=confirm
                                                            row=row
                                                            now=now
                                                        />
                                                    }
                                                })
                                                .collect_view()
                                        }}
                                    </>
                                }
                            })
                        }}
                    </>
                }
                    .into_any()
            }}
        </MenuPopover>
    }
}

/// The address a removed-book row would restore from, for matching a path check
/// back to the row it was about.
fn deleted_path(row: &RestoreRow) -> Option<String> {
    match &row.item {
        Recovered::Deleted(entry) => Some(entry.last_path.clone()),
        Recovered::Moved { .. } => None,
    }
}

/// The folder the page is drilled into, if it is one of a watched folder's
/// shelves. Read at click time: a restore has to name the folder it restores
/// through, and the menu outlives the render that built it.
fn current_folder_id(state: AppState) -> Option<String> {
    let shelf_id = state.library.shelf.get_untracked();
    if shelf_id == ALL_SHELF {
        return None;
    }
    state.library.shelf_folder_id(&shelf_id)
}

/// One restore row. Its own component because a row is four strings and a branch,
/// and building all of that inside an enumerated `map` would be a closure per
/// signal handle for no reason.
#[component]
fn RestoreItem(
    state: AppState,
    open: RwSignal<bool>,
    confirm: RwSignal<Option<Recovered>>,
    row: RestoreRow,
    now: u64,
) -> impl IntoView {
    let label = row.label();
    let sublabel = row.sublabel(now);
    let hint = row.hint().to_string();
    let gone = row.gone;
    // Everything the row needs, taken before `item` moves: the icon is a question
    // about the row, and asking it afterwards would be asking a moved value.
    let icon = row.icon();
    let item = row.item;

    view! {
        <MenuItem
            icon=icon
            label=label
            sublabel=sublabel
            title=hint
            disabled=gone
            on_click=move || {
                match item.clone() {
                    Recovered::Deleted(entry) => {
                        let Some(folder_id) = current_folder_id(state) else {
                            return;
                        };
                        open.set(false);
                        // The row comes off the list when the restore lands, not
                        // before: a measurement that comes back empty keeps the
                        // tombstone, and a menu that had already forgotten it would
                        // have nowhere to put it back.
                        restore_deleted_book(state, folder_id, entry.fp);
                    }
                    Recovered::Moved { .. } => confirm.set(Some(item.clone())),
                }
            }
        />
    }
}

/// The two-choice confirm a moved-book row swaps the list for.
#[component]
fn Confirm(
    state: AppState,
    open: RwSignal<bool>,
    confirm: RwSignal<Option<Recovered>>,
    item: Recovered,
    target: Signal<Option<String>>,
) -> impl IntoView {
    let (book_id, title, home) = match &item {
        Recovered::Moved {
            book_id,
            title,
            home_shelf,
            ..
        } => (book_id.clone(), title.clone(), home_shelf.clone()),
        // Only a moved row ever opens a confirm; anything else is a bug in the
        // caller and an empty block is a quieter answer than a panic.
        Recovered::Deleted(_) => (String::new(), None, None),
    };
    let label = title.clone().unwrap_or_else(|| "This book".to_string());
    let go_label = match &home {
        Some(name) => format!("Show it in {name}"),
        None => "Show it in Home".to_string(),
    };
    let here_id = book_id.clone();
    let go_id = book_id;
    let question = format!("“{label}” is on another shelf.");

    view! {
        <>
            <MenuItem
                icon=IconName::Prev
                label="Back"
                on_click=move || confirm.set(None)
            />
            <Separator spacing="my-1" />
            <p class="px-2 py-1.5 text-xs text-muted">{question}</p>
            <MenuItem
                icon=IconName::Plus
                label="Also show it here"
                sublabel="One book, two shelves — nothing is copied".to_string()
                on_click=move || {
                    open.set(false);
                    if let Some(shelf_id) = target.get_untracked() {
                        crate::services::library::also_show(state, &here_id, &shelf_id);
                    }
                }
            />
            <MenuItem
                icon=IconName::Next
                label=go_label
                sublabel="Closes this menu and takes you to it".to_string()
                on_click=move || {
                    // Closed BEFORE navigating: the menu is anchored to a trigger
                    // inside the grid that is about to be replaced, and a popover
                    // left measuring a node that no longer exists is a popover in
                    // the wrong place.
                    open.set(false);
                    crate::services::library::reveal_book(state, &go_id);
                }
            />
        </>
    }
}

/// A book the menu would offer, for the one test this file can honestly hold: a
/// row's words are a rule, not decoration.
#[cfg(test)]
mod tests {
    use super::RestoreRow;
    use library_core::folder::Tombstone;
    use library_core::ledger::Recovered;
    use library_core::book::Fingerprint;
    use reader_core::format::Format;

    const MINUTE: u64 = 60_000;
    const NOW: u64 = 1_700_000_000_000;

    fn fp(size: u64) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: 1,
            head_hash: 1,
        }
    }

    fn removed(title: Option<&str>, path: &str, size: u64, ago_ms: u64) -> RestoreRow {
        RestoreRow {
            item: Recovered::Deleted(Tombstone {
                fp: fp(size),
                title: title.map(str::to_string),
                format: Format::Pdf,
                last_path: path.to_string(),
                shelf_id: None,
                removed_ms: NOW - ago_ms,
                moved: false,
                returned_row: None,
            }),
            gone: false,
        }
    }

    #[test]
    fn a_removed_book_is_named_by_its_title_or_by_its_file() {
        assert_eq!(removed(Some("Dune"), "/books/dune.pdf", 1, 0).label(), "Dune");
        assert_eq!(
            removed(None, "/books/rust-book.pdf", 1, 0).label(),
            "rust-book"
        );
    }

    #[test]
    fn a_removed_book_says_how_long_ago_and_how_big() {
        let row = removed(Some("Dune"), "/books/dune.pdf", 12 * 1024 * 1024, 3 * MINUTE);
        assert_eq!(row.sublabel(NOW), "removed 3 minutes ago · 12 MB");
    }

    #[test]
    fn a_book_never_measured_shows_no_size_it_does_not_have() {
        // A placeholder fingerprint's size is the length of its path. Printing it
        // would put a number on a menu row that means nothing.
        let row = RestoreRow {
            item: Recovered::Deleted(Tombstone {
                fp: Fingerprint {
                    size: 18,
                    mtime_ms: 0,
                    head_hash: 1,
                },
                title: Some("Dune".into()),
                format: Format::Pdf,
                last_path: "/books/dune.pdf".into(),
                shelf_id: None,
                removed_ms: NOW - MINUTE,
                moved: false,
                returned_row: None,
            }),
            gone: false,
        };
        assert_eq!(row.sublabel(NOW), "removed 1 minute ago");
    }

    #[test]
    fn a_row_whose_file_is_gone_says_so_instead_of_vanishing() {
        let row = RestoreRow {
            gone: true,
            ..removed(Some("Dune"), "/books/dune.pdf", 1, MINUTE)
        };
        assert_eq!(row.sublabel(NOW), "not there any more");
    }

    #[test]
    fn a_moved_book_names_the_shelf_it_went_to() {
        let row = RestoreRow {
            item: Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/dune.pdf".into(),
                home_shelf: Some("Fiction".into()),
            },
            gone: false,
        };
        assert_eq!(row.label(), "Dune");
        assert_eq!(row.sublabel(NOW), "now on “Fiction”");
        // Built rather than updated: `Recovered` is an enum, and a struct-update
        // on one is not a thing.
        let homeless = RestoreRow {
            item: Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/dune.pdf".into(),
                home_shelf: None,
            },
            gone: false,
        };
        assert_eq!(homeless.sublabel(NOW), "in the library, on no shelf");
    }

    #[test]
    fn a_restore_says_that_it_ignores_the_folders_filters() {
        // The one thing a reader could reasonably be surprised by, so it is on the
        // row rather than in a document.
        let row = removed(Some("Dune"), "/books/dune.pdf", 1, 0);
        assert!(row.hint().contains("filters"));
    }

}
