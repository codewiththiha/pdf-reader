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
//! could only drop on the one the reader happened to be looking at. The tree's
//! rows carry one fact a card never had to: the shelf whose member list renders
//! them, so a drop inside an expanded branch lands in the branch and not in the
//! level the page is on. A shelf row is a target in its own right — its middle
//! takes a hold inside, its outer quarters reorder held folders beside it, and a
//! hold that rests on a collapsed one opens it, the courtesy every file manager's
//! tree gives a drag. And a shelf row is a LIFT as well as a landing: a hold
//! enters the selection with the shelf in it and a movement picks it up — the
//! same wiring and the same disk-bound refusal the grid's folder card wears — so
//! a folder is draggable at both densities, and a set of books and folders
//! lifts as one.
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
//! rows' gestures stand down with the page's hosts: a tree with no drag session
//! and no menu keeps the tap and the disclosure and nothing else (see
//! `crate::features::library::gestures`).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;
use library_core::query;
use library_core::shelf::{ALL_SHELF, Shelf, children_of};
use library_core::sort;
use library_core::view::CoverFit;
use reader_core::format::Format;

use crate::features::library::add_menu::AddMenu;
use crate::features::library::content::ShelfOrder;
use crate::features::library::context_menu::{LibraryMenuHost, MenuTarget};
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::folder_card::summary;
use crate::features::library::gestures::{ShelfItemPolicy, use_shelf_item};
use crate::features::library::remove_modal::RemoveSheet;
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

/// How long a hold rests on a collapsed shelf row before the tree opens it: the
/// way deeper is the way IN, and a reader carrying books should not have to put
/// them down to knock. Longer than the fold's own dwell, because opening a level
/// is a navigation the reader has to see happen before they aim into it.
const AUTO_EXPAND_MS: u64 = 650;

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
                <ListRow state=state book=book crop=crop depth=0 parent=None />
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
    // right-click and the drag are the two gestures that need the page's hosts
    // — so both are asked for rather than expected, and a tree without them
    // simply has no menu to offer, no seam to paint and nothing to lift into.
    let menu = use_context::<LibraryMenuHost>();
    let drag = use_context::<DragController>();

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

    // A shelf cut from a watched tree is the tree's to place — every scan
    // re-hangs it on the rung its `rel` names — so offering it a lift would be
    // offering a move the next rescan undoes. The grid's folder card refuses by
    // the same rule; a virtual shelf lifts freely at both densities.
    let disk_id = id.clone();
    let disk_bound = Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .find(|s| s.id == disk_id)
                .is_some_and(|s| s.is_folder())
        })
    });

    // A target at last, and the row IS the shelf: its middle takes a hold
    // inside, and its outer quarters reorder held folders beside it in the
    // level that holds them. Unregistered, a drop here fell through to the
    // level's own box and silently re-filed into the shelf the reader was
    // already standing in — the list's whole drag mess in one sentence.
    let row_dom = format!("shelf-row-{id}");
    if let Some(drag) = drag {
        drag.registry.register(DropTargetEntry {
            id: DropTargetId(DropTargetKind::Folder, id.clone()),
            dom_id: row_dom.clone(),
            shelf: None,
        });
    }

    // The tree's courtesy to a drag: a hold resting on a COLLAPSED row opens
    // it, so the way deeper is the way in and the reader never has to put the
    // hold down to knock. The timer belongs to an effect on the hover, so
    // leaving the row — or the row opening by hand — is the cancellation.
    let hover_id = id.clone();
    let hover_collapsed = Signal::derive(move || {
        drag.is_some_and(|each| each.live().get() && each.over_folder(&hover_id)) && !open.get()
    });
    let expand_id = id.clone();
    Effect::new(move |_| {
        if !hover_collapsed.get() {
            return;
        }
        let at = expand_id.clone();
        let expanded = ctx.expanded;
        let handle = set_timeout_with_handle(
            move || {
                expanded.update(|set| {
                    set.insert(at);
                });
            },
            Duration::from_millis(AUTO_EXPAND_MS),
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });

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

    // The row wears the shelf's one press contract — the same wiring the grid's
    // folder card and the book rows wear (see
    // `crate::features::library::gestures`) with the disclosure's own answer: a
    // tap unfolds, a hold enters the selection with this shelf in it, and a
    // movement lifts it unless the disk places it. The hosts arrive as the
    // Options this row asked for: the sidebar's tree mounts it with neither,
    // and a row with no session and no menu keeps its tap and nothing else.
    let target_id = id.clone();
    let gestures = use_shelf_item(
        state,
        drag,
        menu,
        ShelfItemPolicy {
            id: id.clone(),
            label: Signal::derive(move || format!("the {} shelf", name.get())),
            draggable: Signal::derive(move || !disk_bound.get()),
            open: toggle,
            menu_target: Callback::new(move |_| MenuTarget::Folder {
                id: target_id.clone(),
                // Read at the ask rather than at the mount: a rescan can start
                // or stop watching a folder between the two, and the menu's
                // rows answer to now.
                watched: shelf_is_watched(state, &target_id),
            }),
            // A folder's lift is a nesting, which writes a parent rather than
            // a membership: there is no list to lift it off.
            container: None,
        },
    );
    let is_selected = gestures.is_selected;
    let pressing = gestures.pressing;
    let aria_pressed = gestures.aria_pressed;
    let on_down = Rc::clone(&gestures.on_pointerdown);
    let on_move = Rc::clone(&gestures.on_pointermove);
    let on_up = Rc::clone(&gestures.on_pointerup);
    let on_cancel = Rc::clone(&gestures.on_pointercancel);
    let on_click = Rc::clone(&gestures.on_click);
    let on_context = Rc::clone(&gestures.on_contextmenu);
    let on_key = Rc::clone(&gestures.on_keydown);

    let nav_id = id.clone();
    let nest_id = id.clone();
    let before_id = id.clone();
    let after_id = id.clone();
    let held_id = id.clone();
    // Parked in a `StoredValue` rather than captured: the member rows are built
    // inside the unfold's `Show`, whose children closure has to stay an `Fn` —
    // a `String` owned by the rows' `move` closure would be moved out of it on
    // the first build, and a Copy handle to a scoped cell is the same fix
    // `crate::features::library::breadcrumb` uses for its folded chain.
    let members_parent: StoredValue<Option<String>, LocalStorage> =
        StoredValue::new_local(Some(id.clone()));
    let indent = row_indent(depth);

    view! {
        <>
            <div
                id=row_dom
                class="library-row library-row-shelf"
                style=indent
                // The row's three drag faces: the accent wash and inset ring
                // while the hold is going INSIDE it — the list's answer to the
                // grid's folder-drag-over — and the two sibling seams while a
                // folders-only hold is landing beside it.
                class=("row-nest-here", move || {
                    drag.is_some_and(|each| each.nests_into(&nest_id))
                })
                class=("row-drop-before", move || {
                    drag.is_some_and(|each| each.sibling_at(&before_id) == Some(false))
                })
                class=("row-drop-after", move || {
                    drag.is_some_and(|each| each.sibling_at(&after_id) == Some(true))
                })
                // The row's own three states while the reader holds or carries
                // it: the tint of the set, the press counting, and the fade of
                // everything the session is holding — the same three a book row
                // wears, because a shelf row is a row of the same list.
                class=("library-row-selected", move || is_selected.get())
                class=("library-row-pressing", move || pressing.get())
                class=("library-row-dragging", move || {
                    drag.is_some_and(|each| each.holds(&held_id))
                })
                role="button"
                tabindex="0"
                aria-expanded=move || open.get().to_string()
                aria-pressed=move || aria_pressed.get()
                on:pointerdown=move |ev| (on_down)(&ev)
                on:pointermove=move |ev| (on_move)(&ev)
                on:pointerup=move |ev| (on_up)(&ev)
                on:pointercancel=move |ev| (on_cancel)(&ev)
                // The tap is the wrapper's answer now — it unfolds the way the
                // click did — and what is left on the click is the completed
                // hold's exhaust.
                on:click=move |ev: leptos::ev::MouseEvent| (on_click)(&ev)
                // The shelf's own menu — the same one the grid's folder card
                // asks — so a shelf can be filed, nested and taken apart from
                // either density.
                on:contextmenu=move |ev: leptos::ev::MouseEvent| (on_context)(&ev)
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    // Space is the key a disclosure owns — prevented, so the
                    // page does not scroll on the row that meant to open. Enter
                    // and Shift+Enter are the shared wiring's: open (unfold)
                    // and the keyboard's hold.
                    if ev.key() == " " {
                        ev.prevent_default();
                        toggle.run(());
                        return;
                    }
                    (on_key)(&ev);
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
                    <ListRow
                        state=state
                        book=book
                        crop=crop
                        depth=depth + 1
                        parent=members_parent.get_value()
                    />
                </For>
            </Show>
        </>
    }
}

/// Whether the folder this shelf was cut from is still being watched — the one
/// fact the shelf's right-click menu carries, read when the menu is asked
/// rather than when the row mounts.
fn shelf_is_watched(state: AppState, shelf_id: &str) -> bool {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .is_some_and(|s| {
                s.kind.folder_id().is_some_and(|folder_id| {
                    state.library.folders.with_untracked(|folders| {
                        folders.iter().any(|f| f.id == folder_id && f.opts.watch)
                    })
                })
            })
    })
}

/// The books on a shelf's member list, in the order the page shows books: the
/// shelf's own order is the base and the view's sort rides over it — the same
/// `library_core::sort::ordered` the page's own level runs in
/// `crate::features::library::content::visible`, so an unfolded row and the
/// page it mirrors cannot disagree about what comes first.
fn member_books(state: AppState, members: Signal<Vec<String>>) -> Signal<Vec<Book>> {
    Signal::derive(move || {
        let ids = members.get();
        let view = state.library.view.get();
        state
            .library
            .books
            .with(|books| sort::ordered(books, &ids, view.sort, view.sort_asc))
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
fn ListRow(
    state: AppState,
    book: Book,
    crop: Signal<bool>,
    depth: usize,
    /// The shelf whose member list renders this row: the tree's own id for a
    /// row inside an expanded branch, `None` for the flat section — which the
    /// open level renders, and which the session resolves at the drop rather
    /// than at the mount, so a flat row can never carry a stale container
    /// through a drill. It is the fact a grid card never had to state, because
    /// a card is only ever drawn by the level the page is on.
    parent: Option<String>,
) -> impl IntoView {
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

    // The shelf's one press contract, the same one the grid's cards wear — a
    // row and a card answer a hold, a tap and a movement alike at two densities
    // because they are ONE wiring (see `crate::features::library::gestures`). A
    // movement is always a drag here, including from inside a selection: a set
    // that could not be lifted was a set the bar's "Add to shelf" was the only
    // way to move.
    let open_path = path.clone();
    let context_id = id.clone();
    let context_path = path.clone();
    let gestures = use_shelf_item(
        state,
        Some(drag),
        Some(menu),
        ShelfItemPolicy {
            id: id.clone(),
            label: Signal::stored(title.clone()),
            draggable: Signal::derive(|| true),
            open: Callback::new(move |_| document::open_path(state, open_path.clone())),
            menu_target: Callback::new(move |_| MenuTarget::Book {
                id: context_id.clone(),
                path: context_path.clone(),
                missing,
            }),
            // The tree's own fact: a nested row answers to its branch, a flat
            // row to the level the page is on.
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

    // The same target a card registers, under the same id scheme: a book is one
    // thing to a drag whatever density it is being shown at — plus the one fact
    // the tree adds: the container that renders THIS row, so a drop inside an
    // expanded branch indexes the branch's own member list and not the flat
    // order the page is showing.
    let dom_id = format!("book-{}", id);
    drag.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Book, id.clone()),
        dom_id: dom_id.clone(),
        shelf: parent,
    });

    let reveal_id = id.clone();
    let remove_id = id.clone();
    let over_id = id.clone();
    let after_id = id.clone();
    let fold_id = id.clone();
    let held_id = id.clone();
    let alt_path = path.clone();
    let alt_title = title.clone();
    let row_title = title.clone();
    let row_tooltip = title.clone();
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
            class=("row-drop-after", move || drag.inserts_after(&after_id))
            class=("row-fold-here", move || drag.folds_with(&fold_id))
            class=("row-missing", missing)
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
            // The same answers a card gives, at this density; the wiring is the
            // same wiring — see `crate::features::library::gestures`.
            on:click=move |ev: leptos::ev::MouseEvent| (on_click)(&ev)
            on:contextmenu=move |ev: leptos::ev::MouseEvent| (on_context)(&ev)
            on:keydown=move |ev: leptos::ev::KeyboardEvent| (on_key)(&ev)
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
