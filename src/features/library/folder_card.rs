//! A shelf on the page, drawn as a folder: a 2×2 plate of what is inside it —
//! covers for its books and a plate of their own for its folders, recursively —
//! then its name and what it holds.
//!
//! It replaces the shelf tile, which spanned the whole grid as a row of spines
//! on a board. That shape could not nest — a row is a section, and a section
//! cannot be inside another section — so a shelf inside a shelf had nowhere to
//! be drawn. A folder is a cell like a book's, which is what makes every level
//! of the library the same shape as the one above it.
//!
//! Called a folder because that is what it looks like; the domain word stays
//! *shelf*, because *folder* already means a watched directory in
//! `library_core`, and the two are different facts about the same row (see
//! [`Shelf::parent`]'s note on `ShelfKind::Folder`).
//!
//! Three gestures share the card and one wrapper decides between them — see
//! `crate::components::primitives::interactions::draggable_item`. A tap opens
//! the shelf, a hold starts a multi-select with this shelf already in it, and a
//! movement hands the press to `crate::features::library::dnd`, which is what
//! files a book dropped on it or nests a shelf dropped on it.
//!
//! The one card that does not drag is a shelf cut from a watched folder: its rung
//! in the library is the rung its directory has on disk, re-hung on every scan, so
//! a hand-move would be a promise the next rescan breaks. It still opens, still
//! selects, and still takes the books and virtual shelves dropped on it.
//!
//! A drop this folder refuses — a shelf that would end up inside itself — wears no
//! ring at all, which is the honest half of the gesture: the decision table says
//! no before the pointer gets there, so the reader is never offered a drop that
//! the commit step would then quietly decline.

use std::rc::Rc;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{Shelf, children_of};

use crate::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use crate::components::primitives::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::state::AppState;

/// How many cells a plate has, and the most it fills. Two by two: a folder is
/// recognised by what is inside it, and past four cells the plate is a mosaic
/// nobody reads — the line under the name is the answer to "how much". Always
/// four cells whatever the folder holds, so one book is one cover and three
/// hatched quarters rather than one big rectangle that reads as a book card.
///
/// Shared with the fold preview a drag draws
/// (`crate::features::library::dnd::layer`), because that preview is a promise
/// about this plate and a promise drawn with a different number of cells is a
/// promise about a folder the library does not have.
pub(crate) const THUMB_CAP: usize = 4;

/// The deepest plate the preview recurses to: the folder's own plate, the plates
/// of the folders inside it, and the plates of the folders inside those. Deeper
/// than that a cell is a few pixels across, and draws a folder glyph instead.
const PLATE_DEPTH: usize = 2;

#[component]
pub(crate) fn FolderCard(state: AppState, shelf: Shelf) -> impl IntoView {
    let drag = use_context::<DragController>().expect("the library page installs the drag session");

    // The prop is the shelf the `For` keyed this row on, and a keyed row is not
    // re-created when the shelf's CONTENTS change — a book filed into it, a
    // rename, a shelf nested inside. So everything that can move is read back
    // out of the state by id, and the prop supplies only the identity plus the
    // two facts a rescan owns and a reader cannot change from here.
    let id = shelf.id.clone();
    let watched_folder = shelf.kind.folder_id().map(str::to_string);

    let name_id = id.clone();
    let name = Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == name_id)
                .map(|s| s.name.clone())
                .unwrap_or_default()
        })
    });

    // Two counts, one line: what the folder holds, and what it holds it in.
    let count_id = id.clone();
    let counts = Signal::derive(move || {
        let books = state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == count_id)
                .map(|s| s.books.len())
                .unwrap_or_default()
        });
        let inside = state
            .library
            .shelves
            .with(|shelves| children_of(shelves, Some(count_id.as_str())).len());
        (books, inside)
    });

    // Whether the folder this shelf was cut from is still being watched: the
    // breathing dot that says "this shelf may fill itself". It needs the folder
    // row, so it is a reactive read rather than a copy taken at mount.
    let watched = Signal::derive(move || {
        let Some(folder_id) = watched_folder.clone() else {
            return false;
        };
        state.library.folders.with(|folders| {
            folders.iter().any(|f| f.id == folder_id && f.opts.watch)
        })
    });

    // A shelf cut from a watched tree is the tree's to place — every scan re-hangs
    // it on the rung its `rel` names — so offering it a drag would be offering a
    // move the next rescan undoes. Virtual shelves drag freely.
    let disk_id = id.clone();
    let disk_bound = Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == disk_id)
                .is_some_and(|s| s.is_folder())
        })
    });

    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let selected_id = id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // One wrapper, three gestures, and the mode is decided once per press.
    let hold_id = id.clone();
    let tap_id = id.clone();
    let lift_id = id.clone();
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // A shelf the disk places is not one the pointer gets to place. A set
        // being selected is not a reason to refuse: lifting one of three held
        // folders is the whole of what a multi-drag is.
        draggable: Signal::derive(move || !disk_bound.get()),
        // A hold inside a selection would be a second way to do the thing a tap
        // now does.
        selectable: Signal::derive(move || !selecting.get()),
        on_tap: Callback::new(move |_| {
            if selecting.get_untracked() {
                toggle_selected(state, &tap_id);
                return;
            }
            state.library.shelf.set(tap_id.clone());
        }),
        on_long_press: Callback::new(move |_| enter_selection(state, &hold_id)),
        on_drag_start: Callback::new(move |(x, y)| {
            drag.begin(payload_for(state, &lift_id), x, y);
        }),
        // The session owns the move; see `crate::features::library::book_card`
        // for why its listeners are on the window rather than on the card.
        on_drag_move: Callback::new(move |_| {}),
        on_drag_end: Callback::new(move |(x, y)| drag.release(x, y)),
        on_drag_cancel: Callback::new(move |_| drag.cancel()),
    });
    let pressing = item.pressing;
    let on_down = Rc::clone(&item.on_pointerdown);
    let on_move = Rc::clone(&item.on_pointermove);
    let on_up = Rc::clone(&item.on_pointerup);
    let on_cancel = Rc::clone(&item.on_pointercancel);
    let swallow_click = Rc::clone(&item.swallow_click);
    let swallow_context = Rc::clone(&item.swallow_context);

    // A target as well as a payload: a drop here files the held books on this
    // shelf and nests the held folders inside it, unless doing that would close a
    // loop — which the session asks `library_core::shelf::can_nest` before the
    // pointer ever arrives, so a refused drop wears no ring.
    let dom_id = format!("folder-{}", id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Folder, id.clone()),
        dom_id: dom_id.clone(),
    });

    let context_id = id.clone();
    let over_id = id.clone();
    let held_id = id.clone();
    let key_id = id.clone();
    let hold_key_id = id.clone();
    let pressed_id = id.clone();

    view! {
        <div
            id=dom_id
            class="folder-card"
            class=("folder-selected", move || is_selected.get())
            class=("folder-pressing", move || pressing.get())
            class=("folder-dragging", move || drag.holds(&held_id))
            class=("folder-drag-over", move || drag.nests_into(&over_id))
            role="button"
            tabindex="0"
            aria-label=move || {
                let shown = name.get();
                if selecting.get() {
                    format!("Select the {shown} shelf")
                } else {
                    format!("Open the {shown} shelf")
                }
            }
            aria-pressed=move || {
                selecting.get().then(|| {
                    if selected_set.with(|s| s.contains(&pressed_id)) {
                        "true"
                    } else {
                        "false"
                    }
                })
            }
            on:pointerdown=move |ev| (on_down)(&ev)
            on:pointermove=move |ev| (on_move)(&ev)
            on:pointerup=move |ev| (on_up)(&ev)
            on:pointercancel=move |ev| (on_cancel)(&ev)
            on:click=move |ev: leptos::ev::MouseEvent| {
                // The hold's exhaust and nothing else: the wrapper already
                // decided what this press meant, and the click that follows a
                // completed hold is not an intention to open the shelf.
                if (swallow_click)() {
                    ev.stop_propagation();
                }
            }
            on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                // WebKit answers a hold with a contextmenu as well as with the
                // selection. This card has no menu of its own — a shelf's rename
                // and its removal live on the crumb that names it — so the
                // platform's menu is kept down either way, and inside a
                // selection the same button toggles the way it does on a book.
                ev.prevent_default();
                if (swallow_context)() {
                    return;
                }
                ev.stop_propagation();
                if selecting.get_untracked() {
                    toggle_selected(state, &context_id);
                }
            }
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() != "Enter" {
                    return;
                }
                // A keyboard has no hold to make, so it gets the gesture's two
                // halves as two keys: Shift+Enter enters selection the way a
                // hold does, and once inside, Enter toggles instead of opening.
                if ev.shift_key() && !selecting.get_untracked() {
                    ev.prevent_default();
                    enter_selection(state, &hold_key_id);
                    return;
                }
                if selecting.get_untracked() {
                    toggle_selected(state, &key_id);
                    return;
                }
                state.library.shelf.set(key_id.clone());
            }
        >
            <div class="folder-thumb-grid">
                <Plate state=state shelf_id=id.clone() depth=0 />
                {move || {
                    selecting.get().then(|| {
                        view! {
                            <span class="lib-check" aria-hidden="true">
                                {move || {
                                    is_selected.get().then(|| {
                                        view! { <Icon name=IconName::Check size=11 /> }
                                    })
                                }}
                            </span>
                        }
                    })
                }}
            </div>
            <div class="folder-meta">
                <span class="folder-name" title=move || { name.get() }>
                    {move || name.get()}
                </span>
                <span class="folder-count">{move || summary(counts.get())}</span>
            </div>
            <div class="folder-badges">
                {move || {
                    watched.get().then(|| {
                        view! {
                            <span class="folder-watched" title="Watched for new books"></span>
                        }
                    })
                }}
            </div>
        </div>
    }
}

/// What fills one cell of a plate: a folder, previewed as a plate of its own, or
/// a book, previewed as its cover.
#[derive(Clone)]
enum PlateItem {
    Folder(String),
    Book(Book),
}

/// The first four things inside a shelf, in the order the grid would show them —
/// folders first, then books — because a plate that disagreed with the page about
/// what is inside the folder would be a preview of something else.
///
/// Books and folders share the four cells rather than each having their own four:
/// a folder holding two folders and a book is three cells full and one empty,
/// exactly as the reader will find it.
fn plate_items(state: AppState, shelf_id: &str) -> Vec<PlateItem> {
    let (folders, members): (Vec<String>, Vec<String>) =
        state.library.shelves.with(|shelves| {
            (
                children_of(shelves, Some(shelf_id))
                    .iter()
                    .map(|s| s.id.clone())
                    .collect(),
                shelves
                    .iter()
                    .find(|s| s.id == shelf_id)
                    .map(|s| s.books.clone())
                    .unwrap_or_default(),
            )
        });
    let books: Vec<Book> = state.library.books.with(|books| {
        members
            .iter()
            .filter_map(|member| books.iter().find(|b| &b.id == member).cloned())
            .collect()
    });
    let mut out: Vec<PlateItem> = folders
        .into_iter()
        .map(PlateItem::Folder)
        .chain(books.into_iter().map(PlateItem::Book))
        .collect();
    out.truncate(THUMB_CAP);
    out
}

/// One folder's plate: four cells, filled in order, the rest empty.
///
/// Recursive on purpose. A cell that holds a folder holds that folder's OWN plate
/// — a folder of three books previews as three covers and an empty cell, inside
/// the cell that previews it — because "what is inside this folder" is the same
/// question at every depth, and a preview that flattened the subtree would show
/// covers the reader will not find where the preview put them.
///
/// The recursion stops at [`PLATE_DEPTH`]: a cell four plates deep is a few pixels
/// of something, and below that a folder is drawn as a folder rather than as a
/// smear. The tree is finite — `library_core::shelf::sanitize` sees to that — but
/// four cells per level is four to the power of the depth, and a preview is not
/// worth an exponent.
#[component]
fn Plate(state: AppState, shelf_id: String, depth: usize) -> impl IntoView {
    let items = Signal::derive(move || plate_items(state, &shelf_id));
    view! {
        {move || {
            let items = items.get();
            (0..THUMB_CAP)
                .map(|at| match items.get(at) {
                    Some(PlateItem::Folder(id)) => {
                        let id = id.clone();
                        if depth < PLATE_DEPTH {
                            // The inner grid is the cell's own: a plate is four
                            // cells and nothing else, so a plate inside a cell
                            // needs the container that makes four cells a plate.
                            view! {
                                <span class="folder-thumb-cell">
                                    <span class="folder-thumb-grid">
                                        <Plate state=state shelf_id=id depth=depth + 1 />
                                    </span>
                                </span>
                            }
                                .into_any()
                        } else {
                            view! {
                                <span
                                    class="folder-thumb-cell folder-thumb-deep"
                                    title="A folder, deeper than a preview can show"
                                >
                                    <Icon name=IconName::Open size=12 />
                                </span>
                            }
                                .into_any()
                        }
                    }
                    Some(PlateItem::Book(book)) => {
                        view! { <CoverCell state=state book=book.clone() /> }.into_any()
                    }
                    None => {
                        view! { <span class="folder-thumb-cell folder-thumb-empty"></span> }
                            .into_any()
                    }
                })
                .collect_view()
        }}
    }
}

/// One cell holding a book: the cached cover when there is one, and the empty
/// cell's hatch when there is not yet — a cover is rendered away from the reader,
/// so "nothing cached yet" is a state the plate has to look deliberate in.
#[component]
fn CoverCell(state: AppState, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let empty_path = path.clone();
    let alt = book.title();
    view! {
        <span
            class="folder-thumb-cell"
            class=("folder-thumb-empty", move || {
                state
                    .library
                    .covers
                    .with(|covers| !covers.contains_key(&empty_path))
            })
        >
            {move || {
                match state
                    .library
                    .covers
                    .with(|covers| covers.get(&path).cloned())
                {
                    Some(cover) => {
                        view! {
                            // Not natively draggable; see `book_card`. A plate is
                            // four of these, and any one of them taking the pointer
                            // would take it away from the folder's own gesture.
                            <img
                                class="folder-thumb-img"
                                src=cover.data_url.clone()
                                alt=alt.clone()
                                loading="lazy"
                                draggable="false"
                            />
                        }
                            .into_any()
                    }
                    None => ().into_any(),
                }
            }}
        </span>
    }
}

/// The line under the name: what the folder holds, and what it holds it in.
///
/// Both halves, because "3 books" on a folder with two shelves inside it is an
/// answer to a question the reader did not ask — those shelves are the rest of
/// the library down that path, and a count that leaves them out reads as a
/// folder that is nearly empty.
fn summary(counts: (usize, usize)) -> String {
    let (books, inside) = counts;
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if books > 0 {
        parts.push(plural(books, "book", "books"));
    }
    if inside > 0 {
        parts.push(plural(inside, "shelf", "shelves"));
    }
    if parts.is_empty() {
        "Empty".to_string()
    } else {
        parts.join(" · ")
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_line_says_both_halves_and_neither_when_there_is_nothing() {
        assert_eq!(summary((0, 0)), "Empty");
        assert_eq!(summary((1, 0)), "1 book");
        assert_eq!(summary((3, 0)), "3 books");
        assert_eq!(summary((0, 1)), "1 shelf");
        assert_eq!(summary((0, 2)), "2 shelves");
        assert_eq!(summary((3, 1)), "3 books · 1 shelf");
        assert_eq!(summary((1, 4)), "1 book · 4 shelves");
    }
}
