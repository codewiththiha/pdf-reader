//! The breadcrumb: the library page's only prose, and it is navigation.
//!
//! One crumb per level the page is drilled through, `All` first and the shelf the
//! page is on last. "All" is always a button, because it is the way back, and so is
//! every crumb above the last one: a folder three levels down is three clicks from
//! the root only if the reader can see all three. Before shelves could nest there
//! were two crumbs at most and the chain was a single optional value; it is a list
//! now because that is what a forest's path is.
//!
//! A list has no end, and a title bar does. The oldest crumbs are elided behind an
//! ellipsis, which is the same trade every bar that can go deep makes: the reader
//! keeps the LAST few — the ones nearest where they are — and gets the rest one
//! hover away. Whether the bar folds at all is a DEPTH question first: a chain
//! shallower than the fold's own depth gate (`fold::FOLD_MIN_DEPTH`) never
//! folds, however cramped — its crumbs truncate instead. Past the gate, how
//! many fold is a WIDTH question before it is
//! a count: every crumb is measured in a hidden probe against the cluster's own
//! live box, and the fold deepens on the same frame the bar gets cramped — no
//! window event anywhere. The count rule (`fold::CRUMB_KEEP`) is the fallback
//! for the frames before the first measurement, and for numbers that cannot be
//! trusted.
//!
//! The ellipsis is its own affordance and not an arrow on a crumb, and that is not
//! cosmetics. An arrow on the third level whose panel lists the FIRST and the
//! second reads as "deeper than three", because a disclosure hangs below the thing
//! it discloses; the elided levels are shallower, so the affordance standing for
//! them must not itself be a level. `…` claims to be nothing but a gap, which is
//! what it is, and the chain inside it is drawn in the bar's own grammar —
//! `2 > 3 > 4 >` wrapping to `5 > 6` — so a reader who has understood the bar has
//! already understood the panel.
//!
//! The panel opens on hover rather than on click, and stays open while the pointer
//! is on the ellipsis or on the panel: it is a way of SEEING the levels above, and
//! a click is already taken by the crumb it lands on. Leaving both closes it one
//! beat later, which is the beat the pointer needs to cross the gap between them.
//!

//! ## Crumbs are drop targets
//!
//! Every crumb — elided ones included — is a target the drag can land on, so a book
//! can be filed onto a level the reader is not standing on, including one the bar
//! has elided, which is the only way to reach a deep level with a hand full of
//! books without first putting them down. The ellipsis is a target as well and a
//! drop on it is not: it stands for several levels and names none of them, so
//! resting on it opens the panel and releasing on it does nothing.
//!
//! Which means the panel has to be openable DURING a drag, and a drag cannot raise
//! a `mouseenter`: the card the press began on holds the pointer capture, and a
//! captured pointer reports its boundary events to the capture target alone. So
//! while a drag is live the ellipsis opens from the session's own hot target
//! instead — the same geometry the drop is decided by, and the one thing under a
//! capture that still tells the truth.
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
//!
//! ## Three files, one bar
//!
//! [`fold`] is the fold's arithmetic — how many crumbs the bar keeps, which of
//! them the width hides, and how the panel packs what is hidden into rows —
//! pure and host-tested. [`panel`] is the ellipsis's own surface: the hover
//! intent, the ruler the panel measures itself with, and the chain it draws.
//! This file is the bar and its crumbs.


mod fold;
mod panel;

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::use_resize_observer::observe_elements;
use app_chrome::icon::{Icon, IconName};
use library_core::shelf::{ALL_SHELF, Shelf, ancestors};

use crate::components::primitives::form::text_input::TextInput;
use crate::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::services::library::{delete_shelf, rename_shelf};
use crate::state::AppState;

use fold::choose_split;
use panel::{EllipsisCrumb, HoverIntent};

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
        shelf: None,
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

    // The fold's two live numbers: what each crumb COSTS — the probe's boxes,
    // re-read whenever the chain changes — and what the cluster can HOLD, which
    // is its own client box, observed. Both move without a window resize: a
    // crumb renamed, a level drilled into, the trailing cluster growing, the
    // flex squeeze settling after the fold's own answer. The 0.5px guard on
    // the write is what makes that last one a fixed point rather than a loop.
    let nav_ref: NodeRef<html::Nav> = NodeRef::new();
    let probe_ref: NodeRef<html::Span> = NodeRef::new();
    let widths: RwSignal<Vec<f64>> = RwSignal::new(Vec::new());
    let avail: RwSignal<f64> = RwSignal::new(0.0);

    Effect::new(move |_| {
        let _ = chain.get();
        // A frame later: the probe's own reactive children re-render on the
        // chain change, and measuring the boxes before the patch would measure
        // the chain that just left.
        request_animation_frame(move || {
            let Some(probe) = probe_ref.get() else {
                return;
            };
            let kids = probe.children();
            let mut ws = Vec::with_capacity(kids.length() as usize);
            for index in 0..kids.length() {
                if let Some(kid) = kids.item(index) {
                    ws.push(kid.get_bounding_client_rect().width());
                }
            }
            if widths.get_untracked() != ws {
                widths.set(ws);
            }
        });
    });

    Effect::new(move |_| {
        let Some(nav) = nav_ref.get() else {
            return;
        };
        let Some(parent) = nav.parent_element() else {
            return;
        };
        let observed = parent.clone();
        let read = move || {
            let wide = parent.client_width() as f64;
            if (avail.get_untracked() - wide).abs() > 0.5 {
                avail.set(wide);
            }
        };
        read();
        observe_elements(vec![observed], move |_| read());
    });

    // The live split: by measured width, with the count rule as the fallback
    // for the frames before the probe has answered.
    let split_sig = Signal::derive(move || {
        let len = chain.get().len();
        choose_split(&widths.get(), avail.get(), len)
    });

    view! {
        <nav
            node_ref=nav_ref
            class="flex min-w-0 items-center gap-0.5 text-sm"
            aria-label="Library location"
        >
            // The width fold's ruler: one box per crumb plus one for the
            // ellipsis itself, wearing the live crumbs' own metrics — padding,
            // gap, the 10rem cap — invisible and out of flow. Plain spans with
            // no ids and no registrations: a ruler is not a crumb, and a second
            // element carrying a crumb's id would be a second answer for the
            // drag's hit-test and the reveal's scroll.
            <span node_ref=probe_ref class="lib-crumb-probe" aria-hidden="true">
                <span class="lib-crumb-probe-item">
                    <Icon name=IconName::More size=14 />
                </span>
                {move || {
                    let levels = chain.get();
                    let last = levels.len().saturating_sub(1);
                    levels
                        .into_iter()
                        .enumerate()
                        .map(|(at, crumb)| {
                            let trails = at != last;
                            view! {
                                <span class="lib-crumb-probe-item">
                                    <span class="truncate">{crumb.name}</span>
                                    {trails.then(|| {
                                        view! { <Icon name=IconName::Next size=13 /> }
                                    })}
                                </span>
                            }
                        })
                        .collect_view()
                }}
            </span>
            <AllCrumb state=state ctrl=ctrl />
            {move || {
                let levels = chain.get();
                let len = levels.len();
                let split = split_sig.get();
                let (elided, shown) = levels.split_at(split);
                let last = len.saturating_sub(1);
                // The ellipsis first, because the levels behind it are the OLDEST:
                // left to right has to stay root to leaf, in the bar and in the
                // panel alike, or a chain the reader has just learned to read means
                // something else in one place.
                let gap = (!elided.is_empty()).then(|| {
                    view! {
                        <EllipsisCrumb
                            state=state
                            ctrl=ctrl
                            elided=elided.to_vec()
                            intent=intent
                        />
                    }
                        .into_any()
                });
                let crumbs = shown.iter().enumerate().map(|(at, crumb)| {
                    let crumb = crumb.clone();
                    if split + at == last {
                        view! { <CurrentCrumb state=state ctrl=ctrl crumb=crumb /> }.into_any()
                    } else {
                        view! { <LevelCrumb state=state ctrl=ctrl crumb=crumb /> }.into_any()
                    }
                });
                gap.into_iter().chain(crumbs).collect_view()
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
                            width=224u32
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

