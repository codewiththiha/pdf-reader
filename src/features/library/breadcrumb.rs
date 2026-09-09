//! The breadcrumb: the library page's only prose, and it is navigation.
//!
//! One crumb per level the page is drilled through, `All` first and the shelf the
//! page is on last. "All" is always a button, because it is the way back, and so is
//! every crumb above the last one: a folder three levels down is three clicks from
//! the root only if the reader can see all three. Before shelves could nest there
//! were two crumbs at most and the chain was a single optional value; it is a list
//! now because that is what a forest's path is.
//!
//! A list has no end, and a title bar does. Past [`CRUMB_KEEP`] levels the oldest
//! crumbs fold into a menu hung off the oldest one still shown, which is the same
//! trade every bar that can go deep makes: the reader keeps the LAST few — the ones
//! nearest where they are — and gets the rest one hover away. The arrow sits on the
//! crumb that carries the fold and on nothing else, so a shallow chain has no
//! affordance it does not need and `All` never grows one.
//!
//! The menu opens on hover rather than on click, and stays open while the pointer
//! is on the trigger or the panel: it is a way of SEEING the levels above, and a
//! click is already taken by the crumb it lands on. Leaving both closes it one beat
//! later, which is the beat the pointer needs to cross the gap between them.
//!
//! ## Crumbs are drop targets
//!
//! Every crumb and every row of the fold menu is a target the drag can land on, so
//! a book can be filed onto a level the reader is not standing on — including one
//! that is folded away, which is the only way to reach a deep level with a hand
//! full of books without first putting them down. The fold menu therefore has to be
//! openable DURING a drag, and a drag cannot raise a `mouseenter`: the card the
//! press began on holds the pointer capture, and a captured pointer reports its
//! boundary events to the capture target alone. So while a drag is live the trigger
//! opens from the session's own hot target instead — the same geometry the drop is
//! decided by, which is the one thing under a capture that still tells the truth.
//!
//! ## The parked shelf menu
//!
//! The LAST crumb used to be a button for a second reason: it is where a shelf the
//! reader made got its name changed or got taken apart. That popover is parked
//! behind [`SHOW_SHELF_CRUMB_MENU`] and the crumb reads as plain text, so the bar
//! has one kind of crumb in it while the fold is being settled. It is parked rather
//! than deleted: flipping the constant restores it verbatim, and keeping it
//! compiled is what keeps `crate::services::library::rename_shelf` reachable — a
//! service with no caller left is a service the next change deletes. Removal is
//! still reachable without it, from the selection bar's receipt.
//!
//! Renaming happens inline, in the crumb itself. A dialog that asks for a name
//! before showing the shelf it belongs to is a dialog the reader has to answer to
//! find out what they were asking for; here the thing being named is the thing
//! being typed over. Enter commits and Escape cancels, which is the pair a reader
//! already expects from a field that replaced a label.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::shelf::{ALL_SHELF, Shelf, ancestors};

use crate::components::primitives::form::text_input::TextInput;
use crate::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::services::library::{delete_shelf, rename_shelf};
use crate::state::AppState;

/// How many crumbs the bar keeps once a chain is deeper than this. Everything
/// older folds into the menu hung off the oldest one kept.
///
/// Four, because the bar's left cluster shares a row with a search box that has to
/// stay usable and a window that can be 640px wide: at four crumbs of `max-w-40`
/// the cluster is already asking for more room than a narrow window has, and a
/// fifth crumb is a fifth of the bar spent on where you have been.
const CRUMB_KEEP: usize = 4;

/// The fold menu's width. Fixed so a long shelf name ellipsises instead of
/// stretching the panel, and so the panel is the same box whatever shelf the
/// reader is standing in — a menu that resized between levels would move under the
/// pointer that is hovering it.
const OVERFLOW_WIDTH: u32 = 224;

/// The beat between leaving the trigger and closing, so the pointer can cross the
/// gap between the crumb and the panel.
const MENU_CLOSE_GRACE_MS: u64 = 220;

/// Whether the current shelf's crumb carries its rename/remove popover. Parked;
/// see the module docs.
const SHOW_SHELF_CRUMB_MENU: bool = false;

/// The element id of the root crumb, which stands for no shelf at all and so has
/// no id of its own to be named after.
const ALL_CRUMB_DOM_ID: &str = "crumb-all";

/// One crumb: the level it stands for, what it is called right now, and whether
/// the folder behind it is still watched.
///
/// `Clone` because the chain crosses a signal, and a `Signal` hands out copies
/// rather than references — a derived value's contents are read inside a lock that
/// a view cannot hold a borrow through.
#[derive(Clone)]
struct Crumb {
    id: String,
    name: String,
    /// Whether the shelf was cut from a folder that is still being watched. Every
    /// shelf is removable now; this is the one fact about a removal worth a
    /// sentence under the menu row, because it is the one consequence the reader
    /// cannot see coming — the shelf comes back when the folder places again.
    watched: bool,
}

/// The shelf the page is drilled into, or `None` at the root.
fn current_shelf_id(state: AppState) -> Option<String> {
    let id = state.library.shelf.get_untracked();
    (id != ALL_SHELF).then_some(id)
}

/// The name a shelf has right now. Read when an action runs rather than when the
/// crumb is built, so renaming the same shelf twice starts from what it is called
/// now and not from what it was called at mount.
fn shelf_name_now(state: AppState, shelf_id: &str) -> String {
    state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .find(|s| s.id == shelf_id)
            .map(|s| s.name.clone())
            .unwrap_or_default()
    })
}

/// The levels the page is drilled through, root first and ending with the one it is
/// on. Empty at the root, where "All" is the whole breadcrumb.
///
/// Tracked: the breadcrumb is a view, and a drill in or out is one of the two things
/// that change it (a rename is the other).
fn crumbs(state: AppState) -> Signal<Vec<Crumb>> {
    Signal::derive(move || {
        let id = state.library.shelf.get();
        if id == ALL_SHELF {
            return Vec::new();
        }
        state.library.shelves.with(|shelves| {
            // A shelf the list no longer holds answers as the root rather than as a
            // crumb with no name in it.
            let Some(current) = shelves.iter().find(|s| s.id == id) else {
                return Vec::new();
            };
            let of = |shelf: &Shelf| {
                let watched = shelf.kind.folder_id().is_some_and(|folder_id| {
                    state.library.folders.with_untracked(|folders| {
                        folders.iter().any(|f| f.id == folder_id && f.opts.watch)
                    })
                });
                Crumb {
                    id: shelf.id.clone(),
                    name: shelf.name.clone(),
                    watched,
                }
            };
            // `|s| of(s)` rather than `map(of)`: the adapter takes its closure by
            // value, and the last crumb is built by the same one a line later.
            let mut chain: Vec<Crumb> = ancestors(shelves, &id).into_iter().map(|s| of(s)).collect();
            chain.push(of(current));
            chain
        })
    })
}

// ---------------------------------------------------------------------------
// Drop targets
// ---------------------------------------------------------------------------

/// The element id a crumb's box is read from.
fn crumb_dom_id(shelf_id: &str) -> String {
    if shelf_id.is_empty() {
        ALL_CRUMB_DOM_ID.to_string()
    } else {
        format!("crumb-{shelf_id}")
    }
}

/// Register a crumb as the target for `shelf_id` — empty for the root — and answer
/// with the element id its box will be read from, for the view to put on it.
///
/// One call rather than a `NodeRef` and a rect reader, because a crumb's box is the
/// one thing about it that cannot go stale unnoticed: the chain re-renders on a
/// drill or a rename, and a target that outlived the crumb it belonged to would be
/// a way to file books onto a level that is not on screen. A fold-menu row is the
/// same target as a bar crumb and registers the same way, which is what makes a
/// folded level reachable with a hand full of books.
fn register_crumb(ctrl: &DragController, shelf_id: &str) -> String {
    let dom_id = crumb_dom_id(shelf_id);
    ctrl.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Shelf, shelf_id.to_string()),
        dom_id: dom_id.clone(),
    });
    dom_id
}

/// What a crumb wears. The crumb that is not where you are reads as a way back and
/// the one that is reads as a heading; a crumb under a drag adds the accent rule
/// that says the held items are about to go there.
///
/// A computed string rather than a conditional class because the hot state is the
/// third of three things deciding the look, and three `class=` attributes on one
/// element is three writers of one property.
fn crumb_class(current: bool, hot: bool) -> String {
    let base = "flex min-w-0 max-w-40 items-center gap-1 rounded-md px-1.5 py-0.5 \
                transition-colors focus:outline-none focus-visible:ring-2 \
                focus-visible:ring-accent";
    let tone = if current {
        "font-medium text-ink"
    } else {
        "text-muted hover:bg-line hover:text-ink"
    };
    if hot {
        format!("{base} {tone} crumb-drop")
    } else {
        format!("{base} {tone}")
    }
}

// ---------------------------------------------------------------------------
// Hover intent
// ---------------------------------------------------------------------------

/// One hover-opened menu's two facts: whether it is open, and whether the pointer
/// is on it.
///
/// The close is a beat behind the leave and owned by an effect on `over` rather
/// than by a timer parked in a `StoredValue`, so there is exactly one timer and it
/// is cancelled by the same thing that arms it. Coming back inside the beat re-runs
/// the effect, whose cleanup is the cancellation.
#[derive(Clone, Copy)]
struct HoverIntent {
    open: RwSignal<bool>,
    over: RwSignal<bool>,
}

impl HoverIntent {
    fn new() -> Self {
        let this = Self {
            open: RwSignal::new(false),
            over: RwSignal::new(false),
        };
        Effect::new(move |_| {
            if this.over.get() {
                return;
            }
            let Ok(close) = set_timeout_with_handle(
                move || this.open.set(false),
                Duration::from_millis(MENU_CLOSE_GRACE_MS),
            ) else {
                return;
            };
            on_cleanup(move || close.clear());
        });
        this
    }

    /// The pointer arrived on the trigger or the panel.
    fn enter(&self) {
        self.over.set(true);
        self.open.set(true);
    }

    /// The pointer left both. The close is the effect's, one beat later.
    fn leave(&self) {
        self.over.set(false);
    }

    /// Close now, and stop counting a hover that is not coming back.
    fn close(&self) {
        self.over.set(false);
        self.open.set(false);
    }
}

// ---------------------------------------------------------------------------

#[component]
pub(crate) fn Breadcrumb(state: AppState) -> impl IntoView {
    let ctrl = use_context::<DragController>().expect("the library page installs the drag session");
    let chain = crumbs(state);
    let intent = HoverIntent::new();
    let live = ctrl.live();

    // A drill, a rename or a removal lists different levels, and a fold menu of
    // the old ones is a menu of places the bar is no longer showing.
    Effect::new(move |_| {
        let _ = chain.get();
        intent.close();
    });
    // So is a drag that has ended: the menu was opened by the drop target the
    // pointer was on, and a reader who has let go is not holding anything over it.
    Effect::new(move |_| {
        if !live.get() {
            intent.close();
        }
    });

    view! {
        <nav class="flex min-w-0 items-center gap-0.5 text-sm" aria-label="Library location">
            <AllCrumb state=state ctrl=ctrl />
            {move || {
                let levels = chain.get();
                let len = levels.len();
                // Nothing is folded until the chain is deeper than the bar keeps,
                // and the oldest KEPT crumb is the one that carries the fold.
                let split = len.saturating_sub(CRUMB_KEEP);
                let (folded, shown) = levels.split_at(split);
                let last = len.saturating_sub(1);
                shown
                    .iter()
                    .enumerate()
                    .map(|(at, crumb)| {
                        let crumb = crumb.clone();
                        if split + at == last {
                            view! { <CurrentCrumb state=state ctrl=ctrl crumb=crumb /> }.into_any()
                        } else if at == 0 && split > 0 {
                            view! {
                                <OverflowCrumb
                                    state=state
                                    ctrl=ctrl
                                    crumb=crumb
                                    folded=folded.to_vec()
                                    intent=intent
                                />
                            }
                                .into_any()
                        } else {
                            view! { <LevelCrumb state=state ctrl=ctrl crumb=crumb /> }.into_any()
                        }
                    })
                    .collect_view()
            }}
        </nav>
    }
}

/// The root crumb. Always a button, because it is the way back, and a target with
/// an empty id — the library's spelling of "no shelf", which is what makes a drop
/// here take a book OFF the shelf it was dragged out of.
#[component]
fn AllCrumb(state: AppState, ctrl: DragController) -> impl IntoView {
    let dom_id = register_crumb(&ctrl, "");
    let at_root = Signal::derive(move || state.library.shelf.get() == ALL_SHELF);

    view! {
        <button
            id=dom_id
            type="button"
            title="The top level of the library"
            on:click=move |_| state.library.shelf.set(ALL_SHELF.to_string())
            class=move || {
                let base = "shrink-0 rounded-md px-1.5 py-0.5 transition-colors \
                            focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                let tone = if at_root.get() {
                    "font-medium text-ink"
                } else {
                    "text-muted hover:bg-line hover:text-ink"
                };
                if ctrl.over_shelf("") {
                    format!("{base} {tone} crumb-drop")
                } else {
                    format!("{base} {tone}")
                }
            }
        >
            "All"
        </button>
    }
}

/// A crumb that is not where you are: the chevron, the level's name, and the click
/// that goes back to it.
///
/// Its own component so the last crumb's menu state stays out of it — a way back is
/// a link, and a link with a rename field inside it is two controls fighting over
/// one click.
#[component]
fn LevelCrumb(state: AppState, ctrl: DragController, crumb: Crumb) -> impl IntoView {
    let id = crumb.id.clone();
    let label = crumb.name.clone();
    let tooltip = crumb.name.clone();
    let aria = format!("Go back to {}", crumb.name);
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id.clone();

    view! {
        <span class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                id=dom_id
                type="button"
                title=tooltip
                aria-label=aria
                on:click=move |_| state.library.shelf.set(id.clone())
                class=move || crumb_class(false, ctrl.over_shelf(&hot_id))
            >
                <span class="truncate">{label}</span>
            </button>
        </span>
    }
}

/// The shelf the page is on. Plain text while its popover is parked, but still a
/// drop target: releasing a held book here files it onto the level the reader is
/// already looking at, which is how a drag from inside a nested shelf lands back on
/// the shelf that contains it.
#[component]
fn CurrentCrumb(state: AppState, ctrl: DragController, crumb: Crumb) -> impl IntoView {
    if SHOW_SHELF_CRUMB_MENU {
        return view! { <ShelfCrumbMenu state=state crumb=crumb /> }.into_any();
    }
    let id = crumb.id.clone();
    let label = crumb.name.clone();
    let tooltip = crumb.name;
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id;

    view! {
        <span class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <span id=dom_id title=tooltip class=move || crumb_class(true, ctrl.over_shelf(&hot_id))>
                <span class="truncate">{label}</span>
            </span>
        </span>
    }
        .into_any()
}

/// The oldest crumb still shown, with the folded ones behind it.
///
/// The arrow is the affordance and the hover is the gesture: a click still goes to
/// the crumb's own level, because a crumb that opened a menu instead of navigating
/// would be a way back that is not a way back. ArrowDown is the gesture a keyboard
/// gets, and it is not a nicety — the folded levels are on no other surface, so
/// without it a chain deeper than the bar keeps would be navigable by mouse only.
#[component]
fn OverflowCrumb(
    state: AppState,
    ctrl: DragController,
    crumb: Crumb,
    folded: Vec<Crumb>,
    intent: HoverIntent,
) -> impl IntoView {
    let id = crumb.id.clone();
    let label = crumb.name.clone();
    let tooltip = crumb.name.clone();
    let aria = format!("Go back to {}, and see the levels above it", crumb.name);
    let dom_id = register_crumb(&ctrl, &id);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let live = ctrl.live();
    // Parked in a `StoredValue` rather than captured: a component's children are
    // an `Fn`, and a children closure that owned the folded list would be an
    // `FnOnce` the first time it built a row. A Copy handle to a plain scoped cell
    // is the same fix `crate::components::primitives::floating::popover` uses for
    // its panel class.
    let rows: StoredValue<Vec<Crumb>, LocalStorage> = StoredValue::new_local(folded);
    // One clone per closure that outlives this frame: the click, the class and the
    // drag effect each need the id, and a `move` closure takes what it captures.
    let click_id = id.clone();
    let hot_id = id.clone();
    let drag_id = id;

    // A drag cannot raise a `mouseenter` — the card the press began on holds the
    // pointer capture, and a captured pointer reports its boundary events to the
    // capture target alone. So while a drag is live the menu opens from the
    // session's hot target instead, which is the same geometry the drop itself is
    // decided by and the one thing under a capture that still tells the truth.
    Effect::new(move |_| {
        if live.get() && ctrl.over_shelf(&drag_id) {
            intent.enter();
        }
    });

    view! {
        <div
            node_ref=anchor
            class="relative flex min-w-0 items-center gap-0.5"
            on:mouseenter=move |_| intent.enter()
            on:mouseleave=move |_| intent.leave()
        >
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                id=dom_id
                type="button"
                title=tooltip
                aria-label=aria
                aria-haspopup="menu"
                aria-expanded=move || intent.open.get().to_string()
                on:click=move |_| state.library.shelf.set(click_id.clone())
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    // A keyboard cannot hover, and the levels behind this crumb are
                    // nowhere else on the page — so the key that means "show me what
                    // is under this" opens the fold. Enter still navigates, which is
                    // what the crumb's own label promises.
                    if ev.key() == "ArrowDown" {
                        ev.prevent_default();
                        intent.enter();
                    }
                }
                class=move || crumb_class(false, ctrl.over_shelf(&hot_id))
            >
                <span class="truncate">{label}</span>
                <Icon name=IconName::ChevronDown size=11 class="shrink-0 text-muted" />
            </button>
            // The panel is a DOM descendant of the trigger, so the pointer crossing
            // into it never leaves the surface the hover is counted on — except for
            // the gap between the two, which is what the grace beat is for. The
            // rows carry the same two handlers so entering the panel cancels the
            // close that crossing the gap started.
            //
            // The rows are built inside the popover's reactive child rather than
            // before the markup, because each one registers a drop target that
            // leaves the registry when the row unmounts: built here, that owner is
            // the popover's, so closing the menu is what unregisters the rows, and
            // a menu that has gone is not a set of targets the drag can still hit.
            <MenuPopover
                open=intent.open
                anchor=anchor
                width=OVERFLOW_WIDTH
                coordinate_space="toolbar-row"
                class="max-h-80 overflow-y-auto p-1".to_string()
            >
                <div
                    on:mouseenter=move |_| intent.enter()
                    on:mouseleave=move |_| intent.leave()
                >
                    {move || {
                        rows.get_value()
                            .into_iter()
                            .map(|each| {
                                view! {
                                    <OverflowRow state=state ctrl=ctrl crumb=each intent=intent />
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </MenuPopover>
        </div>
    }
}

/// One folded level in the menu.
///
/// A row and not `crate::components::primitives::menu::menu_item`: the primitive
/// computes its own class and has no element id to offer, and this row needs both —
/// an id for the drag to read its box from, and the hot state a target under a drag
/// wears. It is a level to go to and a place to put things, not an action.
#[component]
fn OverflowRow(state: AppState, ctrl: DragController, crumb: Crumb, intent: HoverIntent) -> impl IntoView {
    let id = crumb.id.clone();
    // Two names for one string: the markup's children are built before its
    // attributes, so a `title` and a text node that shared a variable would be a
    // borrow of a value the child had already moved.
    let label = crumb.name.clone();
    let tooltip = crumb.name.clone();
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id.clone();

    view! {
        <button
            id=dom_id
            type="button"
            role="menuitem"
            title=tooltip
            on:click=move |_| state.library.shelf.set(id.clone())
            on:mouseenter=move |_| intent.enter()
            on:mouseleave=move |_| intent.leave()
            class=move || {
                let base = "flex w-full min-w-0 items-center rounded-md px-2 py-1.5 text-left \
                            text-sm text-muted transition-colors hover:bg-line hover:text-ink \
                            focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                if ctrl.over_shelf(&hot_id) {
                    format!("{base} crumb-drop")
                } else {
                    base.to_string()
                }
            }
        >
            <span class="truncate">{label}</span>
        </button>
    }
}

/// The current shelf's own popover: rename in place, or take the shelf apart.
/// Parked behind [`SHOW_SHELF_CRUMB_MENU`]; see the module docs.
#[component]
fn ShelfCrumbMenu(state: AppState, crumb: Crumb) -> impl IntoView {
    let menu_open = RwSignal::new(false);
    let renaming = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let name = crumb.name.clone();
    let tooltip = name.clone();
    let aria = format!("{name} shelf options");
    // A Copy local rather than a field of the prop, so the popover's children stay
    // an `Fn`: the note under the row is the only thing here that reads the crumb.
    let watched = crumb.watched;

    view! {
        <div class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            {move || {
                if renaming.get() {
                    return view! { <RenameField state=state draft=draft renaming=renaming /> }
                        .into_any();
                }
                let title = tooltip.clone();
                let aria_label = aria.clone();
                let shown = name.clone();
                view! {
                    <div node_ref=anchor class="relative flex min-w-0 shrink items-center">
                        <button
                            type="button"
                            title=title
                            aria-label=aria_label
                            on:click=move |_| menu_open.set(!menu_open.get_untracked())
                            class="flex min-w-0 max-w-40 items-center gap-1 rounded-md px-1.5 py-0.5 \
                                   font-medium text-ink transition-colors hover:bg-line \
                                   focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        >
                            <span class="truncate">{shown}</span>
                            <Icon name=IconName::ChevronDown size=11 class="shrink-0 text-muted" />
                        </button>
                        <MenuPopover
                            open=menu_open
                            anchor=anchor
                            width=224
                            coordinate_space="toolbar-row"
                            class="p-1".to_string()
                        >
                            <MenuItem
                                icon=IconName::Type
                                label="Rename…"
                                on_click=move || {
                                    menu_open.set(false);
                                    if let Some(id) = current_shelf_id(state) {
                                        draft.set(shelf_name_now(state, &id));
                                    }
                                    renaming.set(true);
                                }
                            />
                            <MenuItem
                                icon=IconName::Close
                                label="Remove shelf"
                                tone=MenuItemTone::Danger
                                on_click=move || {
                                    menu_open.set(false);
                                    if let Some(id) = current_shelf_id(state) {
                                        delete_shelf(state, &id);
                                    }
                                }
                            />
                            {watched.then(|| {
                                view! {
                                    <p class="px-2 py-1.5 text-[11px] text-muted">
                                        "Cut from a watched folder: removing takes it off the list, and it
                                         returns if the folder places a book here again."
                                    </p>
                                }
                            })}
                        </MenuPopover>
                    </div>
                }
                    .into_any()
            }}
        </div>
    }
}

/// The crumb while it is being renamed: the field that replaced the label.
#[component]
fn RenameField(
    state: AppState,
    draft: RwSignal<String>,
    renaming: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <span class="w-40 shrink-0">
            <TextInput
                value=draft
                on_input=Callback::new(move |text| draft.set(text))
                aria_label="Shelf name".to_string()
                autofocus=true
                class="w-full rounded border border-accent bg-paper px-1.5 py-0.5 text-sm text-ink focus:outline-none"
                    .to_string()
                on_keydown=Callback::new(move |ev: leptos::ev::KeyboardEvent| {
                    match ev.key().as_str() {
                        "Enter" => {
                            ev.prevent_default();
                            if let Some(id) = current_shelf_id(state) {
                                rename_shelf(state, &id, &draft.get_untracked());
                            }
                            renaming.set(false);
                        }
                        "Escape" => {
                            ev.prevent_default();
                            renaming.set(false);
                        }
                        _ => {}
                    }
                })
            />
        </span>
    }
}
