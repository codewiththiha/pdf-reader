//! The list: one row per book, for a library the reader is scanning rather than
//! browsing.
//!
//! Same books, same order, same drag rules and the same three gestures as the grid
//! — the only thing that changes is the shape of a row, which is why the order
//! arrives from the same context signal rather than being derived a second time
//! here. A reader who switched to the denser layout has not thereby lost the way
//! in, the way out, or the hold that starts a selection.
//!
//! A row shows the author when the book has one and the resume point when it does
//! not: at this density there is room for one line of prose and the reader gets to
//! choose which by opening the book.
//!
//! Folders are NOT here, and were not before either: the shelf tile only ever
//! rendered in the grid, so the dense layout has always been the books and the way
//! in. The breadcrumb is the way around a folder from here, and a reader who wants
//! to see the folders is a reader who wants the grid.

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
use crate::features::library::drag::{self, DropTarget, ShelfOrder};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::{enter_selection, toggle_selected};
use crate::services::document;
use crate::services::library::move_to_shelf;
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
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

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
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // While a set is selected the pointer is choosing, not filing.
        draggable: Signal::derive(move || !selecting.get()),
        selectable: Signal::derive(move || !selecting.get()),
        on_tap: Callback::new(move |_| {
            if selecting.get_untracked() {
                toggle_selected(state, &tap_id);
                return;
            }
            document::open_path(state, tap_path.clone());
        }),
        on_long_press: Callback::new(move |_| enter_selection(state, &press_id)),
        on_drag_start: Callback::new(move |_| drop_target.0.set(None)),
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

    let context_id = id.clone();
    let key_id = id.clone();
    let aria_id = id.clone();
    let select_key_id = id.clone();
    let dom_id = format!("book-{}", id);
    let reveal_id = id.clone();
    let remove_id = id.clone();
    let drag_id = id.clone();
    let over_id = id.clone();
    let hover_id = id.clone();
    let leave_id = id.clone();
    let drop_id = id.clone();
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
            class=("row-drop-before", move || {
                drop_target
                    .0
                    .with(|t| t.as_deref() == Some(over_id.as_str()))
            })
            class=("row-missing", missing)
            class=("library-row-selected", move || is_selected.get())
            class=("library-row-pressing", move || pressing.get())
            class=("library-row-dragging", move || dragging.get())
            role="button"
            tabindex="0"
            draggable=move || if selecting.get() { "false" } else { "true" }
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
            on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                ev.prevent_default();
                if (swallow_context)() {
                    return;
                }
                ev.stop_propagation();
                if selecting.get_untracked() {
                    toggle_selected(state, &context_id);
                    return;
                }
                remove_sheet.ask(&context_id);
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
            on:dragstart=move |ev| drag::begin(&ev, &drag_id)
            on:dragend=move |_| drop_target.0.set(None)
            on:dragover=move |ev| {
                // A book only: the line this row draws is an index in a list of
                // books, and the list has no folders in it to nest anything into.
                if drag::accepts_book(&ev) {
                    drop_target.0.set(Some(hover_id.clone()));
                }
            }
            on:dragleave=move |_| drop_target.release(&leave_id)
            on:drop=move |ev| {
                ev.prevent_default();
                ev.stop_propagation();
                drop_target.0.set(None);
                let Some(dragged) = drag::dragged(&ev) else {
                    return;
                };
                let manual = state.library.view.with_untracked(|v| v.drag_reorders());
                let index = manual.then(|| {
                    order
                        .0
                        .with_untracked(|list| list.iter().position(|b| b.id == drop_id))
                        .unwrap_or(0)
                });
                let shelf = state.library.shelf.get_untracked();
                move_to_shelf(state, dragged, Some(shelf.clone()), shelf, index);
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
                                <img
                                    class="library-row-img"
                                    src=cover.data_url.clone()
                                    alt=alt_title.clone()
                                    loading="lazy"
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
