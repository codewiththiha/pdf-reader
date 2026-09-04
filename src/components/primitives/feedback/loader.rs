//! The app's one loading mark: three dots chasing around a square.
//!
//! Pure CSS (`.loader` in `styles/components/animations.css`), painted in
//! `currentColor` so it takes whatever ink the surface around it uses — a
//! dark theme, a tinted paper, a muted caption all colour it for free. It
//! carries no timing of its own beyond the loop: the caller mounts it while
//! something is loading and unmounts it the moment that thing exists, so it
//! is never on screen a frame longer than the wait it stands for.
//!
//! The three dots are three elements, not a background trick, and the split is
//! load-bearing rather than stylistic. A mark that animates a paint property
//! (`background-position`, one box, three gradients — what this was) needs the
//! main thread every frame, and the main thread is precisely what is busy while
//! a document opens; the loop keeps its last frame instead of advancing, so the
//! indicator freezes during the wait it exists to cover. `transform` and
//! `opacity` are what a compositor animates on its own, which is why each dot is
//! an element that moves: the mark now reports on the load from outside it, and
//! the only thing the loading process still controls is whether the mark is on
//! screen at all.
//!
//! Same reason the reduced-motion nets do not stop it outright: they trade its
//! hops for a fade, because a motionless spinner is indistinguishable from a
//! hung app.

use leptos::prelude::*;

/// Default edge of the mark in CSS px. Big enough to read from across the
/// window when it sits alone in the middle of a surface.
const DEFAULT_SIZE: u32 = 72;

/// The three-dot loader. `size` is the square's edge in CSS px.
///
/// The dots are absolutely positioned inside the box the inline `width` gives
/// the parent, so `size` scales the whole mark — including the distance a dot
/// hops, which is a percentage of the dot itself.
#[component]
pub fn Loader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div
            class="loader"
            role="status"
            aria-label="Loading"
            style=format!("width:{size}px")
        >
            <span class="loader-dot loader-dot-a"></span>
            <span class="loader-dot loader-dot-b"></span>
            <span class="loader-dot loader-dot-c"></span>
        </div>
    }
}

/// The loader alone, centred in whatever box the caller gives it — the
/// shape every full-surface wait takes (opening a book, an empty panel
/// filling in). Fills its parent; the parent decides the extent.
#[component]
pub fn CenteredLoader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div class="flex h-full w-full items-center justify-center text-ink">
            <Loader size=size />
        </div>
    }
}
