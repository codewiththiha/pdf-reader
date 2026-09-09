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
//! crumbs are elided behind an ellipsis, which is the same trade every bar that can
//! go deep makes: the reader keeps the LAST few — the ones nearest where they are —
//! and gets the rest one hover away.
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

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;
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
/// older goes behind the ellipsis.
///
/// Three, because the bar's left cluster shares a row with a search box that has to
/// stay usable and a window that can be 640px wide — and because the ellipsis takes
/// a slot of its own, so three kept plus one elided is the four the bar used to
/// show. A fifth element is a fifth of the bar spent on where you have been.
const CRUMB_KEEP: usize = 3;

/// The id of the ruler the folded panel measures itself with: the chain drawn
/// once more, invisible and unwrapped, whose natural width is the width the
/// panel wants. Deliberately not in the drag's registry — it stands for no
/// level, and a target that is a measurement would be a way to file books onto
/// a ruler.
const ELIDED_MAX_DOM_ID: &str = "crumb-elided-max";

/// How wide the folded panel may be. 80% of the window is the budget, because a
/// panel across the whole bar is a panel hiding the shelf it is naming the way
/// back to; a window too narrow to split gets the whole width, because a panel
/// wider than its window is a panel off the edge.
fn elided_budget_px() -> f64 {
    let window = web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if window < 640.0 {
        window
    } else {
        window * 0.8
    }
}

/// The element id of the ellipsis. Deliberately not a `crumb-` id: it is a target
/// the drag can rest on but it stands for no level, and sharing the crumbs' scheme
/// would let a reader of the registry mistake it for one.
const ELLIPSIS_DOM_ID: &str = "crumb-elided";

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

/// How many of the chain's oldest levels the bar elides.
///
/// Never one. A single elided level costs the reader a hover to reach and costs the
/// bar the same width as showing it would have, so the ellipsis earns its slot from
/// two levels up — which is why a chain of four shows all four crumbs and a chain of
/// five shows three plus the ellipsis.
fn elide_at(len: usize) -> usize {
    let split = len.saturating_sub(CRUMB_KEEP);
    // `split == 1` is the one depth where eliding costs more than it saves.
    if split == 1 {
        0
    } else {
        split
    }
}


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
                let split = elide_at(len);
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

/// The ellipsis: the bar's one admission that it is not showing everything.
///
/// Not an arrow on a crumb, and the reason is the confusion an arrow makes — see
/// the module docs. It is a button, so a keyboard reaches the panel the way a
/// pointer does, and it is a target the drag can rest on but never drop on.
#[component]
fn EllipsisCrumb(
    state: AppState,
    ctrl: DragController,
    elided: Vec<Crumb>,
    intent: HoverIntent,
) -> impl IntoView {
    ctrl.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Ellipsis, String::new()),
        dom_id: ELLIPSIS_DOM_ID.to_string(),
    });
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let live = ctrl.live();
    // `elide_at` never answers one, so this is always the plural.
    let tooltip = format!("{} levels above, folded", elided.len());
    let aria = tooltip.clone();
    // Parked in a `StoredValue` rather than captured: a component's children are an
    // `Fn`, and a children closure that owned the list would be an `FnOnce` the
    // first time it built a crumb. A Copy handle to a plain scoped cell is the same
    // fix `crate::components::primitives::floating::popover` uses for its panel
    // class.
    let folded: StoredValue<Vec<Crumb>, LocalStorage> = StoredValue::new_local(elided);

    // The panel's width is measured rather than picked. Three numbers decide it:
    // the chain's own unwrapped width (what the panel WANTS), the window's budget
    // (what it MAY have), and the widest single crumb (what it must have — a name
    // does not wrap, and a panel narrower than its widest crumb is a panel with a
    // crumb hanging out of it). A constant guessed between the three and got all
    // of them wrong: too wide for one short name, too narrow for a chain that
    // would have fit. Written only on a real change, because the popover
    // re-places itself on this signal and a resize storm should not re-place it
    // by sub-pixels.
    let panel_width: RwSignal<f64> = RwSignal::new(0.0);
    let measure = move || {
        let Some(ruler) = by_id(ELIDED_MAX_DOM_ID) else {
            return;
        };
        let budget = elided_budget_px();
        if budget <= 0.0 {
            return;
        }
        let chain = ruler.get_bounding_client_rect().width();
        let mut widest: f64 = 0.0;
        let crumbs = ruler.children();
        for index in 0..crumbs.length() {
            if let Some(crumb) = crumbs.item(index) {
                widest = widest.max(crumb.get_bounding_client_rect().width());
            }
        }
        let want = chain.min(budget).max(widest.min(budget));
        if (panel_width.get_untracked() - want).abs() > 0.5 {
            panel_width.set(want);
        }
    };
    // Measured once the panel — and the ruler inside it — has mounted, and
    // re-measured on every resize while the panel is open: the budget is a
    // fraction of the window, and the chain re-wraps when the fraction moves.
    // The measurement is stable under a hover, which is what the old constant
    // was actually protecting: the width only moves when the chain or the
    // window does, and a chain that changed closed the panel a beat earlier.
    Effect::new(move |_| {
        if !intent.open.get() {
            return;
        }
        request_animation_frame(measure);
        let handle = window_event_listener(leptos::ev::resize, move |_| measure());
        on_cleanup(move || handle.remove());
    });

    // A drag cannot raise a `mouseenter` — the card the press began on holds the
    // pointer capture, and a captured pointer reports its boundary events to the
    // capture target alone. So while a drag is live the panel opens from the
    // session's hot target instead, which is the same geometry the drop itself is
    // decided by and the one thing under a capture that still tells the truth.
    Effect::new(move |_| {
        if live.get() && ctrl.over_ellipsis() {
            intent.enter();
        }
    });

    view! {
        <div
            node_ref=anchor
            class="relative flex shrink-0 items-center"
            on:mouseenter=move |_| intent.enter()
            on:mouseleave=move |_| intent.leave()
        >
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                id=ELLIPSIS_DOM_ID
                type="button"
                title=tooltip
                aria-label=aria
                aria-expanded=move || intent.open.get().to_string()
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    // A keyboard cannot hover, and the levels behind this are on no
                    // other surface — so the key that means "show me what is under
                    // this" opens the panel.
                    if ev.key() == "ArrowDown" {
                        ev.prevent_default();
                        intent.enter();
                    }
                }
                class="flex shrink-0 items-center rounded-md px-1 py-0.5 text-muted \
                       transition-colors hover:bg-line hover:text-ink focus:outline-none \
                       focus-visible:ring-2 focus-visible:ring-accent"
            >
                <Icon name=IconName::More size=14 />
            </button>
            // The chain is built inside the popover's reactive child rather than
            // before the markup, because each crumb in it registers a drop target
            // that leaves the registry when the crumb unmounts: built here, that
            // owner is the popover's, so closing the panel is what unregisters them,
            // and a panel that has gone is not a set of targets the drag can hit.
            <MenuPopover
                open=intent.open
                anchor=anchor
                width=Signal::derive(move || panel_width.get() as u32)
                coordinate_space="toolbar-row"
                class="lib-elided-panel max-h-80 overflow-y-auto p-1.5".to_string()
            >
                // The chain, drawn twice: once as the ruler — invisible,
                // unwrapped, never hovered — and once as the chain the reader
                // reads. The ruler wears the crumbs' own metrics (the
                // `.lib-elided-crumb` item, and a label boxed like the live
                // button) so what it measures is what the live chain needs, and
                // its natural width is the panel's answer to "how wide is this
                // fold".
                <div id=ELIDED_MAX_DOM_ID class="lib-elided-probe" aria-hidden="true">
                    {move || {
                        let levels = folded.get_value();
                        let last = levels.len().saturating_sub(1);
                        levels
                            .into_iter()
                            .enumerate()
                            .map(|(at, crumb)| {
                                view! {
                                    <span class="lib-elided-crumb">
                                        <span class="lib-elided-probe-label">
                                            <span class="truncate">{crumb.name}</span>
                                        </span>
                                        {(at != last).then(|| {
                                            view! {
                                                <Icon name=IconName::Next size=13 class="shrink-0" />
                                            }
                                        })}
                                    </span>
                                }
                            })
                            .collect_view()
                    }}
                </div>
                <div
                    class="lib-elided-chain"
                    on:mouseenter=move |_| intent.enter()
                    on:mouseleave=move |_| intent.leave()
                >
                    {move || {
                        let levels = folded.get_value();
                        let last = levels.len().saturating_sub(1);
                        levels
                            .into_iter()
                            .enumerate()
                            .map(|(at, crumb)| {
                                // Hoisted out of the markup: the last crumb in the
                                // chain trails nothing, and a comparison in an
                                // attribute is one more thing the macro has to
                                // agree with the reader about.
                                let trails = at != last;
                                view! {
                                    <ElidedCrumb
                                        state=state
                                        ctrl=ctrl
                                        crumb=crumb
                                        trails=trails
                                        intent=intent
                                    />
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </MenuPopover>
        </div>
    }
}


/// One elided level, in the panel: a chip in a chain rather than a row in a list.
///
/// The chain is the point. Drawn the way the bar draws it — name, chevron, name — a
/// reader who has understood the breadcrumb has already understood the panel, and
/// the panel can be three levels wide where a list of rows would have been three
/// levels tall. The chevron TRAILS its crumb and lives in the same flex item, so a
/// wrapped line ends on `4 >` and the next begins on `5`, which is how a chain reads
/// when it has to break; a leading chevron would put a stray `>` at the head of
/// every line but the first.
///
/// A chip and not `crate::components::primitives::menu::menu_item`: the primitive
/// computes its own class, has no element id to offer, and is a full-width row by
/// construction. This needs an id for the drag to read its box from, the hot state
/// a target under a drag wears, and a width that is its content's. It is a level to
/// go to and a place to put things, not an action.
#[component]
fn ElidedCrumb(
    state: AppState,
    ctrl: DragController,
    crumb: Crumb,
    trails: bool,
    intent: HoverIntent,
) -> impl IntoView {
    let id = crumb.id.clone();
    // Two names for one string: the markup's children are built before its
    // attributes, so a `title` and a text node sharing a variable would be a borrow
    // of a value the child had already moved.
    let label = crumb.name.clone();
    let tooltip = crumb.name;
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id.clone();
    let click_id = id;

    view! {
        <span class="lib-elided-crumb">
            <button
                id=dom_id
                type="button"
                title=tooltip
                on:click=move |_| state.library.shelf.set(click_id.clone())
                on:mouseenter=move |_| intent.enter()
                on:mouseleave=move |_| intent.leave()
                class=move || {
                    let base = "flex min-w-0 max-w-32 items-center rounded-md px-1.5 py-0.5 \
                                text-sm text-muted transition-colors hover:bg-line \
                                hover:text-ink focus:outline-none focus-visible:ring-2 \
                                focus-visible:ring-accent";
                    if ctrl.over_shelf(&hot_id) {
                        format!("{base} crumb-drop")
                    } else {
                        base.to_string()
                    }
                }
            >
                <span class="truncate">{label}</span>
            </button>
            {trails.then(|| {
                view! { <Icon name=IconName::Next size=13 class="shrink-0 text-muted" /> }
            })}
        </span>
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shallow_chain_elides_nothing() {
        for len in 0..=CRUMB_KEEP {
            assert_eq!(elide_at(len), 0, "a chain of {len} fits the bar whole");
        }
    }

    #[test]
    fn one_elided_crumb_is_not_worth_an_affordance() {
        // Four levels with three kept is one hidden crumb, and the ellipsis costs
        // the bar the same width the crumb would have — so the bar shows all four
        // and the ellipsis first earns its slot at five.
        assert_eq!(elide_at(CRUMB_KEEP + 1), 0);
        assert_eq!(elide_at(CRUMB_KEEP + 2), 2);
    }

    #[test]
    fn past_the_first_fold_the_bar_stays_the_same_width() {
        // However deep the chain goes, the bar keeps CRUMB_KEEP crumbs plus the
        // ellipsis, and everything older is in the panel.
        for len in (CRUMB_KEEP + 2)..=(CRUMB_KEEP * 4) {
            let split = elide_at(len);
            assert_eq!(len - split, CRUMB_KEEP, "a chain of {len} shows only {kept}", kept = CRUMB_KEEP);
            assert!(split >= 2, "and never hides just one");
        }
    }

    #[test]
    fn the_elided_and_the_shown_are_one_chain_and_never_overlap() {
        for len in 0..12 {
            let split = elide_at(len);
            assert!(split <= len);
            assert_eq!(split + (len - split), len);
        }
    }
}
