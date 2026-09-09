//! The drag overlay: one fixed layer, no pointer events of its own, drawn from
//! the controller and nothing else.
//!
//! A browser drag image cannot be any of the four things this is. It is one
//! bitmap of the element the press began on, so a set of four books drags as the
//! one that was pressed; it is made before the drag starts, so it cannot become
//! the shelf the drag is about to make; it is composited by the engine, so nothing
//! in the stylesheet can reach it; and it only ever sits under the pointer, so it
//! cannot sink into the thing it is about to land on. The layer is a view like any
//! other, which is what lets all four be true at once.
//!
//! `pointer-events: none` is load-bearing rather than tidy: the drag hit-tests the
//! registry against coordinates, so a layer that caught the pointer would only
//! ever be a target for itself.
//!
//! Portalled to the document body. The layer's coordinates are viewport
//! coordinates, and `position: fixed` only means the viewport while no ancestor is
//! a containing block for it — which a `backdrop-filter`, a `transform` or a
//! `contain` anywhere above would quietly stop being true. The floating surfaces
//! in this app already pay for that once, by name:
//! `crate::components::primitives::floating::popover`'s `coordinate_space`. A
//! portal is the same fix with nothing left to remember.

use leptos::portal::Portal;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::features::library::dnd::controller::{DragController, GhostTile};
use crate::features::library::folder_card::THUMB_CAP;

/// The most tiles the ghost fans out. The same cap a folder's plate has, so a
/// drag of nine books looks like the shelf it is offering to make and not like a
/// hand of cards; the count badge carries the rest.
const GHOST_TILES: usize = 4;

/// The overlay a live drag draws under the pointer.
#[component]
pub(crate) fn DragLayer() -> impl IntoView {
    let ctrl = use_context::<DragController>().expect("the library page installs the drag session");
    let live = ctrl.live();
    let fold = ctrl.fold();
    let ghost = ctrl.ghost();
    let count = ctrl.count();
    let at = ctrl.pointer();
    let sunk = ctrl.sink();
    // Hoisted out of the markup: `view!` reads a `>` in an attribute as the end of
    // the tag, so a comparison has to be made somewhere else and arrive as a bool.
    let several = Signal::derive(move || count.get() > 1);
    // One signal for the whole sunk state — the anchor, the scale AND the
    // transition — because they are one fact. A separate "is animating" flag would
    // be a second writer of the same frame, and the one frame the two could
    // disagree about is exactly the frame that matters: the ghost coming off a
    // target, where a transition armed a beat too long is a follow that starts out
    // trailing the hand.
    let is_sunk = Signal::derive(move || sunk.get().is_some());
    let style = Signal::derive(move || {
        // The sink spot rather than the pointer while sunk. The ghost has stopped
        // being about where the hand is and started being about where the drop
        // would land, and the two are the same point only by coincidence.
        match sunk.get() {
            Some(spot) => format!("left:{:.2}px;top:{:.2}px", spot.x, spot.y),
            None => {
                let (x, y) = at.get();
                format!("left:{x:.2}px;top:{y:.2}px")
            }
        }
    });

    view! {
        <Portal>
            <Show when=move || live.get() fallback=|| ()>
                <div
                    class="lib-drag-layer"
                    class=("lib-drag-sunk", move || is_sunk.get())
                    style=move || style.get()
                    aria-hidden="true"
                >
                    <div class="lib-drag-ghost">
                        {move || {
                            // A brewing fold replaces the ghost rather than sitting
                            // beside it: the thing the reader is about to drop IS
                            // the plate, and two answers to "what happens if I let
                            // go" is one too many.
                            if let Some(preview) = fold.get() {
                                return view! { <FoldPlate filled=preview.filled /> }.into_any();
                            }
                            let tiles = ghost.get();
                            let total = count.get();
                            view! {
                                {tiles
                                    .into_iter()
                                    .take(GHOST_TILES)
                                    .enumerate()
                                    .map(|(fan, tile)| view! { <GhostCard fan=fan tile=tile /> })
                                    .collect_view()}
                                <Show when=move || several.get() fallback=|| ()>
                                    <span class="lib-drag-count">{total}</span>
                                </Show>
                            }
                                .into_any()
                        }}
                    </div>
                </div>
            </Show>
        </Portal>
    }
}

/// One fanned tile of the ghost: the cover when the library has art for it, a
/// folder's glyph for a shelf, and the name's first letter when neither.
#[component]
fn GhostCard(fan: usize, tile: GhostTile) -> impl IntoView {
    let GhostTile { cover, label, folder } = tile;
    let letter = initial(&label);
    let face = match cover {
        Some(cover) => view! { <img class="lib-drag-img" src=cover alt="" loading="lazy" /> }
            .into_any(),
        None if folder => {
            view! { <span class="lib-drag-folder"><Icon name=IconName::Open size=16 /></span> }
                .into_any()
        }
        None => view! { <span class="lib-drag-letter">{letter}</span> }.into_any(),
    };
    view! {
        <span class="lib-drag-tile" style=format!("--fan:{fan}") title=label>
            {face}
        </span>
    }
}

/// The shelf the drop is about to make: a folder's own plate, with one cell lit
/// per item it would hold and a `+` in the next one.
///
/// The folder card's classes rather than a look of its own, on purpose — the
/// preview is a promise about what the card on this level will look like in a
/// moment, and a promise drawn in a different style is a promise the reader has
/// to translate.
#[component]
fn FoldPlate(filled: usize) -> impl IntoView {
    view! {
        <div class="lib-drag-fold">
            <div class="folder-thumb-grid">
                {(0..THUMB_CAP)
                    .map(|at| {
                        let next = at == filled;
                        let class = if at < filled {
                            "folder-thumb-cell folder-thumb-fill"
                        } else if next {
                            "folder-thumb-cell folder-thumb-next"
                        } else {
                            "folder-thumb-cell folder-thumb-empty"
                        };
                        view! {
                            <span class=class>
                                {next.then(|| view! { <Icon name=IconName::Plus size=14 /> })}
                            </span>
                        }
                    })
                    .collect_view()}
            </div>
            <span class="lib-drag-fold-label">"New shelf"</span>
        </div>
    }
}

/// The first letter of a name, for a tile with no art to show.
fn initial(label: &str) -> String {
    label
        .chars()
        .next()
        .map(|letter| letter.to_uppercase().to_string())
        .unwrap_or_default()
}
