//! The breadcrumb: the library page's only prose, and it is navigation.
//!
//! One crumb per level the page is drilled through, `All` first and the shelf the
//! page is on last. "All" is always a button, because it is the way back, and so is
//! every crumb above the last one: a folder three levels down is three clicks from
//! the root only if the reader can see all three. Before shelves could nest there
//! were two crumbs at most and the chain was a single optional value; it is a list
//! now because that is what a forest's path is.
//!
//! The LAST crumb is a button for a different reason: it is where a shelf the reader
//! made gets its name changed or gets taken apart, and a menu hanging off the crumb
//! is the one place on the page where "this shelf" has a handle. The crumbs above it
//! are navigation and nothing else, because a menu on each of them would be N menus
//! sharing one set of open/rename state — and one shared set is exactly what makes
//! the last crumb's menu work.
//!
//! Renaming happens inline, in the crumb itself. A dialog that asks for a name
//! before showing the shelf it belongs to is a dialog the reader has to answer to
//! find out what they were asking for; here the thing being named is the thing
//! being typed over. Enter commits and Escape cancels, which is the pair a reader
//! already expects from a field that replaced a label.
//!
//! Nothing here captures a shelf id for the menu's actions. Every one of them asks
//! which shelf the page is on when it runs, which is both the honest question — a
//! crumb is a handle for *here*, and "here" can change under a menu that is still
//! open — and the reason these closures can all be `Fn`: a reactive child is re-run,
//! and a closure that moved an id out of its environment on the first run has
//! nothing left for the second. The way-back crumbs DO capture theirs, because
//! "go to this level" is a fact about the crumb rather than about here.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::shelf::{ALL_SHELF, Shelf, ancestors};

use crate::components::primitives::form::text_input::TextInput;
use crate::components::primitives::menu::menu_item::{MenuItem, MenuItemTone};
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::services::library::{delete_shelf, rename_shelf};
use crate::state::AppState;

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

#[component]
pub(crate) fn Breadcrumb(state: AppState) -> impl IntoView {
    let chain = crumbs(state);
    // One set of handles, shared with whatever shelf is drilled into: only one LAST
    // crumb exists at a time, so a rename or a menu cannot be open in two places at
    // once, and a fresh crumb inherits a closed menu rather than a stale open one.
    let menu_open = RwSignal::new(false);
    let renaming = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let at_root = Signal::derive(move || chain.get().is_empty());

    view! {
        <nav class="flex min-w-0 items-center gap-0.5 text-sm" aria-label="Library location">
            <button
                type="button"
                title="The top level of the library"
                on:click=move |_| state.library.shelf.set(ALL_SHELF.to_string())
                class=move || {
                    // The crumb that is not where you are reads as a way back, and
                    // the one that is reads as a heading.
                    let base = "shrink-0 rounded-md px-1.5 py-0.5 transition-colors \
                                focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                    if at_root.get() {
                        format!("{base} font-medium text-ink")
                    } else {
                        format!("{base} text-muted hover:bg-line hover:text-ink")
                    }
                }
            >
                "All"
            </button>
            {move || {
                let levels = chain.get();
                let last = levels.len().saturating_sub(1);
                levels
                    .into_iter()
                    .enumerate()
                    .map(|(at, crumb)| {
                        let Crumb { id, name, watched } = crumb;
                        if at == last {
                            view! {
                                <ShelfCrumb
                                    state=state
                                    name=name
                                    watched=watched
                                    anchor=anchor
                                    menu_open=menu_open
                                    renaming=renaming
                                    draft=draft
                                />
                            }
                                .into_any()
                        } else {
                            view! { <LevelCrumb state=state id=id name=name /> }.into_any()
                        }
                    })
                    .collect_view()
            }}
        </nav>
    }
}

/// A crumb that is not where you are: the chevron, the level's name, and the click
/// that goes back to it.
///
/// Its own component so the last crumb's menu state stays out of it — a way back is
/// a link, and a link with a rename field inside it is two controls fighting over
/// one click.
#[component]
fn LevelCrumb(state: AppState, id: String, name: String) -> impl IntoView {
    let label = name.clone();
    let tooltip = name.clone();
    let aria = format!("Go back to {name}");

    view! {
        <span class="flex min-w-0 items-center gap-0.5">
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                type="button"
                title=tooltip
                aria-label=aria
                on:click=move |_| state.library.shelf.set(id.clone())
                class="flex min-w-0 max-w-40 items-center rounded-md px-1.5 py-0.5 text-muted \
                       transition-colors hover:bg-line hover:text-ink focus:outline-none \
                       focus-visible:ring-2 focus-visible:ring-accent"
            >
                <span class="truncate">{label}</span>
            </button>
        </span>
    }
}

/// The last crumb: the chevron, the shelf's name, and the menu behind it.
///
/// Its own component so the name arrives as an owned prop and the two states the
/// crumb can be in are two views rather than one view with a branch inside a
/// reactive child.
#[component]
fn ShelfCrumb(
    state: AppState,
    name: String,
    watched: bool,
    anchor: NodeRef<html::Div>,
    menu_open: RwSignal<bool>,
    renaming: RwSignal<bool>,
    draft: RwSignal<String>,
) -> impl IntoView {
    let tooltip = name.clone();
    let aria = format!("{name} shelf options");

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
