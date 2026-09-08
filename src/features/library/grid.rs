//! The grid: the folders at this level, then the books, then the add card.
//!
//! One CSS grid holds all three, and a folder is a cell of it exactly like a
//! book's card is — which is the whole of what makes the library nestable. The
//! shelf tile this replaced spanned the grid (`grid-column: 1 / -1`) to read as a
//! row OF books rather than a card among them, and a row cannot be inside a row:
//! a shelf filed in another shelf had nowhere to be drawn. A folder card can be,
//! and every level of the library is the same shape as the one above it.
//!
//! The column count is a custom property the view menu writes, which keeps "how
//! many across" a single token rather than a class per count.
//!
//! The level itself — which folders and which books — arrives from
//! `crate::features::library::content` as [`FolderOrder`] and [`ShelfOrder`],
//! derived once for both views.

use leptos::prelude::*;

use library_core::shelf::ALL_SHELF;
use library_core::view::CoverFit;

use crate::features::library::add_card::AddCard;
use crate::features::library::book_card::BookCard;
use crate::features::library::drag::{self, DropTarget, FolderOrder, ShelfOrder};
use crate::features::library::folder_card::FolderCard;
use crate::services::library::{move_to_shelf, nest_shelf};
use crate::state::AppState;

#[component]
pub(crate) fn GridView(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");
    let crop = Signal::derive(move || state.library.view.with(|v| v.cover == CoverFit::Crop));
    let columns = Signal::derive(move || state.library.view.with(|v| v.columns_token()));

    view! {
        <div
            class="library-grid"
            // One class on the container rather than one per card: the books not in
            // the set all step back by the same amount, and a shelf of three hundred
            // should not run three hundred derivations to agree on that.
            class=("library-grid-selecting", move || state.library.selecting.get())
            style=move || format!("--lib-cols:{}", columns.get())
            on:dragover=move |ev| {
                // Empty space is a target too: dropping there appends a book to
                // this level and un-nests a folder into it, and without a claim
                // the cursor would say "no" over the gutters.
                if drag::accept(&ev) {
                    drop_target.0.set(None);
                }
            }
            on:drop=move |ev| {
                ev.prevent_default();
                drop_target.0.set(None);
                // "All" is not a shelf, so a drop at the root has no shelf to
                // file out of and no shelf to file into — `move_to_shelf` reads
                // the pseudo-shelf as "re-order the library's own list", and
                // `nest_shelf` reads `None` as "the top level".
                let at = state.library.shelf.get_untracked();
                let inside = (at != ALL_SHELF).then_some(at);
                if let Some(dragged) = drag::dragged(&ev) {
                    let target = inside.clone().unwrap_or_else(|| ALL_SHELF.to_string());
                    move_to_shelf(state, dragged, inside, target, None);
                    return;
                }
                if let Some(moved) = drag::dragged_folder(&ev) {
                    nest_shelf(state, &moved, inside.as_deref());
                }
            }
        >
            // Folders before books at every level: the doors out of this page are
            // the things a reader scans for first, and a folder that renders after
            // three hundred covers is a folder that has to be hunted for.
            <For each=move || folders.0.get() key=|s| s.id.clone() let:shelf>
                <FolderCard state=state shelf=shelf />
            </For>
            <For each=move || order.0.get() key=|b| b.id.clone() let:book>
                <BookCard state=state book=book crop=crop />
            </For>
            <AddCard state=state />
        </div>
    }
}
