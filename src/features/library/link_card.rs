//! A link row: the shelf's own card and row shape, with a pointer's facts on it
//! instead of a book's.
//!
//! A link is not a book and cannot borrow a book's card — it has no address to
//! read, no page to render art from, no resume point to draw a bar for and no
//! format to name. What it has is a name (the name of the book it points at,
//! which is what makes it recognisable beside it), a target, and the same three
//! gestures every other row on the shelf answers to: a tap goes to the book, a
//! hold selects it, a movement files it somewhere else. So it wears the shelf's
//! own press contract — [`use_shelf_item`] is the same wiring a card and a list
//! row both wear — and the plate where a cover would be says what it is instead.
//!
//! The one thing a link does that no book does is open somewhere else: a tap
//! reveals the book it points at, on the shelf that book is filed on, and lights
//! its card (`crate::services::library::reveal_book`). That is the whole of the
//! row's purpose, and it is why a link is a row the reader can file anywhere
//! without ever moving a file.

use std::rc::Rc;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::gestures::{ShelfItemPolicy, use_shelf_item};
use crate::features::library::list::row_indent;
use crate::features::library::remove_modal::RemoveSheet;
use crate::services::document;
use crate::state::AppState;

/// The line a link carries where a book carries its author or its page: what it
/// is, and that a tap goes to the book rather than opening a file.
const LINK_LINE: &str = "Link · opens the book where it is";

/// One link on the grid.
#[component]
pub(crate) fn LinkCard(state: AppState, id: String, name: String) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let drag = use_context::<DragController>().expect("the library page installs the drag session");
    // One owned id per closure: each handler is a closure of its own and a
    // `move` takes what it captures.
    let open_id = id.clone();
    let menu_id = id.clone();
    let gestures = use_shelf_item(
        state,
        Some(drag),
        Some(menu),
        ShelfItemPolicy {
            id: id.clone(),
            label: Signal::stored(name.clone()),
            draggable: Signal::derive(|| true),
            open: Callback::new(move |_| document::open_row(state, open_id.clone())),
            // A link is a row like any other to the menu: Open goes to the book,
            // Select and Remove mean what they always mean, and "Find again" is
            // not offered because a pointer has no address to die.
            menu_target: Callback::new(move |_| MenuTarget::Book {
                id: menu_id.clone(),
                missing: false,
            }),
            container: None,
        },
    );
    let is_selected = gestures.is_selected;
    let pressing = gestures.pressing;
    let aria_label = gestures.aria_label;
    let on_down = Rc::clone(&gestures.on_pointerdown);
    let on_move = Rc::clone(&gestures.on_pointermove);
    let on_up = Rc::clone(&gestures.on_pointerup);
    let on_cancel = Rc::clone(&gestures.on_pointercancel);
    let on_click = Rc::clone(&gestures.on_click);
    let on_context = Rc::clone(&gestures.on_contextmenu);
    let on_key = Rc::clone(&gestures.on_keydown);
    let aria_pressed = gestures.aria_pressed;

    let dom_id = crate::features::library::dnd::target::row_dom_id(DropTargetKind::Book, &id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Book, id.clone()),
        dom_id: dom_id.clone(),
        shelf: None,
    });

    let reveal_id = id.clone();
    let held_id = id.clone();
    let over_id = id.clone();
    let fold_id = id.clone();
    let remove_id = id.clone();
    let plate_name = name.clone();
    let title = name.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };

    view! {
        <div
            id=dom_id
            class="book-card book-link"
            class=("book-reveal", move || {
                state.library.reveal.with(|at| {
                    at.as_ref().is_some_and(|(id, _)| id == reveal_id.as_str())
                })
            })
            class=("book-drop-before", move || drag.inserts_before(&over_id))
            class=("book-fold-here", move || drag.folds_with(&fold_id))
            class=("book-selected", move || is_selected.get())
            class=("book-pressing", move || pressing.get())
            class=("book-dragging", move || drag.holds(&held_id))
            role="button"
            tabindex="0"
            aria-label=move || aria_label.get()
            aria-pressed=move || aria_pressed.get()
            on:pointerdown=move |ev| (on_down)(&ev)
            on:pointermove=move |ev| (on_move)(&ev)
            on:pointerup=move |ev| (on_up)(&ev)
            on:pointercancel=move |ev| (on_cancel)(&ev)
            on:click=move |ev: leptos::ev::MouseEvent| (on_click)(&ev)
            on:contextmenu=move |ev: leptos::ev::MouseEvent| (on_context)(&ev)
            on:keydown=move |ev: leptos::ev::KeyboardEvent| (on_key)(&ev)
        >
            <div class="book-cover-wrap">
                <div class="book-cover" style:aspect-ratio=library_core::view::A4_ASPECT_CSS.to_string()>
                    <div class="book-cover-fallback">
                        <span>{plate_name.clone()}</span>
                    </div>
                    <span class="book-link-badge" title="A pointer at a book, not a copy of one">
                        <Icon name=IconName::Link size=11 />
                    </span>
                </div>
            </div>

            <div class="book-info">
                <span class="book-title" title=title.clone()>{name.clone()}</span>
                <span class="book-page">{LINK_LINE}</span>
            </div>

            <button
                type="button"
                class="book-remove"
                title="Remove this link"
                aria-label=move || format!("Remove the link to {}", name.clone())
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}

/// One link in the list, at the depth its branch puts it at.
#[component]
pub(crate) fn LinkRow(
    state: AppState,
    id: String,
    name: String,
    depth: usize,
    /// The shelf whose member list renders this row — the tree's own id inside
    /// an expanded branch, `None` in the flat section. See
    /// `crate::features::library::list::ListRow` for why a row carries it.
    parent: Option<String>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let menu = use_context::<LibraryMenuHost>().expect("the library page provides the menu");
    let drag = use_context::<DragController>().expect("the library page installs the drag session");
    let open_id = id.clone();
    let menu_id = id.clone();
    let gestures = use_shelf_item(
        state,
        Some(drag),
        Some(menu),
        ShelfItemPolicy {
            id: id.clone(),
            label: Signal::stored(name.clone()),
            draggable: Signal::derive(|| true),
            open: Callback::new(move |_| document::open_row(state, open_id.clone())),
            menu_target: Callback::new(move |_| MenuTarget::Book {
                id: menu_id.clone(),
                missing: false,
            }),
            container: parent.clone(),
        },
    );
    let is_selected = gestures.is_selected;
    let pressing = gestures.pressing;
    let aria_label = gestures.aria_label;
    let on_down = Rc::clone(&gestures.on_pointerdown);
    let on_move = Rc::clone(&gestures.on_pointermove);
    let on_up = Rc::clone(&gestures.on_pointerup);
    let on_cancel = Rc::clone(&gestures.on_pointercancel);
    let on_click = Rc::clone(&gestures.on_click);
    let on_context = Rc::clone(&gestures.on_contextmenu);
    let on_key = Rc::clone(&gestures.on_keydown);
    let aria_pressed = gestures.aria_pressed;

    let dom_id = crate::features::library::dnd::target::row_dom_id(DropTargetKind::Book, &id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Book, id.clone()),
        dom_id: dom_id.clone(),
        shelf: parent,
    });

    let reveal_id = id.clone();
    let held_id = id.clone();
    let over_id = id.clone();
    let after_id = id.clone();
    let fold_id = id.clone();
    let remove_id = id.clone();
    let indent = row_indent(depth);
    let tooltip = name.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };

    view! {
        <div
            id=dom_id
            class="library-row book-link"
            style=indent
            class=("row-reveal", move || {
                state.library.reveal.with(|at| {
                    at.as_ref().is_some_and(|(id, _)| id == reveal_id.as_str())
                })
            })
            class=("row-drop-before", move || drag.inserts_before(&over_id))
            class=("row-drop-after", move || drag.inserts_after(&after_id))
            class=("row-fold-here", move || drag.folds_with(&fold_id))
            class=("library-row-selected", move || is_selected.get())
            class=("library-row-pressing", move || pressing.get())
            class=("library-row-dragging", move || drag.holds(&held_id))
            role="button"
            tabindex="0"
            aria-label=move || aria_label.get()
            aria-pressed=move || aria_pressed.get()
            on:pointerdown=move |ev| (on_down)(&ev)
            on:pointermove=move |ev| (on_move)(&ev)
            on:pointerup=move |ev| (on_up)(&ev)
            on:pointercancel=move |ev| (on_cancel)(&ev)
            on:click=move |ev: leptos::ev::MouseEvent| (on_click)(&ev)
            on:contextmenu=move |ev: leptos::ev::MouseEvent| (on_context)(&ev)
            on:keydown=move |ev: leptos::ev::KeyboardEvent| (on_key)(&ev)
        >
            <span class="library-row-ext" title="A pointer at a book, not a copy of one">
                <Icon name=IconName::Link size=12 />
            </span>
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=tooltip.clone()>
                    {name.clone()}
                </span>
                <span class="block truncate text-xs text-muted">{LINK_LINE}</span>
            </span>
            <button
                type="button"
                class="library-row-remove"
                title="Remove this link"
                aria-label=move || format!("Remove the link to {}", name.clone())
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
