//! The shelf's last cell: an add card shaped like a cover, opening the same two
//! rows the empty state offers.
//!
//! It is a card and not a toolbar button because the shelf is where the reader is
//! looking when they decide to add to it, and because a grid with a hole at the
//! end reads as unfinished. The shape is the affordance: same box as a cover,
//! dashed instead of painted, plus instead of art.
//!
//! It wears the book card's classes for that shape and is not a book, so the
//! selection dimming in `styles/library.css` names it as an exception — an add
//! affordance that stepped back with the unselected books would be advertising a
//! choice it does not offer.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::shelf;

use crate::features::library::add_menu::AddMenu;
use crate::state::AppState;

#[component]
pub(crate) fn AddCard(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    // A pick made from inside a shelf files onto it; one made from the root has
    // no shelf to file onto, and "All" is not a shelf.
    let target = Signal::derive(move || shelf::level_of_owned(&state.library.shelf.get()));

    view! {
        <div class="book-card book-add" node_ref=anchor>
            <button
                class="book-cover book-add-cover"
                type="button"
                aria-label="Add books"
                aria-haspopup="menu"
                aria-expanded=move || open.get().to_string()
                title="Add books"
                on:click=move |_| open.set(!open.get_untracked())
            >
                <Icon name=IconName::Plus size=32 class="text-muted" />
            </button>
            <AddMenu state=state open=open anchor=anchor target=target />
        </div>
    }
}
