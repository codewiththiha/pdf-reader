//! The list: one row per book, for a library the reader is scanning rather than
//! browsing.
//!
//! Same books, same order, same drag rules and the same three gestures as the grid
//! — the only thing that changes is the shape of a row, which is why the order
//! arrives from the same context signal rather than being derived a second time
//! here. A reader who switched to the denser layout has not thereby lost the way
//! in, the way out, or the hold that starts a selection.
//!
//! A row is the same drop target a card is, and registers under the same kind: a
//! book at one density and the same book at the other are one thing to a drag, and
//! a session that had to be told which layout was showing would be a session that
//! could only drop on the one the reader happened to be looking at.
//!
//! A row shows the author when the book has one and the resume point when it does
//! not: at this density there is room for one line of prose and the reader gets to
//! choose which by opening the book.
//!
//! Folders are NOT here, and were not before either: the shelf tile only ever
//! rendered in the grid, so the dense layout has always been the books and the way
//! in. The breadcrumb is the way around a folder from here — and, being a drop
//! target too, the way to file onto a shelf from here — and a reader who wants to
//! see the folders is a reader who wants the grid.

use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::shelf::ALL_SHELF;
use library_core::view::CoverFit;
use reader_core::format::Format;

use crate::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use crate::components::primitives::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::add_menu::AddMenu;
use crate::features::library::content::ShelfOrder;
use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::document;
use crate::state::AppState;

#[component]
pub(crate) fn ListView(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));

    view! {
        <div
            class="library-list divide-y divide-line rounded-xl border border-line"
            class=("library-list-selecting", move || state.library.selecting.get())
        >
            <For each=move || order.0.get() key=|b| b.id.clone() let:book>
                <ListRow state=state book=book crop=crop />
            </For>
            // The grid ends in an add card, so the list ends in an add row: the two
            // layouts are the same library, and a reader who switched to the denser
            // one has not thereby lost the way in.
            <AddRow state=state />
        </div>
    }
}

/// The list's last row: the same two sources the grid's add card offers, in the
/// shape of a row rather than the shape of a cover.
#[component]
fn AddRow(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let target = Signal::derive(move || {
        let id = state.library.shelf.get();
        (id != ALL_SHELF).then_some(id)
    });
    view! {
        <div node_ref=anchor class="relative">
            <button
                type="button"
                aria-label="Add books"
                aria-haspopup="menu"
                aria-expanded=move || open.get().to_string()
                title="Add books"
                on:click=move |_| open.set(!open.get_untracked())
                class="library-add-row"
            >
                <Icon name=IconName::Plus size=15 />
                <span>"Add books"</span>
            </button>
            <AddMenu state=state open=open anchor=anchor target=target />
        </div>
    }
}

#[component]
fn ListRow(state: AppState, book: Book, crop: Signal<bool>) -> impl IntoView {
    // Both, because the card keeps its own ✕: the menu is what a right-click asks
    // and the sheet is what a removal costs, and the second is reached from the
    // first as well as from the button.
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let drag = use_context::<DragController>().expect("the library page installs the drag session");

    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let selected_id = book.id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    let id = book.id.clone();
    let path = book.path().to_string();
    let title = book.title();
    let author = book.author().unwrap_or_else(|| {
        if book.num_pages > 0 {
            format!("Page {} of {}", book.page, book.num_pages)
        } else {
            format!("Page {}", book.page)
        }
    });
    let missing = book.missing;
    let percent = book.progress().map(|p| format!("{:.0}%", p * 100.0));
    // The list has room for the format on every row, and at this density a reader
    // is scanning names rather than looking at art — so the kind of thing a row is
    // earns its place here in a way a chip on a cover would not.
    let chip = (book.format != Format::Pdf).then(|| book.format.label().to_string());
    let path_hint = book.path().to_string();

    // One wrapper, three gestures, and the mode is decided once per press — the
    // same wrapper the grid's cards use, so a row and a card answer a hold, a tap
    // and a movement alike at two densities.
    let press_id = id.clone();
    let tap_id = id.clone();
    let tap_path = path.clone();
    let lift_id = id.clone();
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // A movement is always a drag here, including from inside a selection: a
        // set that could not be lifted was a set the bar's "Add to shelf" was the
        // only way to move.
        draggable: Signal::derive(|| true),
        selectable: Signal::derive(move || !selecting.get()),
        on_tap: Callback::new(move |_| {
            if selecting.get_untracked() {
                toggle_selected(state, &tap_id);
                return;
            }
            document::open_path(state, tap_path.clone());
        }),
        on_long_press: Callback::new(move |_| enter_selection(state, &press_id)),
        on_drag_start: Callback::new(move |(x, y)| {
            drag.begin(payload_for(state, &lift_id), x, y);
        }),
        // The session owns the move; see `crate::features::library::book_card` for
        // why its listeners are on the window rather than on the row.
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

    // The same target a card registers, under the same id scheme: a book is one
    // thing to a drag whatever density it is being shown at.
    let dom_id = format!("book-{}", id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Book, id.clone()),
        dom_id: dom_id.clone(),
    });

    let context_id = id.clone();
    let context_path = path.clone();
    let key_id = id.clone();
    let aria_id = id.clone();
    let select_key_id = id.clone();
    let reveal_id = id.clone();
    let remove_id = id.clone();
    let over_id = id.clone();
    let fold_id = id.clone();
    let held_id = id.clone();
    let alt_path = path.clone();
    let alt_title = title.clone();
    let row_title = title.clone();
    let row_tooltip = title.clone();
    let row_label = title.clone();

    view! {
        <div
            id=dom_id
            class="library-row"
            class=("row-reveal", move || {
                state.library.reveal.with(|at| {
                    at.as_ref()
                        .is_some_and(|(id, _)| id == reveal_id.as_str())
                })
            })
            class=("row-drop-before", move || drag.inserts_before(&over_id))
            class=("row-fold-here", move || drag.folds_with(&fold_id))
            class=("row-missing", missing)
            class=("library-row-selected", move || is_selected.get())
            class=("library-row-pressing", move || pressing.get())
            class=("library-row-dragging", move || drag.holds(&held_id))
            role="button"
            tabindex="0"
            aria-label=move || {
                if selecting.get() {
                    format!("Select {row_label}")
                } else {
                    format!("Open {row_label}")
                }
            }
            aria-pressed=move || {
                selecting.get().then(|| {
                    if selected_set.with(|s| s.contains(&aria_id)) {
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
                // completed hold is not an intention to open the book.
                if (swallow_click)() {
                    ev.stop_propagation();
                }
            }
            // The same answer a card gives, at this density; see `book_card`.
            on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                // Stopped before the swallow is asked; see `book_card`.
                ev.prevent_default();
                ev.stop_propagation();
                if (swallow_context)() {
                    return;
                }
                let (x, y) = (ev.client_x() as f64, ev.client_y() as f64);
                let in_set = selecting.get_untracked()
                    && selected_set.with_untracked(|set| set.contains(&context_id));
                if in_set {
                    menu.ask(x, y, MenuTarget::Selection);
                    return;
                }
                menu.ask(
                    x,
                    y,
                    MenuTarget::Book {
                        id: context_id.clone(),
                        path: context_path.clone(),
                        missing,
                    },
                );
            }
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() != "Enter" {
                    return;
                }
                // Shift+Enter is the keyboard's long-press; see `book_card`.
                if ev.shift_key() && !selecting.get_untracked() {
                    ev.prevent_default();
                    enter_selection(state, &select_key_id);
                    return;
                }
                if selecting.get_untracked() {
                    toggle_selected(state, &key_id);
                    return;
                }
                document::open_path(state, path.clone());
            }
        >
            <span
                class="library-row-cover"
                class=("book-cover-crop", move || crop.get())
            >
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
                {move || {
                    state
                        .library
                        .covers
                        .with(|covers| covers.get(&alt_path).cloned())
                        .map(|cover| {
                            view! {
                                // Not natively draggable; see `book_card`.
                                <img
                                    class="library-row-img"
                                    src=cover.data_url.clone()
                                    alt=alt_title.clone()
                                    loading="lazy"
                                    draggable="false"
                                />
                            }
                        })
                }}
            </span>
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=row_tooltip.clone()>
                    {row_title}
                </span>
                <span class="block truncate text-xs text-muted" title=path_hint.clone()>
                    {author}
                </span>
            </span>
            {chip.map(|label| view! { <span class="library-row-format">{label}</span> })}
            {percent.map(|p| {
                view! {
                    <span class="shrink-0 text-xs tabular-nums text-muted">{p}</span>
                }
            })}
            <button
                class="library-row-remove"
                type="button"
                title="Remove from library"
                aria-label="Remove from library"
                on:click=move |ev: leptos::ev::MouseEvent| {
                    ev.stop_propagation();
                    remove_sheet.ask(&remove_id);
                }
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
