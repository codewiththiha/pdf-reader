//! A shelf on the page, drawn as a folder: the first four covers inside it on a
//! 2×2 plate, its name, and what it holds.
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
//! drag files a book dropped on it or nests a shelf dropped on it.

use std::rc::Rc;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::{ALL_SHELF, Shelf, can_nest, children_of};

use crate::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use crate::components::primitives::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::drag::{self, DropTarget};
use crate::features::library::selection::{enter_selection, toggle_selected};
use crate::services::library::{move_to_shelf, nest_shelf};
use crate::state::AppState;

/// How many covers the plate shows. Two by two: a folder is recognised by the
/// art inside it, and past four cells the plate is a mosaic nobody reads — the
/// line under the name is the answer to "how much".
const THUMB_CAP: usize = 4;

#[component]
pub(crate) fn FolderCard(state: AppState, shelf: Shelf) -> impl IntoView {
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");

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

    // The plate's covers: this shelf's members resolved against the library, cut
    // at the cap. A member that names no book is skipped rather than drawn as a
    // blank cell, so the plate shows the art that is actually there.
    let thumb_id = id.clone();
    let thumbs = Signal::derive(move || {
        let members: Vec<String> = state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == thumb_id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        });
        state.library.books.with(|books| {
            members
                .iter()
                .filter_map(|member| books.iter().find(|b| &b.id == member))
                .take(THUMB_CAP)
                .cloned()
                .collect::<Vec<Book>>()
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

    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let selected_id = id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // One wrapper, three gestures, and the mode is decided once per press.
    let hold_id = id.clone();
    let tap_id = id.clone();
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // While a set is selected the pointer is choosing, not filing.
        draggable: Signal::derive(move || !selecting.get()),
        // And a hold inside a selection would be a second way to do the thing a
        // tap now does.
        selectable: Signal::derive(move || !selecting.get()),
        on_tap: Callback::new(move |_| {
            if selecting.get_untracked() {
                toggle_selected(state, &tap_id);
                return;
            }
            state.library.shelf.set(tap_id.clone());
        }),
        on_long_press: Callback::new(move |_| enter_selection(state, &hold_id)),
        on_drag_start: Callback::new(move |_| {
            // The pointer has committed to a move, so the marker under it
            // belongs to the drag that has just ended rather than to this one.
            drop_target.0.set(None);
        }),
        // The browser's own drag carries the payload and the coordinates; this
        // half only decided that a movement meant "drag" and not "hold".
        on_drag_move: Callback::new(move |_| {}),
        on_drag_end: Callback::new(move |_| drop_target.0.set(None)),
    });
    let pressing = item.pressing;
    let dragging = item.dragging;
    let on_down = Rc::clone(&item.on_pointerdown);
    let on_move = Rc::clone(&item.on_pointermove);
    let on_up = Rc::clone(&item.on_pointerup);
    let on_cancel = Rc::clone(&item.on_pointercancel);
    let swallow_click = Rc::clone(&item.swallow_click);
    let swallow_context = Rc::clone(&item.swallow_context);

    let over_marker = DropTarget::folder(&id);
    let drag_marker = over_marker.clone();
    let leave_marker = over_marker.clone();
    let start_id = id.clone();
    let context_id = id.clone();
    let over_id = id.clone();
    let nest_id = id.clone();
    let drop_id = id.clone();
    let key_id = id.clone();
    let hold_key_id = id.clone();
    let pressed_id = id.clone();
    let dom_id = format!("folder-{}", id);

    view! {
        <div
            id=dom_id
            class="folder-card"
            class=("folder-selected", move || is_selected.get())
            class=("folder-pressing", move || pressing.get())
            class=("folder-dragging", move || dragging.get())
            class=("folder-drag-over", move || {
                drop_target.0.with(|at| at.as_deref() == Some(over_marker.as_str()))
            })
            role="button"
            tabindex="0"
            // Dragging a shelf one level deeper; while a set is selected the
            // pointer is choosing, not filing.
            draggable=move || if selecting.get() { "false" } else { "true" }
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
            on:dragstart=move |ev| drag::begin_folder(&ev, &start_id)
            on:dragend=move |_| drop_target.0.set(None)
            on:dragover=move |ev| {
                // A book is always welcome. A shelf is welcome unless filing it
                // here would put it inside itself — and while the drag data
                // store is protected the payload cannot be read, so the drop is
                // the half that refuses what the cursor could not.
                if drag::accepts_book(&ev) {
                    drop_target.0.set(Some(drag_marker.clone()));
                    return;
                }
                let closes_a_loop = drag::dragged_folder(&ev).is_some_and(|moved| {
                    !can_nest(&state.library.shelves.get_untracked(), &moved, &over_id)
                });
                if closes_a_loop {
                    return;
                }
                if drag::accepts_folder(&ev) {
                    drop_target.0.set(Some(drag_marker.clone()));
                }
            }
            on:dragleave=move |_| drop_target.release(&leave_marker)
            on:drop=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                drop_target.0.set(None);
                let from = state.library.shelf.get_untracked();
                let source = (from != ALL_SHELF).then_some(from);
                if let Some(book_id) = drag::dragged(&ev) {
                    move_to_shelf(state, book_id, source, drop_id.clone(), None);
                    return;
                }
                // `nest_shelf` asks `can_nest` again: the dragover could not
                // always read the payload, and a drop that closed a loop would
                // leave a folder no level renders.
                if let Some(moved) = drag::dragged_folder(&ev) {
                    nest_shelf(state, &moved, Some(nest_id.as_str()));
                }
            }
        >
            <div
                class="folder-thumb-grid"
                class=("folder-thumb-single", move || counts.get().0 == 1)
            >
                {move || {
                    let shown = thumbs.get();
                    (0..THUMB_CAP)
                        .map(|at| {
                            match shown.get(at) {
                                Some(book) => {
                                    view! { <FolderThumb state=state book=book.clone() /> }
                                        .into_any()
                                }
                                None => {
                                    view! {
                                        <span class="folder-thumb-cell folder-thumb-empty"></span>
                                    }
                                        .into_any()
                                }
                            }
                        })
                        .collect_view()
                }}
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

/// One cell of the plate holding a book: the cached cover when there is one, and
/// nothing — the cell's own hatched background — when there is not yet.
///
/// All four cells are drawn whatever the folder holds, so a folder of two books
/// and a folder of four are the same shape on the shelf and the plate never
/// reflows as books arrive. The exception is a folder of exactly one, where a
/// quarter-sized cover is a thumbnail of a thumbnail: the empty cells go and the
/// one book takes the plate, which is what `.folder-thumb-single` says in
/// `styles/library.css`.
#[component]
fn FolderThumb(state: AppState, book: Book) -> impl IntoView {
    let path = book.path().to_string();
    let alt = book.title();

    view! {
        <span class="folder-thumb-cell">
            {move || {
                match state
                    .library
                    .covers
                    .with(|covers| covers.get(&path).cloned())
                {
                    Some(cover) => {
                        view! {
                            <img
                                class="folder-thumb-img"
                                src=cover.data_url.clone()
                                alt=alt.clone()
                                loading="lazy"
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
