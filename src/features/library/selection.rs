//! Multi-select on the shelf: the gesture that starts it, the set it fills, and
//! the bar that acts on it.
//!
//! The gesture is the app's one long-press primitive with the app's one set of
//! tuning constants, so holding a book feels exactly like holding a highlight —
//! same delay, same slop, same swallowed click afterwards. The bar is the
//! `ActionBar` primitive, which has named library item selection as a consumer
//! since before there was a library to select from.
//!
//! What a selection can DO is narrower than what it can hold, deliberately. Filing
//! books on a shelf is membership and cannot touch a file. Removing them goes
//! through the same receipt sheet a single removal uses, because a bulk delete that
//! skipped the itemisation would be the one place in the app where "remove" does
//! not tell you what it takes — and the sheet already knows how to aggregate, so
//! the honest version costs nothing extra.
//!
//! Pointer capture is on, which is also what makes this coexist with dragging a
//! card: starting a drag fires `pointercancel` on the captured element, the press
//! is cancelled, and the drag proceeds. A hold that never moves completes instead.
//!
//! A keyboard cannot hold anything down, so it gets the same two halves as two
//! keys: Shift+Enter on a card enters selection with that card in it, and Enter
//! inside selection toggles. The mode is reachable without a pointer, which is the
//! only reason a bulk action on this shelf is not a mouse-only feature.

use std::collections::HashSet;

use leptos::html;
use leptos::prelude::*;

use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::PlacementSide;
use app_chrome::icon::IconName;
use library_core::shelf::{ALL_SHELF, Shelf};

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::overlay::action_bar::ActionBar;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::features::library::drag::ShelfOrder;
use crate::features::library::remove_modal::RemoveSheet;
use crate::services::library::{create_shelf, file_many};
use crate::state::AppState;

/// Enter selection with the pressed book already in it, which is what a long-press
/// means: not "start selecting" and then a second gesture to select this one.
pub(crate) fn enter_selection(state: AppState, book_id: &str) {
    let id = book_id.to_string();
    state.library.selecting.set(true);
    state.library.selected.update(|selected| {
        selected.insert(id);
    });
}

/// Leave selection and drop the set. Every exit goes through here — Done, Escape, a
/// click on empty shelf, an action that consumed the selection, leaving the page —
/// so there is one place that decides what "not selecting" means.
pub(crate) fn exit_selection(state: AppState) {
    state.library.selecting.set(false);
    state.library.selected.set(HashSet::new());
}

/// Toggle one book. The high-frequency operation, and the reason the set is a set:
/// "is this one in it" is asked by every card on every repaint, and a list would
/// answer it by walking.
pub(crate) fn toggle_selected(state: AppState, book_id: &str) {
    state.library.selected.update(|selected| {
        if !selected.remove(book_id) {
            selected.insert(book_id.to_string());
        }
    });
}

/// The selection as a list, in no particular order. Read untracked: every caller is
/// an action about to act, not a view about to subscribe.
pub(crate) fn selected_ids(state: AppState) -> Vec<String> {
    state
        .library
        .selected
        .with_untracked(|selected| selected.iter().cloned().collect())
}

/// The selection-mode wiring the page owns: the exit paths, and dropping the
/// selection when the page goes away.
///
/// Installed here rather than per card because Escape and "clicked on empty shelf"
/// are facts about the page, and a card that installed its own dismissal would be
/// one listener per card all racing to exit the same mode.
pub(crate) fn use_select_mode(state: AppState) {
    use_dismiss(
        state.library.selecting.into(),
        Callback::new(move |_| exit_selection(state)),
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::Click),
            // A card or a row handles its own click (it toggles), the bar is the
            // thing being reached for, and a shelf menu opened from the bar is a
            // continuation of the action rather than a click outside it.
            exclude_selectors: vec![
                ".book",
                ".library-row",
                ".library-select-bar",
                ".menu-popover",
            ],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );

    // Leaving the library drops the selection: it is a set of books the reader can
    // see selected, and the reader route shows none of them.
    on_cleanup(move || exit_selection(state));
}

/// The shelves a selection can be filed onto, in the order the grid shows them.
///
/// "All books" is not one of them: it is the pseudo-shelf the root breadcrumb
/// stands for, and filing onto it would be a way of filing nowhere at all.
fn shelf_choices(state: AppState) -> Signal<Vec<Shelf>> {
    Signal::derive(move || {
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .filter(|s| s.id != ALL_SHELF)
                .cloned()
                .collect()
        })
    })
}

#[component]
pub(crate) fn LibrarySelectBar(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let selecting = state.library.selecting;
    let count = Signal::derive(move || state.library.selected.with(|s| s.len()));
    let choices = shelf_choices(state);
    let shelf_menu = RwSignal::new(false);
    let shelf_anchor: NodeRef<html::Div> = NodeRef::new();

    view! {
        <ActionBar
            visible=Signal::derive(move || selecting.get())
            role="toolbar"
            aria_label="Book selection"
            class="library-select-bar"
        >
            <span class="mr-1.5 text-xs font-medium tabular-nums text-muted">
                {move || format!("{} selected", count.get())}
            </span>

            <Button
                on_click=move |_| {
                    // Everything on screen, not everything in the library: a
                    // search or a drilled shelf narrows what "All" can mean, and
                    // selecting books the reader cannot see is how a bulk action
                    // becomes a surprise.
                    let visible: Vec<String> = order
                        .0
                        .get_untracked()
                        .into_iter()
                        .map(|book| book.id)
                        .collect();
                    state.library.selected.update(|selected| {
                        selected.extend(visible);
                    });
                }
                variant=ButtonVariant::Ghost
                compact=true
                class="rounded-full px-3"
            >
                "All"
            </Button>

            <div node_ref=shelf_anchor class="relative inline-flex">
                <Button
                    on_click=move |_| shelf_menu.set(!shelf_menu.get_untracked())
                    variant=ButtonVariant::Ghost
                    compact=true
                    active=Signal::derive(move || shelf_menu.get())
                    disabled=Signal::derive(move || count.get() == 0)
                    class="rounded-full px-3"
                    title="File the selection on a shelf"
                >
                    "Add to shelf"
                </Button>
                <MenuPopover
                    open=shelf_menu
                    anchor=shelf_anchor
                    width=240
                    placement=PlacementSide::Above
                    // The bar is nowhere near the reader's title bar, so there is
                    // no bar to hold open while this menu is up.
                    hold_titlebar=false
                    class="max-h-72 overflow-y-auto p-1".to_string()
                >
                    <SectionLabel text="File the selection on" />
                    {move || {
                        choices.get().into_iter().map(|shelf| {
                            let label = shelf.name.clone();
                            let shelf_id = shelf.id.clone();
                            view! {
                                <MenuItem
                                    label=label
                                    on_click=move || {
                                        shelf_menu.set(false);
                                        let ids = selected_ids(state);
                                        file_many(state, &ids, &shelf_id);
                                        exit_selection(state);
                                    }
                                />
                            }
                        }).collect_view()
                    }}
                    <div class="my-1"><Separator /></div>
                    <MenuItem
                        icon=IconName::Plus
                        label="New shelf"
                        on_click=move || {
                            shelf_menu.set(false);
                            // Created without drilling into it: the reader picked
                            // books on one shelf and asked for them to be on
                            // another, and navigating away is an answer to a
                            // question they did not ask.
                            let shelf_id = create_shelf(state);
                            let ids = selected_ids(state);
                            file_many(state, &ids, &shelf_id);
                            exit_selection(state);
                        }
                    />
                </MenuPopover>
            </div>

            <Button
                on_click=move |_| {
                    let ids = selected_ids(state);
                    if ids.is_empty() {
                        return;
                    }
                    exit_selection(state);
                    remove_sheet.ask_many(ids);
                }
                variant=ButtonVariant::Ghost
                tone=ButtonTone::Danger
                compact=true
                class="rounded-full px-3"
                disabled=Signal::derive(move || count.get() == 0)
            >
                {move || format!("Remove ({})", count.get())}
            </Button>

            <Button
                on_click=move |_| exit_selection(state)
                variant=ButtonVariant::Ghost
                compact=true
                class="rounded-full px-3"
            >
                "Done"
            </Button>
        </ActionBar>
    }
}
