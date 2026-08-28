//! Scroll-horizontal layout: all pages in one horizontal strip.
//! Mirrors the scroll-vertical layout; only the axis flips.

use leptos::prelude::*;
use pdf_core::layout::Axis;
use virtual_list_leptos::Virtualizer;

use crate::components::document::shells::scroll_shell::ScrollShell;
use crate::state::ReaderState;

#[component]
pub fn ScrollHorizontalLayout(state: ReaderState, virtualizer: Virtualizer) -> impl IntoView {
    let progress_visible = Signal::derive(|| false);
    view! {
        <ScrollShell
            state=state
            virtualizer=virtualizer
            axis=Axis::Horizontal
            progress_visible=progress_visible
        />
    }
}
