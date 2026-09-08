//! The grid: bookshelves, then the books, then the add card.
//!
//! One CSS grid holds all three, and a shelf tile spans the whole of it
//! (`grid-column: 1 / -1` in `styles/library.css`) so a bookshelf reads as a row
//! of the shelf rather than as a section above it. The column count is a custom
//! property the view menu writes, which keeps "how many across" a single token
//! rather than a class per count.

use leptos::prelude::*;

use library_core::shelf::{Shelf, ALL_SHELF};
use library_core::view::CoverFit;

use crate::features::library::add_card::AddCard;
use crate::features::library::book_card::BookCard;
use crate::features::library::drag::{self, DropTarget, ShelfOrder};
use crate::features::library::shelf_tile::ShelfTile;
use crate::services::library::move_to_shelf;
use crate::state::AppState;

/// The shelves the grid shows: all of them at the root, none inside one. A shelf
/// drilled into has nothing left to file into, so the tiles would be doors to
/// doors.
fn shelves(state: AppState) -> Signal<Vec<Shelf>> {
    Signal::derive(move || {
        if state.library.shelf.get() != ALL_SHELF {
            return Vec::new();
        }
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
pub(crate) fn GridView(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let drop_target = use_context::<DropTarget>().expect("the library content provides the target");
    let tiles = shelves(state);
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
                // Empty space is a target too: dropping there appends, and
                // without a claim the cursor would say "no" over the gutters.
                if drag::accept(&ev) {
                    drop_target.0.set(None);
                }
            }
            on:drop=move |ev| {
                ev.prevent_default();
                drop_target.0.set(None);
                let Some(dragged) = drag::dragged(&ev) else {
                    return;
                };
                let shelf = state.library.shelf.get_untracked();
                let source = (shelf != ALL_SHELF).then(|| shelf.clone());
                move_to_shelf(state, dragged, source, shelf, None);
            }
        >
            <For each=move || tiles.get() key=|s| s.id.clone() let:shelf>
                <ShelfTile state=state shelf=shelf />
            </For>
            <For each=move || order.0.get() key=|b| b.id.clone() let:book>
                <BookCard state=state book=book crop=crop />
            </For>
            <AddCard state=state />
        </div>
    }
}
