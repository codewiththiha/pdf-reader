//! The list: one row per book, for a library the reader is scanning rather than
//! browsing.
//!
//! Same books, same order, same drag rules as the grid — the only thing that
//! changes is the shape of a row, which is why the order arrives from the same
//! context signal rather than being derived a second time here. A row shows the
//! author when the book has one and the resume point when it does not: at this
//! density there is room for one line of prose and the reader gets to choose
//! which by opening the book.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::view::CoverFit;

use crate::features::library::drag::{self, DropTarget, ShelfOrder};
use crate::services::document;
use crate::services::library::{move_to_shelf, remove_book};
use crate::state::AppState;

#[component]
pub(crate) fn ListView(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));

    view! {
        <div class="library-list divide-y divide-line rounded-xl border border-line">
            <For each=move || order.0.get() key=|b| b.id.clone() let:book>
                <ListRow state=state book=book crop=crop />
            </For>
        </div>
    }
}

#[component]
fn ListRow(state: AppState, book: Book, crop: Signal<bool>) -> impl IntoView {
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");

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

    let click_path = path.clone();
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

    view! {
        <div
            class="library-row group"
            class=("row-drop-before", move || {
                drop_target
                    .0
                    .with(|t| t.as_deref() == Some(over_id.as_str()))
            })
            class=("row-missing", missing)
            role="button"
            tabindex="0"
            draggable="true"
            on:click=move |_| document::open_path(state, click_path.clone())
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if ev.key() == "Enter" {
                    document::open_path(state, path.clone());
                }
            }
            on:dragstart=move |ev| drag::begin(&ev, &drag_id)
            on:dragend=move |_| drop_target.0.set(None)
            on:dragover=move |ev| {
                if drag::accept(&ev) {
                    drop_target.0.set(Some(hover_id.clone()));
                }
            }
            on:dragleave=move |_| {
                drop_target.0.update(|at| {
                    if at.as_deref() == Some(leave_id.as_str()) {
                        *at = None;
                    }
                });
            }
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
                <span class="block truncate text-xs text-muted">{author}</span>
            </span>
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
                    remove_book(state, remove_id.clone());
                }
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
