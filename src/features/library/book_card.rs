//! One book on the shelf: a cover in a frame, a title, and the two things a
//! library needs that a recents list never did — a drag handle, and a way back
//! when the address the book points at dies.
//!
//! The cover sits in a frame (`.book-cover-wrap` in `styles/library.css`) rather
//! than carrying its own shadow, spine gradient and fore-edge. Three reasons, and
//! the first is the one that decided it: the reading-progress bar belongs at the
//! bottom of the ART, and a bar under the title is a bar the eye has to leave the
//! cover to find. A frame that clips gives the bar a box to be flush with, and
//! gives the lift, the ring and the selection outline one element to be painted on
//! instead of four. The second is that the skeuomorphic spine and fore-edge were
//! two pseudo-elements spent on a decoration, and the third is that a shelf of
//! mixed page sizes reads calmer as one row of frames than as one row of volumes.
//!
//! Three gestures share the card and one wrapper decides between them — see
//! `crate::components::primitives::interactions::draggable_item`. A tap opens the
//! book, or toggles it once the shelf is in multi-select. A hold starts that
//! multi-select with this book already in it. A right-click asks for the removal
//! receipt. The hold is the same gesture, at the same tuning, that a highlighted
//! stroke on a page answers to, so holding a book and holding a highlight are one
//! idea rather than two that happen to feel alike.
//!
//! The movement is the one the card does not own. It hands the press to
//! `crate::features::library::dnd` and takes its two visible halves back from
//! there: a fade while it is one of the items being held, and either an insertion
//! line or a fold's ring while it is the thing under the pointer. Registering as a
//! target is one call and leaving is the card's own unmount, so a shelf the reader
//! scrolled or drilled through carries no targets that are not on screen.
//!
//! Feature-local on purpose: it understands [`Book`], cover persistence, the open
//! flow and what a press on this card picks up, and none of those belong in a
//! primitive.

use std::rc::Rc;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;

use crate::components::primitives::interactions::draggable_item::{
    DRAG_THRESHOLD_PX, DraggableItemOptions, use_draggable_item,
};
use crate::components::primitives::interactions::long_press::SELECT_PRESS_MS;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::document;
use crate::services::library::relink_dialog;
use crate::state::AppState;
use crate::state::reader::DEFAULT_PAGE_ASPECT;

/// One book on the shelf.
///
/// `crop` is the view's cover treatment, passed in rather than read here so the
/// whole grid switches on one signal instead of every card subscribing to the
/// view separately.
#[component]
pub(crate) fn BookCard(state: AppState, book: Book, crop: Signal<bool>) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");
    let drag = use_context::<DragController>().expect("the library page installs the drag session");

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

    // One wrapper, three gestures, and the mode is decided once per press: a
    // movement is a drag, a hold is a selection, and a release that was neither
    // is the open.
    let press_id = id.clone();
    let tap_id = id.clone();
    let tap_path = path.clone();
    let lift_id = id.clone();
    let item = use_draggable_item(DraggableItemOptions {
        press_ms: SELECT_PRESS_MS,
        drag_threshold_px: DRAG_THRESHOLD_PX,
        // A movement is always a drag here. It used to be off while a set was
        // selected, on the reasoning that the pointer was choosing rather than
        // filing — which is the reasoning that made a selection undraggable, and
        // lifting one of three held books is the whole of what a multi-drag is.
        draggable: Signal::derive(|| true),
        // A hold inside a selection would be a second way to do the thing a tap
        // now does.
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
            // What the press picks up: the whole set when this card is already in
            // it, and this card alone when it is not.
            drag.begin(payload_for(state, &lift_id), x, y);
        }),
        // The session owns the move. Its listeners are on the window rather than
        // on this element, so a card that unmounts mid-drag — a focus rescan
        // filing this book somewhere else while the reader is holding it — leaves
        // a drag that can still end.
        on_drag_move: Callback::new(move |_| {}),
        // Both halves end the session and the first one there wins: this release
        // bubbles ahead of the window's own.
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

    // A target as well as a payload: a drop here lands the held items at this
    // book's place in the level, and a rest here while holding two or more offers
    // to fold them into a new shelf beside it. Registered for the life of the
    // card, which is the life of its box on screen.
    let dom_id = format!("book-{}", id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Book, id.clone()),
        dom_id: dom_id.clone(),
    });

    // A right-click is the shelf's answer to a stroke's remove menu: it asks, and
    // the sheet that answers is the same one the card's own ✕ opens. Inside
    // selection mode the same button toggles instead, because a reader who is
    // picking books out is not asking to remove one of them.
    let context_id = id.clone();
    let on_context = move |ev: leptos::ev::MouseEvent| {
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
    };

    let key_path = path.clone();
    let key_id = id.clone();
    let select_key_id = id.clone();
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() != "Enter" {
            return;
        }
        // A keyboard has no hold to make, so it gets the gesture's two halves as
        // two keys: Shift+Enter enters selection the way a hold does, and once
        // inside, Enter toggles instead of opening.
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

    let reveal_id = id.clone();
    let aria_id = id.clone();
    let held_id = id.clone();
    let over_id = id.clone();
    let fold_id = id.clone();
    let cover_title = title.clone();
    let alt_title = title.clone();
    let meta_title = title.clone();
    let card_title = title.clone();
    let author_line = author.clone();
    let aria_title = title.clone();
    let alt_path = path.clone();
    let progress_width = progress.map(|p| format!("{:.0}%", p * 100.0));
    let progress_now = progress.map(|p| format!("{:.0}", (p * 100.0).round()));

    view! {
        <div
            id=dom_id
            class="book-card"
            class=("book-reveal", move || {
                state.library.reveal.with(|at| {
                    at.as_ref()
                        .is_some_and(|(id, _)| id == reveal_id.as_str())
                })
            })
            class=("book-drop-before", move || drag.inserts_before(&over_id))
            class=("book-fold-here", move || drag.folds_with(&fold_id))
            class=("book-missing", missing)
            class=("book-selected", move || is_selected.get())
            class=("book-pressing", move || pressing.get())
            // Every held card fades, not only the one the press began on: the set
            // the reader picked up has to stay readable as a set while the pointer
            // carries it, and only the session knows which cards that is.
            class=("book-dragging", move || drag.holds(&held_id))
            role="button"
            tabindex="0"
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
            on:contextmenu=on_context
            on:keydown=on_key
        >
            <div class="book-cover-wrap">
                <div
                    class="book-cover"
                    class=("book-cover-crop", move || crop.get())
                    style:aspect-ratio=move || {
                        if crop.get() {
                            // A4 portrait, so a shelf of mixed scans and exports
                            // reads as one row of identical frames.
                            "210 / 297".to_string()
                        } else {
                            format!("{:.5} / 1", aspect())
                        }
                    }
                >
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
                                    // Not draggable, and the reason is the whole
                                    // of this card's gesture: an image is natively
                                    // draggable, so a press on the cover would hand
                                    // the pointer to the engine's own drag, which
                                    // is the drag this shelf no longer uses and the
                                    // one that used to swallow the release.
                                    <img
                                        class="book-cover-img"
                                        src=c.data_url.clone()
                                        alt=alt_title.clone()
                                        loading="lazy"
                                        draggable="false"
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
                </div>
                // Reading progress, flush with the bottom of the art rather than
                // under the title: it is a fact about the cover the reader is
                // looking at, and the frame is what makes it flush with anything.
                {progress_width.map(|width| {
                    let now = progress_now.clone().unwrap_or_default();
                    view! {
                        <div
                            class="book-progress-track"
                            role="progressbar"
                            aria-label="Reading progress"
                            aria-valuemin="0"
                            aria-valuemax="100"
                            aria-valuenow=now
                        >
                            <div class="book-progress-fill" style:width=width></div>
                        </div>
                    }
                })}
            </div>

            <div class="book-info">
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
