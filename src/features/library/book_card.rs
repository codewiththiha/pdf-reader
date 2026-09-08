//! One book on the shelf: cover, spine, title, resume hint, and the two things a
//! library needs that a recents list never did — a drag handle, and a way back
//! when the address the book points at dies.
//!
//! Three gestures share the card. A click opens the book, or toggles it once the
//! shelf is in multi-select. A long-press starts that multi-select with this book
//! already in it. A right-click asks for the removal receipt. All three are the
//! gestures a highlighted stroke on a page already answers to, from the same
//! primitive with the same tuning, so holding a book and holding a highlight are
//! one idea rather than two that happen to feel alike.
//!
//! Feature-local on purpose: it understands [`Book`], cover persistence, the open
//! flow and the drag payload, and none of those belong in a primitive.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use reader_core::format::Format;

use crate::components::primitives::interactions::long_press::{
    LongPressOptions, SELECT_PRESS_MS, SELECT_SLOP_PX, use_long_press,
};
use crate::features::library::drag::{self, DropTarget, ShelfOrder};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::{enter_selection, toggle_selected};
use crate::services::document;
use crate::services::library::{move_to_shelf, relink_dialog};
use crate::state::AppState;
use crate::state::reader::DEFAULT_PAGE_ASPECT;

/// One book on the shelf.
///
/// `crop` is the view's cover treatment, passed in rather than read here so the
/// whole grid switches on one signal instead of every card subscribing to the
/// view separately.
#[component]
pub(crate) fn BookCard(state: AppState, book: Book, crop: Signal<bool>) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    // Selection is a page-wide mode, so every card asks the same two signals rather
    // than being told about itself.
    let selecting = state.library.selecting;
    let selected_set = state.library.selected;
    let selected_id = book.id.clone();
    let is_selected = Signal::derive(move || selected_set.with(|s| s.contains(&selected_id)));

    // Owned copies so each closure below captures its own value: the card renders
    // a dozen closures that all outlive this function's frame.
    let id = book.id.clone();
    let path = book.path().to_string();
    let title = book.title();
    let author = book.author();
    let missing = book.missing;
    let progress = book.progress();
    let page_line = if book.num_pages > 0 {
        format!("Page {} of {}", book.page, book.num_pages)
    } else {
        format!("Page {}", book.page)
    };
    // A chip for the formats that are NOT the default. A shelf of PDFs is a shelf
    // of covers and a chip on every one of them would be noise; a Markdown file has
    // no cover to be recognised by, so the one thing that says what a reader is
    // about to open is worth printing.
    let chip = (book.format != Format::Pdf).then(|| book.format.label().to_string());
    // Where the book lives, on the line that has room for it and nothing better to
    // say. A card whose title came from the document gives the reader no way to
    // tell two books called "Report" apart; the address does.
    let path_hint = book.path().to_string();

    // Aspect ratio (width / height) for the cover box, so a landscape plate stays
    // landscape on the shelf. Clamped so a pathological page cannot break the
    // grid; falls back to 3:4 portrait.
    let cover_path = path.clone();
    let aspect = move || {
        state.library.covers.with(|covers| {
            covers
                .get(&cover_path)
                .map(|c| {
                    if c.width > 0.0 && c.height > 0.0 {
                        (c.width / c.height).clamp(0.55, 1.8)
                    } else {
                        DEFAULT_PAGE_ASPECT
                    }
                })
                .unwrap_or(DEFAULT_PAGE_ASPECT)
        })
    };

    // The hold that starts a selection. Disabled once one is running: inside
    // selection mode a tap already toggles, and a second gesture per card would be
    // a second way to do the thing a tap now does.
    let press_id = id.clone();
    let lp = use_long_press(LongPressOptions {
        press_ms: SELECT_PRESS_MS,
        slop_px: SELECT_SLOP_PX,
        // Capture so the hold survives drifting off a narrow spine, and so starting
        // a drag arrives here as `pointercancel` and cancels the press instead of
        // letting it complete behind the drag.
        capture_pointer: true,
        enabled: Signal::derive(move || !selecting.get_untracked()),
        on_press: Callback::new(move |_| enter_selection(state, &press_id)),
    });

    let click_path = path.clone();
    let click_id = id.clone();
    let on_click = move |ev: leptos::ev::MouseEvent| {
        // The click a completed hold generates is the gesture's exhaust, not an
        // intention to open the book.
        if (lp.swallow_click)() {
            return;
        }
        if selecting.get_untracked() {
            ev.stop_propagation();
            toggle_selected(state, &click_id);
            return;
        }
        document::open_path(state, click_path.clone());
    };

    // A right-click is the shelf's answer to a stroke's remove menu: it asks, and
    // the sheet that answers is the same one the card's own ✕ opens. Inside
    // selection mode the same button toggles instead, because a reader who is
    // picking books out is not asking to remove one of them.
    let context_id = id.clone();
    let on_context = move |ev: leptos::ev::MouseEvent| {
        ev.prevent_default();
        if (lp.swallow_context)() {
            return;
        }
        ev.stop_propagation();
        if selecting.get_untracked() {
            toggle_selected(state, &context_id);
            return;
        }
        remove_sheet.ask(&context_id);
    };

    let key_path = path.clone();
    let key_id = id.clone();
    let select_key_id = id.clone();
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() != "Enter" {
            return;
        }
        // A keyboard has no hold to make, so it gets the gesture's two halves as
        // two keys: Shift+Enter enters selection the way a long-press does, and
        // once inside, Enter toggles instead of opening.
        if ev.shift_key() && !selecting.get_untracked() {
            ev.prevent_default();
            enter_selection(state, &select_key_id);
            return;
        }
        if selecting.get_untracked() {
            toggle_selected(state, &key_id);
            return;
        }
        document::open_path(state, key_path.clone());
    };

    // A removal asks first. The card does not know what a removal costs — the
    // resume point, the placements, the highlights, the app's own copy — and the
    // sheet that does is one context away.
    let remove_id = id.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };
    let relink_id = id.clone();
    let relink = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        relink_dialog(state, relink_id.clone());
    };

    let dom_id = format!("book-{}", id);
    let reveal_id = id.clone();
    let aria_id = id.clone();
    let drag_id = id.clone();
    let hover_id = id.clone();
    let leave_id = id.clone();
    let over_id = id.clone();
    let drop_id = id.clone();
    let cover_title = title.clone();
    let alt_title = title.clone();
    let meta_title = title.clone();
    let card_title = title.clone();
    let author_line = author.clone();
    let aria_title = title.clone();
    let alt_path = path.clone();
    let progress_str = progress.map(|p| format!("{:.0}%", p * 100.0));

    view! {
        <div
            id=dom_id
            class="book group"
            class=("book-reveal", move || {
                state.library.reveal.with(|at| {
                    at.as_ref()
                        .is_some_and(|(id, _)| id == reveal_id.as_str())
                })
            })
            class=("book-drop-before", move || {
                drop_target
                    .0
                    .with(|t| t.as_deref() == Some(over_id.as_str()))
            })
            class=("book-missing", missing)
            class=("book-selected", move || is_selected.get())
            class=("book-pressing", move || lp.pressing.get())
            role="button"
            tabindex="0"
            // Dragging files one card on a shelf; while a set is selected the
            // pointer is choosing, not filing.
            draggable=move || if selecting.get() { "false" } else { "true" }
            aria-label=move || {
                if selecting.get() {
                    format!("Select {aria_title}")
                } else {
                    format!("Open {aria_title}")
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
            on:pointerdown=move |ev| {
                // Only the primary button starts a hold — the right one owns the
                // receipt.
                if ev.button() != 0 {
                    return;
                }
                (lp.on_pointerdown)(&ev);
            }
            on:pointermove=move |ev| (lp.on_pointermove)(&ev)
            on:pointerup=move |ev| (lp.on_pointerup)(&ev)
            on:pointercancel=move |ev| (lp.on_pointercancel)(&ev)
            on:click=on_click
            on:contextmenu=on_context
            on:keydown=on_key
            on:dragstart=move |ev| drag::begin(&ev, &drag_id)
            on:dragend=move |_| drop_target.0.set(None)
            on:dragover=move |ev| {
                if drag::accept(&ev) {
                    drop_target.0.set(Some(hover_id.clone()));
                }
            }
            on:dragleave=move |_| {
                // Only clear our own line: the card being entered has already
                // written itself, and undoing that here would leave no marker at
                // all for the frame between the two events.
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
                // Dropping on a card means "put it here", which is an index in
                // the order the reader is looking at — and only means anything
                // while that order is the manual one.
                let manual = state.library.view.with_untracked(|v| v.drag_reorders());
                let index = manual.then(|| {
                    order
                        .0
                        .with_untracked(|list| {
                            list.iter().position(|b| b.id == drop_id)
                        })
                        .unwrap_or(0)
                });
                let shelf = state.library.shelf.get_untracked();
                move_to_shelf(state, dragged, Some(shelf.clone()), shelf, index);
            }
        >
            <div
                class="book-cover"
                class=("book-cover-crop", move || crop.get())
                style:aspect-ratio=move || {
                    if crop.get() {
                        // A4 portrait, so a shelf of mixed scans and exports
                        // reads as one row of identical spines.
                        "210 / 297".to_string()
                    } else {
                        format!("{:.5} / 1", aspect())
                    }
                }
            >
                // Fore-edge: stacked page sheets peeking past the right side.
                <div class="book-pages"></div>
                // The set membership, printed on the cover while the shelf is
                // choosing: an outline alone asks the reader to remember which
                // cards they have already tapped.
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
                    match state
                        .library
                        .covers
                        .with(|covers| covers.get(&alt_path).cloned())
                    {
                        Some(c) => {
                            view! {
                                <img
                                    class="book-cover-img"
                                    src=c.data_url.clone()
                                    alt=alt_title.clone()
                                    loading="lazy"
                                />
                            }
                                .into_any()
                        }
                        None => {
                            view! {
                                <div class="book-cover-fallback">
                                    <span>{cover_title.clone()}</span>
                                </div>
                            }
                                .into_any()
                        }
                    }
                }}
                {missing.then(|| {
                    view! {
                        <span class="book-missing-badge" title="This file is not where the library left it">
                            <Icon name=IconName::Close size=11 />
                        </span>
                    }
                })}
                {chip.map(|label| view! { <span class="book-format">{label}</span> })}
            </div>

            <div class="book-meta">
                <span class="book-title" title=meta_title.clone()>{card_title.clone()}</span>
                {match author_line {
                    Some(author) => {
                        let author_title = author.clone();
                        view! { <span class="book-author" title=author_title>{author}</span> }
                            .into_any()
                    }
                    None => {
                        view! {
                            <span class="book-page" title=path_hint.clone()>{page_line.clone()}</span>
                        }
                            .into_any()
                    }
                }}
                {progress_str.map(|p| {
                    view! {
                        <span class="book-progress">
                            <span class="book-progress-fill" style:width=p></span>
                        </span>
                    }
                })}
            </div>

            {missing.then(|| {
                view! {
                    <button
                        class="book-relink"
                        type="button"
                        title="Find this book again"
                        aria-label="Find this book again"
                        on:click=relink
                    >
                        <Icon name=IconName::Open size=11 />
                    </button>
                }
            })}
            <button
                class="book-remove"
                type="button"
                title="Remove from library"
                aria-label="Remove from library"
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
