//! The list: one row per book and one row per shelf — the level as a tree that
//! unfolds in place, for a library the reader is scanning rather than browsing.
//!
//! Same books, same order, same drag rules and the same three gestures as the
//! grid — the only thing that changes is the shape of a row, which is why the
//! order arrives from the same context signal rather than being derived a second
//! time here. A reader who switched to the denser layout has not thereby lost the
//! way in, the way out, or the hold that starts a selection.
//!
//! Shelves are rows here now, and were not before: the shelf tile only ever
//! rendered in the grid, so the dense layout was the books and the way in. A
//! shelf row unfolds — the shelves filed in it and its own books indent under it,
//! as deep as the forest goes — while the way in stays a separate gesture: the
//! row's Open drills the breadcrumb route, and the row itself only unfolds.
//! Splitting the two is the disclosure's whole contract, because unfolding is a
//! way of LOOKING and must never move the reader: a tree that navigated on expand
//! could not be scanned without being travelled. Which shelves are unfolded is
//! this component's own memory and is never persisted — the tree is a way of
//! looking at the library, and the forest the library stores is the same one
//! whichever layout is showing it.
//!
//! A search divides the tree the way the page divides it: the doors narrow by
//! name, with the same rule `crate::features::library::content` filters the
//! grid's folders with, and the matches themselves are the flat list's alone —
//! an unfolded shelf withholds its members while a query is open, because the
//! root search already lists every match in the library and the same book twice
//! at two indents is one book too many.
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
//! ## The tree is a component, not a layout
//!
//! [`ShelfTree`] is the whole recipe — which root to walk and how dense to draw —
//! as a plain prop bag, and nothing under it reads a context the reader's sidebar
//! could not provide (the right-click's host is asked for, not expected). The
//! sidebar's shelf tab mounts the same tree with `dense`: file-name rows whose
//! format chip stands in for the cover art, because at that width the kind of
//! thing a row is earns the pixels the art would cost. When it gets there, the
//! row's own gestures lift behind an `interactive` prop the same way.

use std::collections::HashSet;
use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::query;
use library_core::shelf::{ALL_SHELF, Shelf, children_of};
use library_core::sort;
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
use crate::features::library::folder_card::summary;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::{enter_selection, payload_for, toggle_selected};
use crate::services::document;
use crate::state::AppState;

/// One level of the shelf tree, expanded in place.
///
/// A plain prop bag on purpose: the reader sidebar's shelf tab will mount the
/// same tree inside its own panel and get the same walk with file-name rows and
/// format chips, with nothing this page provides.
#[derive(Clone, Default)]
pub struct ShelfTree {
    /// Root to walk. `None` follows the level the page is on: the roots of the
    /// whole library at the top of it, and the shelves filed inside a shelf once
    /// the breadcrumb has drilled — the list is a view OF a level, the same level
    /// the grid shows, and the disclosure is how the reader goes deeper without
    /// leaving it. A mount that wants a fixed subtree names its root.
    pub root: Option<String>,
    /// Sidebar density: no counts, no covers-as-art — the format chip and the
    /// file's own name, because at that width the art is the row's whole budget.
    pub dense: bool,
}

/// What every row under a [`ListView`] shares: which shelves the reader has
/// unfolded, and how dense the tree is being drawn.
#[derive(Clone, Copy)]
struct TreeCtx {
    expanded: RwSignal<HashSet<String>>,
    dense: bool,
}

/// The tree's indent scale: the row's own padding at the level, plus one step per
/// depth — enough to read the shape of the forest at a glance, not enough to run
/// a deep row's title out of room.
fn row_indent(depth: usize) -> String {
    format!("padding-left:{}rem", 0.75 + depth as f32 * 0.9)
}

#[component]
pub(crate) fn ListView(state: AppState, #[prop(optional)] tree: ShelfTree) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));
    let expanded: RwSignal<HashSet<String>> = RwSignal::new(HashSet::new());
    provide_context(TreeCtx {
        expanded,
        dense: tree.dense,
    });

    // The shelves the tree's top level lists: the prop's root when a mount
    // pinned one, else the level the page is on — the same level the grid's
    // folders come from. An open query narrows the doors by name, with the same
    // rule `crate::features::library::content::visible_folders` applies to the
    // grid's; the books a search keeps arrive flat in `order`, and the rows
    // below withhold their members while it is open, so a match is listed once.
    let roots = Signal::derive(move || {
        let at = state.library.shelf.get();
        let terms = state.library.query.get();
        let parent = match &tree.root {
            Some(root) => Some(root.clone()),
            None => (at != ALL_SHELF).then_some(at),
        };
        state.library.shelves.with(|shelves| {
            children_of(shelves, parent.as_deref())
                .into_iter()
                .filter(|s| s.id != ALL_SHELF)
                .filter(|s| query::matches_terms(&s.name, &terms))
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    view! {
        <div
            class="library-list divide-y divide-line rounded-xl border border-line"
            class=("library-list-selecting", move || state.library.selecting.get())
        >
            <For each=move || roots.get() key=|s| s.id.clone() let:shelf>
                <TreeRow state=state shelf=shelf depth=0 crop=crop />
            </For>
            <For each=move || order.0.get() key=|b| b.id.clone() let:book>
                <ListRow state=state book=book crop=crop depth=0 />
            </For>
            // The grid ends in an add card, so the list ends in an add row: the two
            // layouts are the same library, and a reader who switched to the denser
            // one has not thereby lost the way in.
            <AddRow state=state />
        </div>
    }
}

/// A shelf as a row, and — once unfolded — everything inside it: the shelves
/// filed in it as rows of their own one indent deeper, and its member books as
/// the same rows the level's books get. Recursive because the forest is.
#[component]
fn TreeRow(state: AppState, shelf: Shelf, depth: usize, crop: Signal<bool>) -> impl IntoView {
    let ctx = use_context::<TreeCtx>().expect("the list provides the tree context");
    // The sidebar will mount this tree with no library page under it, and the
    // right-click is the one gesture that needs the page's host — so the host is
    // asked for rather than expected, and a tree without one simply has no menu
    // to offer.
    let menu = use_context::<LibraryMenuHost>();

    // The prop is the shelf the `For` keyed this row on, and a keyed row is not
    // re-created when the shelf's CONTENTS change — a book filed into it, a
    // rename, a shelf nested inside. So everything that can move is read back out
    // of the state by id and the prop supplies the identity: the rule the grid's
    // folder card follows (see `crate::features::library::folder_card`).
    let id = shelf.id.clone();

    // The doors under this row, narrowed by an open query the same way the top
    // level is: a search that hid the matching shelves but kept showing the ones
    // between them would be a filter of the leaves and not of the tree.
    let kids_id = id.clone();
    let kids = Signal::derive(move || {
        let terms = state.library.query.get();
        state.library.shelves.with(|shelves| {
            children_of(shelves, Some(kids_id.as_str()))
                .into_iter()
                .filter(|s| query::matches_terms(&s.name, &terms))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let members_id = id.clone();
    let members = Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == members_id)
                .map(|s| s.books.clone())
                .unwrap_or_default()
        })
    });
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
    let open_id = id.clone();
    let open = Signal::derive(move || ctx.expanded.with(|set| set.contains(&open_id)));

    // A search lists its matches flat — the level's `order` is the whole
    // library's when a query is open — so an unfolded shelf withholds its
    // members while the search is on: the doors stay, to show WHERE the matches
    // live, and the matches themselves are the flat list's alone.
    let books = member_books(state, members);
    let searching = Signal::derive(move || state.library.query.with(|q| query::is_active(q)));
    let shown_books = Signal::derive(move || {
        if searching.get() {
            Vec::new()
        } else {
            books.get()
        }
    });

    let toggle_id = id.clone();
    let toggle = Callback::new(move |_| {
        let at = toggle_id.clone();
        ctx.expanded.update(|set| {
            if !set.remove(&at) {
                set.insert(at);
            }
        });
    });

    let nav_id = id.clone();
    let context_id = id;
    let indent = row_indent(depth);

    view! {
        <>
            <div
                class="library-row library-row-shelf"
                style=indent
                role="button"
                tabindex="0"
                aria-expanded=move || open.get().to_string()
                on:click=move |_| toggle.run(())
                on:contextmenu=move |ev: leptos::ev::MouseEvent| {
                    ev.prevent_default();
                    ev.stop_propagation();
                    let Some(menu) = menu else {
                        return;
                    };
                    // The shelf's own menu — the same one the grid's folder card
                    // asks — so a shelf can be named and taken apart from either
                    // density. `watched` is read at the ask rather than at the
                    // mount: a rescan can start or stop watching a folder between
                    // the two, and the menu's rows answer to now.
                    let watched = state.library.shelves.with_untracked(|shelves| {
                        shelves
                            .iter()
                            .find(|s| s.id == context_id)
                            .is_some_and(|s| {
                                s.kind.folder_id().is_some_and(|folder_id| {
                                    state.library.folders.with_untracked(|folders| {
                                        folders.iter().any(|f| f.id == folder_id && f.opts.watch)
                                    })
                                })
                            })
                    });
                    menu.ask(
                        ev.client_x() as f64,
                        ev.client_y() as f64,
                        MenuTarget::Folder {
                            id: context_id.clone(),
                            watched,
                        },
                    );
                }
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    // Enter unfolds the way a click does, and Space is the key a
                    // disclosure owns — prevented, so the page does not scroll on
                    // the row that meant to open.
                    if ev.key() == "Enter" || ev.key() == " " {
                        ev.prevent_default();
                        toggle.run(());
                    }
                }
            >
                {move || {
                    // Closed points at what the row would open; open points down
                    // at what it is showing. The chevron is the disclosure's
                    // whole picture.
                    let glyph = if open.get() {
                        IconName::ChevronDown
                    } else {
                        IconName::Next
                    };
                    view! { <Icon name=glyph size=13 class="shrink-0 text-muted" /> }
                }}
                <Icon name=IconName::Outline size=14 class="shrink-0 text-muted" />
                <span
                    class="min-w-0 flex-1 truncate text-sm font-semibold text-ink"
                    title=move || name.get()
                >
                    {move || name.get()}
                </span>
                <Show when=move || !ctx.dense>
                    <span class="shrink-0 text-xs text-muted">
                        {move || summary((members.with(|m| m.len()), kids.get().len()))}
                    </span>
                </Show>
                // Open drills the breadcrumb route; the row itself only unfolds.
                // Two gestures on one shelf because they are two questions:
                // "show me inside it" and "take me to it".
                <button
                    class="library-row-remove"
                    type="button"
                    title="Open shelf"
                    aria-label=move || format!("Open the {} shelf", name.get())
                    on:click=move |ev: leptos::ev::MouseEvent| {
                        ev.stop_propagation();
                        state.library.shelf.set(nav_id.clone());
                    }
                >
                    <Icon name=IconName::Open size=12 />
                </button>
            </div>
            <Show when=move || open.get()>
                <For each=move || kids.get() key=|s| s.id.clone() let:child>
                    // Erased through `AnyView`, the way the grid's recursive
                    // folder plate is: a recursive component whose children
                    // named its own opaque return type would be a type that
                    // never resolves.
                    {view! { <TreeRow state=state shelf=child depth=depth + 1 crop=crop /> }
                        .into_any()}
                </For>
                <For each=move || shown_books.get() key=|b| b.id.clone() let:book>
                    <ListRow state=state book=book crop=crop depth=depth + 1 />
                </For>
            </Show>
        </>
    }
}

/// The books on a shelf's member list, in the order the page shows books: the
/// shelf's own order is the base and the view's sort rides over it — the same two
/// steps `crate::features::library::content::visible` runs for a drilled-into
/// shelf, so an unfolded row and the page it mirrors cannot disagree about what
/// comes first.
fn member_books(state: AppState, members: Signal<Vec<String>>) -> Signal<Vec<Book>> {
    Signal::derive(move || {
        let ids = members.get();
        let view = state.library.view.get();
        state.library.books.with(|books| {
            let mut list: Vec<Book> = ids
                .iter()
                .filter_map(|id| books.iter().find(|b| &b.id == id).cloned())
                .collect();
            sort::sort_books(&mut list, view.sort, view.sort_asc);
            list
        })
    })
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
fn ListRow(state: AppState, book: Book, crop: Signal<bool>, depth: usize) -> impl IntoView {
    let ctx = use_context::<TreeCtx>().expect("the list provides the tree context");
    // The dense variant's whole difference, decided once at the mount: at
    // sidebar width the format IS the cover — the kind of thing the row is, in
    // the footprint the art would have had — and the one line of prose under the
    // title is a line the sidebar does not have.
    let dense = ctx.dense;

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
    let ext = book.format.label();
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
    let indent = row_indent(depth);

    view! {
        <div
            id=dom_id
            class="library-row"
            style=indent
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
            {if dense {
                // The dense variant's cover: the extension chip, in the art's own
                // footprint, so a sidebar row still leads with what the file IS.
                // No selection check rides it — the dense tree is a browser, not
                // a picker, and the row's own tint is the whole of its state.
                view! { <span class="library-row-ext">{ext}</span> }.into_any()
            } else {
                view! {
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
                }
                    .into_any()
            }}
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=row_tooltip.clone()>
                    {row_title}
                </span>
                {if dense {
                    None
                } else {
                    Some(
                        view! {
                            <span class="block truncate text-xs text-muted" title=path_hint.clone()>
                                {author}
                            </span>
                        },
                    )
                }}
            </span>
            {if dense {
                // The chip's whole job is done by the head-of-row extension.
                None
            } else {
                chip.map(|label| view! { <span class="library-row-format">{label}</span> })
            }}
            {if dense {
                None
            } else {
                percent.map(|p| {
                    view! {
                        <span class="shrink-0 text-xs tabular-nums text-muted">{p}</span>
                    }
                })
            }}
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
