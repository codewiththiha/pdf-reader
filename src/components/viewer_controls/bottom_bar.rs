//! Auto-hide bottom bar: the displaced PageNavigation (prev/next + page input)
//! plus a unified, page-based progress slider.
//!
//! The scrubber is a [`RangeInput`] mapped straight onto `viewer.page`
//! (`1..=num_pages`) rather than a scroll offset. Because a page number is
//! layout-agnostic it behaves identically in Single, Dual, Continuous and
//! Horizontal modes: there is no dependency on the active scroll axis, and no
//! reactive DOM scroll-offset reads to keep in sync. `navigation_sync` already
//! turns a `viewer.page` write into the matching scroll / `scroll_to_index`
//! jump (and the dominant-item tracker does the reverse), so dragging the
//! thumb and stepping with the PageNavigation stay consistent for free.
//!
//! THE HIDE MACHINE mirrors the title bar's (`app_chrome::titlebar::root`),
//! because the old shape — an enter-only strip plus a leave on the bar alone —
//! could leave the bar up after the pointer was already gone. An exit through
//! the window's bottom edge fires the strip's leave while the animating bar
//! has not become the hover target yet, so the bar never learns the pointer
//! left; a scrubber drag is the second case, because pointer capture keeps
//! every event glued to the input and swallows the bar's mouseleave until the
//! thumb is released. So the strip now carries BOTH hover edges like the
//! title bar's band, strip and bar share one `hovered` truth, the scrubber
//! drag is the bar's hold (the title bar's open popovers are its own), and a
//! recheck settles the visibility the moment that hold releases — the same
//! recheck the title bar runs for its holds.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use crate::components::primitives::floating::types::z::BAR;
use crate::components::primitives::form::range_input::RangeInput;
use app_chrome::hooks::use_timeout::use_hover_visibility;
use crate::components::viewer_controls::page_navigation::PageNavigation;
use crate::state::ReaderState;

/// Pointer must be off the bar this long before it hides.
const BOTTOM_HIDE_DELAY_MS: u64 = 400;

/// Whether the point still lands on the bar. Pointer capture keeps a scrubber
/// drag's events glued to the input — including releases that land outside
/// the bar — so after a drag the release coordinates are the only trustworthy
/// answer to "is the pointer still over us?". `element_from_point` skips
/// pointer-events:none decorations (the corner page indicator is one), so a
/// release over an inert overlay still counts as on-bar.
fn released_on_bar(bar: &web_sys::Element, x: f32, y: f32) -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.element_from_point(x, y))
        .is_some_and(|el| bar.contains(Some(&el)))
}

#[component]
pub fn ReaderBottomBar(reader: ReaderState) -> impl IntoView {
    let bar_ref = NodeRef::<html::Div>::new();
    // The scrubber drag is this bar's hold — what an open popover is to the
    // title bar: while it lasts the surface stays up, and when it ends the
    // recheck below settles the visibility.
    let dragging = RwSignal::new(false);
    // Show on enter, hide after a grace period unless the drag holds the bar
    // open. The shared primitive owns the timer + re-check-at-fire semantics;
    // the bar owns the hold definition — same split as the title bar.
    let hover = use_hover_visibility(
        Duration::from_millis(BOTTOM_HIDE_DELAY_MS),
        move || dragging.get(),
    );
    let visible = hover.visible;

    // One `hovered` truth serves the strip and the bar alike, exactly like
    // the title bar's band and row share theirs.
    let hovered = StoredValue::new_local(false);
    let enter = {
        let show = hover.show.clone();
        move || {
            hovered.set_value(true);
            show();
        }
    };
    let leave = {
        let hide = hover.hide_later.clone();
        move || {
            hovered.set_value(false);
            hide();
        }
    };
    let recheck = hover.hide_later.clone();
    Effect::new(move |_| {
        let _ = dragging.get();
        if !dragging.get() && !hovered.get_value() {
            recheck(); // the hold is gone and so is the pointer → hide
        }
    });

    let enter_strip = enter.clone();
    let leave_strip = leave.clone();
    let enter_bar = enter;
    let leave_bar = leave.clone();
    let leave_drag = leave;

    // End of drag. The captured pointerup / pointercancel bubble from the
    // input even when the release lands outside the bar; capture swallowed
    // the bar's mouseleave for the whole drag, so this is where the bar
    // learns the pointer's real position again — and records a leave when
    // the release did not come back to it.
    let end_drag = move |ev: leptos::ev::PointerEvent| {
        if !dragging.get_untracked() {
            return;
        }
        dragging.set(false);
        let over = bar_ref
            .get()
            .is_some_and(|bar| released_on_bar(&bar, ev.client_x() as f32, ev.client_y() as f32));
        if !over {
            leave_drag();
        }
    };

    view! {
        // The hover strip: this bar's band. Like the title bar's band it
        // carries BOTH edges of the hover — an enter-only strip leaves the
        // bar up whenever the pointer exits through the window's bottom edge
        // before the animating bar becomes the hover target.
        <div
            class=format!("absolute inset-x-0 bottom-0 {BAR} h-2")
            data-tauri-drag-region="true"
            on:mouseenter=move |_| enter_strip()
            on:mouseleave=move |_| leave_strip()
        ></div>

        <div
            node_ref=bar_ref
            class=format!(
                "toolbar-glass absolute inset-x-0 bottom-0 {BAR} flex h-10 items-center \
                 gap-3 px-3 transition-all duration-200 ease-out"
            )
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| enter_bar()
            on:mouseleave=move |_| leave_bar()
            on:pointerdown=move |_| dragging.set(true)
            on:pointerup=end_drag.clone()
            on:pointercancel=end_drag
            class=("translate-y-3", move || !visible.get())
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            <PageNavigation state=reader />
            <RangeInput
                value=Signal::derive(move || reader.viewer.page.get() as f64)
                min=Signal::derive(|| 1.0)
                max=Signal::derive(move || (reader.document.num_pages.get() as f64).max(1.0))
                step=Signal::derive(|| 1.0)
                on_input=move |page| {
                    reader.viewer.page.set(page.round() as u32);
                }
                aria_label="Page position"
                class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
            />
        </div>
    }
}
