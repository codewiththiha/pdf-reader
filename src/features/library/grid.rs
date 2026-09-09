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
//! derived once for both views. The grid itself is not a drop target and carries
//! no drag handlers: the space a card is not standing on belongs to the level, and
//! the level registers its own box once in [`content`] for both layouts to share.
//!
//! [`content`]: crate::features::library::content

use leptos::prelude::*;

use library_core::view::CoverFit;

use crate::features::library::add_card::AddCard;
use crate::features::library::book_card::BookCard;
use crate::features::library::content::{FolderOrder, ShelfOrder};
use crate::features::library::folder_card::FolderCard;
use crate::state::AppState;

#[component]
pub(crate) fn GridView(state: AppState) -> impl IntoView {
    let order = use_context::<ShelfOrder>().expect("the library content provides the order");
    let folders = use_context::<FolderOrder>().expect("the library content provides the folders");
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
