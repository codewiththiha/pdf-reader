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
//! many across" a single token rather than a class per count. While Auto owns the
//! count the traffic runs the other way as well: the grid is the only thing that
//! knows how many columns the flow is producing, so it reads the computed tracks
//! on every resize and reports them into the view's `auto_fit` — which is what
//! lets the menu SHOW Auto's live count, and the stepper's first `+` pin the
//! count the reader is looking at rather than an idea nobody can see.
//!
//! The level itself — which folders and which books — arrives from
//! `crate::features::library::content` as [`FolderOrder`] and [`ShelfOrder`],
//! derived once for both views. The grid itself is not a drop target and carries
//! no drag handlers: the space a card is not standing on belongs to the level, and
//! the level registers its own box once in [`content`] for both layouts to share.
//!
//! [`content`]: crate::features::library::content

use leptos::html;
use leptos::prelude::*;

use library_core::view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit};

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
    let grid_ref: NodeRef<html::Div> = NodeRef::new();

    // While Auto owns the count, the grid is the only thing that knows it: read
    // the computed track count and hand it to the view, so the stepper's `+`
    // starts from what the shelf is showing (5 → 6) rather than from 1. The
    // write cannot re-layout: `auto_fit` is deliberately no part of
    // `columns_token`, so a measurement never moves the thing it measured.
    Effect::new(move |_| {
        let Some(node) = grid_ref.get() else {
            return;
        };
        // Tracked on purpose: the report belongs to Auto alone, so a pin takes
        // the listener down with the count it froze — and entering Auto again
        // measures at once rather than waiting for a resize that may not come.
        if state.library.view.with(|v| v.columns.is_some()) {
            return;
        }
        let view = state.library.view;
        let report = move || {
            let Some(tracks) = web_sys::window()
                .and_then(|w| w.get_computed_style(&node).ok().flatten())
                .and_then(|style| style.get_property_value("grid-template-columns").ok())
            else {
                return;
            };
            // A grid that is not being laid out reports `none`, and there is
            // nothing to count in it.
            if tracks == "none" {
                return;
            }
            let count = tracks.split_whitespace().count();
            if count == 0 {
                return;
            }
            let fit = count.clamp(usize::from(COLUMNS_MIN), usize::from(COLUMNS_MAX)) as u8;
            // Stale is harmless — a resize refreshes it before the next click —
            // but a write only on a real change keeps the signal quiet, and
            // with it this effect, which the write would otherwise re-run.
            if view.with_untracked(|v| v.auto_fit) != fit {
                view.update(|v| v.auto_fit = fit);
            }
        };
        report();
        let handle = window_event_listener(leptos::ev::resize, move |_| report());
        on_cleanup(move || handle.remove());
    });

    view! {
        <div
            node_ref=grid_ref
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
