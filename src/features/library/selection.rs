//! Multi-select on the shelf: the gesture that starts it, the set it fills, and
//! the bar that acts on it.
//!
//! The gesture is the app's one card wrapper at the app's one hold tuning, so
//! holding a book feels exactly like holding a highlight — same delay, same
//! swallowed click afterwards — and holding a folder feels exactly like holding a
//! book. See `crate::components::primitives::interactions::draggable_item` for why
//! one wrapper decides between the hold, the tap and the drag rather than three
//! listeners racing. The bar is the `ActionBar` primitive, which has named library
//! item selection as a consumer since before there was a library to select from.
//!
//! The set holds ids and does not care which kind they are: a selection of books
//! and folders is one selection, because that is what the reader sees on screen.
//! What a selection can DO is narrower than what it can hold, deliberately, and the
//! two halves are different operations on the same shelf list. Filing a book is
//! membership; filing a folder is nesting, which `library_core::shelf::can_nest`
//! refuses when it would close a loop and `Shelf::is_folder` refuses outright for
//! a shelf the disk places — its rung is the watched tree's, not the selection's. Removing goes through the receipt sheet a
//! single removal uses, and the sheet receipts both halves at once: a book's row
//! itemises what it takes with it, a shelf's row says what SURVIVES it — the books
//! stay in the library, the shelves inside it move up a level, and a shelf cut from
//! a watched folder says the folder keeps watching. One gesture, one confirmation,
//! one honest receipt.
//!
//! A keyboard cannot hold anything down, so it gets the same two halves as two
//! keys: Shift+Enter on a card enters selection with that card in it, and Enter
//! inside selection toggles. The mode is reachable without a pointer, which is the
//! only reason a bulk action on this shelf is not a mouse-only feature.
//!
//! A drag picks the set up rather than starting one — see [`payload_for`]. The
//! bar's "Add to shelf" is the deliberate half of a bulk move and a drag is the
//! quick half, and the two have to agree about what "the selection" is holding or
//! a reader who lifts three books and drops them on a folder gets something other
//! than what the bar said they had.

use std::collections::HashSet;

use leptos::html;
use leptos::prelude::*;

use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::PlacementSide;
use app_chrome::icon::IconName;
use library_core::shelf::{ALL_SHELF, Shelf, can_nest};

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::overlay::action_bar::ActionBar;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::features::library::content::{FolderOrder, ShelfOrder, visible};
use crate::features::library::dnd::controller::DragPayload;
use crate::features::library::remove_modal::RemoveSheet;
use crate::services::library::{create_shelf, file_many, nest_many};
use crate::state::AppState;

/// Enter selection with the pressed card already in it, which is what a hold
/// means: not "start selecting" and then a second gesture to select this one.
///
/// The id is a book's or a shelf's; the set does not tell them apart and nothing
/// below needs it to.
pub(crate) fn enter_selection(state: AppState, item_id: &str) {
    let id = item_id.to_string();
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

/// Toggle one card. The high-frequency operation, and the reason the set is a set:
/// "is this one in it" is asked by every card on every repaint, and a list would
/// answer it by walking.
pub(crate) fn toggle_selected(state: AppState, item_id: &str) {
    state.library.selected.update(|selected| {
        if !selected.remove(item_id) {
            selected.insert(item_id.to_string());
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

/// The selected ids that are books. What the receipt sheet is handed: it itemises
/// what a removal costs, and only a book has a resume point, placements,
/// highlights and maybe a store copy to itemise.
fn selected_books(state: AppState) -> Vec<String> {
    let folders = selected_folders(state);
    selected_ids(state)
        .into_iter()
        .filter(|id| !folders.contains(id))
        .collect()
}

/// The selected ids that are shelves. Filing these on a shelf is a nesting rather
/// than a membership, so the two halves of one action are two calls.
fn selected_folders(state: AppState) -> Vec<String> {
    let ids = selected_ids(state);
    state.library.shelves.with_untracked(|shelves| {
        ids.into_iter()
            .filter(|id| shelves.iter().any(|s| &s.id == id))
            .collect()
    })
}

/// What a press on `item_id` picks up: the whole set when the card is already in
/// it, and that card alone when it is not.
///
/// The rule a file manager teaches and the one that makes a selection worth
/// having — holding three books and lifting one of them lifts all three, while
/// lifting a book nobody selected lifts that book and leaves the set the reader
/// built alone. Telling the two apart is a question about the set rather than about
/// the gesture, so it lives here with the rest of the set's rules and both layouts
/// get the same answer.
///
/// Split into books and shelves on the way out, because the two halves of every
/// move are different operations on one list: a book becomes a member and a shelf
/// is nested.
pub(crate) fn payload_for(state: AppState, item_id: &str) -> DragPayload {
    if state
        .library
        .selected
        .with_untracked(|selected| selected.contains(item_id))
    {
        return DragPayload {
            books: in_page_order(state, selected_books(state)),
            folders: selected_folders(state),
        };
    }
    let folder = state
        .library
        .shelves
        .with_untracked(|shelves| shelves.iter().any(|s| s.id == item_id));
    if folder {
        DragPayload {
            books: Vec::new(),
            folders: vec![item_id.to_string()],
        }
    } else {
        DragPayload {
            books: vec![item_id.to_string()],
            folders: Vec::new(),
        }
    }
}

/// A set of books in the order the page is showing them.
///
/// The set is a set, and a set has no order — but a drop does: three books put
/// down before a card land in whatever order the payload names them, and an order
/// a hash iteration chose is an order the reader cannot predict. So the payload is
/// sorted into the level's own order on the way out, which is the same order the
/// drop counts its index in.
///
/// A book the page is NOT showing keeps its place at the end rather than being
/// dropped: a search can narrow the level under a set that was picked before it,
/// and a drag that silently lost a book would be a drag that removed one.
fn in_page_order(state: AppState, ids: Vec<String>) -> Vec<String> {
    let mut ordered: Vec<String> = visible(state)
        .into_iter()
        .map(|book| book.id)
        .filter(|id| ids.contains(id))
        .collect();
    // Collected before it is appended: a filter that read `ordered` while
    // `extend` held it mutably would be two borrows of one list.
    let rest: Vec<String> = ids
        .into_iter()
        .filter(|id| !ordered.contains(id))
        .collect();
    ordered.extend(rest);
    ordered
}

/// File the selection on one shelf: the books become members and the folders are
/// filed inside it. One action from the reader's side, two operations on the same
/// list, and a folder that cannot be nested there (because it would end up inside
/// itself) is simply left where it is rather than failing the batch.
fn file_selection(state: AppState, shelf_id: &str) {
    let books = selected_books(state);
    file_many(state, &books, shelf_id);
    let folders = selected_folders(state);
    nest_many(state, &folders, shelf_id);
}

/// Make a shelf at this level and file the selection in it, then drop the set.
///
/// Created without drilling into it: the reader picked cards on one shelf and
/// asked for them to be on another, and navigating away is an answer to a question
/// they did not ask. Created at this level, so the shelf the selection just went
/// into is one the reader can still see.
///
/// One definition, and it lives here rather than in either caller because the bar's
/// *New shelf* row and the level's right-click menu are the same action: two places
/// that each minted a shelf would eventually differ about whether to drill in.
pub(crate) fn file_selection_on_new_shelf(state: AppState) {
    let shelf_id = create_shelf(state);
    file_selection(state, &shelf_id);
    exit_selection(state);
}

/// Ask for the removal of the whole selection, and drop the set.
///
/// Both halves go to the sheet, because the sheet receipts both: a book's row
/// itemises what it takes with it and a shelf's row says what survives it. An ask
/// that handed over the books alone would take the shelves apart with no receipt
/// at all.
pub(crate) fn ask_remove_selection(state: AppState, sheet: &RemoveSheet) {
    let books = selected_books(state);
    let folders = selected_folders(state);
    if books.is_empty() && folders.is_empty() {
        return;
    }
    exit_selection(state);
    sheet.ask_many(books, folders);
}

/// Select everything on screen, which is what the bar's *All* and the level's
/// right-click both mean by it.
///
/// Everything ON SCREEN and not everything in the library: a search or a drilled
/// shelf narrows what "All" can mean, and selecting cards the reader cannot see is
/// how a bulk action becomes a surprise. Both halves of the level — the books and
/// the folders — because both are on it.
pub(crate) fn select_on_screen(state: AppState, order: ShelfOrder, folders: FolderOrder) {
    let on_screen: Vec<String> = order
        .0
        .get_untracked()
        .into_iter()
        .map(|book| book.id)
        .chain(folders.0.get_untracked().into_iter().map(|each| each.id))
        .collect();
    state.library.selecting.set(true);
    state.library.selected.update(|selected| {
        selected.extend(on_screen);
    });
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
            // A card, a folder or a row handles its own click (it toggles), the
            // bar is the thing being reached for, and a shelf menu opened from the
            // bar is a continuation of the action rather than a click outside it.
            exclude_selectors: vec![
                ".book-card",
                ".folder-card",
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
///
/// Neither is a shelf the selection could not be filed onto. With only books
/// selected that is none of them, but a selection holding a folder cannot be filed
/// inside that folder or inside one of its own children — and a menu row that
/// silently does nothing is worse than no row.
fn shelf_choices(state: AppState) -> Signal<Vec<Shelf>> {
    Signal::derive(move || {
        let selected = state.library.selected.get();
        state.library.shelves.with(|shelves| {
            shelves
                .iter()
                .filter(|s| s.id != ALL_SHELF)
                .filter(|s| {
                    selected
                        .iter()
                        .all(|id| can_nest(shelves, id, &s.id))
                })
                .cloned()
                .collect()
        })
    })
}

#[component]
pub(crate) fn LibrarySelectBar(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let selecting = state.library.selecting;
    // One count for both kinds, because the button removes both kinds and the
    // sheet's receipt is where the two halves are told apart.
    let count = Signal::derive(move || state.library.selected.with(|s| s.len()));
    let choices = shelf_choices(state);
    let shelf_menu = RwSignal::new(false);
    let shelf_anchor: NodeRef<html::Div> = NodeRef::new();

    view! {
        <ActionBar
            visible=Signal::derive(move || selecting.get())
            role="toolbar"
            aria_label="Library selection"
            class="library-select-bar"
        >
            <span class="mr-1.5 text-xs font-medium tabular-nums text-muted">
                {move || format!("{} selected", count.get())}
            </span>

            <Button
                on_click=move |_| select_on_screen(state, order, folders)
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
                                        file_selection(state, &shelf_id);
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
                            file_selection_on_new_shelf(state);
                        }
                    />
                </MenuPopover>
            </div>

            <Button
                on_click=move |_| ask_remove_selection(state, &remove_sheet)
                variant=ButtonVariant::Ghost
                tone=ButtonTone::Danger
                compact=true
                class="rounded-full px-3"
                disabled=Signal::derive(move || count.get() == 0)
                title="Remove the selected books, and take the selected shelves apart"
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
