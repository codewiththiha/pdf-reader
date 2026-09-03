//! Scroll-vertical layout: all pages in one vertical strip — or, for a
//! reflowable document, no pages at all.
//!
//! The format fork is the whole file: a PDF (and a text document in any of
//! the paged modes' shape) streams page hosts through the shared
//! [`ScrollShell`], while a TXT/Markdown document in this mode renders as
//! the continuous [`TextStream`](crate::components::text::TextStreamLayout)
//! — one uninterrupted column of blocks on the window's own paper, which is
//! what vertical reading of reflowable text is. The stream mounts under the
//! same scroller id, so everything that addresses the column by name (the
//! overlay scrollbar, the container observer, auto-scroll, the keyboard)
//! serves it unchanged.

use leptos::prelude::*;
use pdf_core::layout::Axis;
use virtual_list_leptos::Virtualizer;

use crate::components::document::shells::scroll_shell::ScrollShell;
use crate::components::text::TextStreamLayout;
use crate::state::ReaderState;

#[component]
pub fn ScrollVerticalLayout(
    state: ReaderState,
    virtualizer: Virtualizer,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    // The branch below is a dynamic child, and a dynamic child's closure
    // must be Send — which the Rc-backed Virtualizer is not. So it is
    // parked in local storage (the same pattern `Viewer` and `ScrollShell`
    // use) and resolved lazily for the PDF branch only; the stream mounts
    // its own virtualizer over the blocks.
    let strip_virtualizer = StoredValue::new_local(virtualizer);
    let stream_visible = progress_visible.clone();
    view! {
        {move || {
            if state.document.format.get().is_text() {
                view! {
                    <TextStreamLayout state=state progress_visible=stream_visible.clone() />
                }
                .into_any()
            } else {
                view! {
                    <ScrollShell
                        state=state
                        virtualizer=strip_virtualizer.get_value()
                        axis=Axis::Vertical
                        progress_visible=progress_visible.clone()
                    />
                }
                .into_any()
            }
        }}
    }
}
