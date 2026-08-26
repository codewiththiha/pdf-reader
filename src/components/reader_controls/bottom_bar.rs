//! Auto-hide bottom bar: the displaced PageNavigation (prev/next + page input) and,
//! in continuous mode, a scroll slider. Mirrors the top bar's hover-reveal
//! pattern (always-mounted hot strip + slide-up + hide after a grace period),
//! so mouse readers keep page navigation without any persistent bottom chrome.
//!
//! The slider is a raw `<input type="range">` rather than the shared
//! `Slider` because its max is REACTIVE (it tracks the live virtualized
//! document extent); `Slider`'s `min`/`max` are fixed `f64`s. `navigation_sync`
//! already syncs DOM scroll ↔ `viewer.page`, so the slider and the page pill
//! stay consistent for free.

use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::reader_controls::page_navigation::PageNavigation;
use crate::state::ReaderState;
use pdf_core::layout::ViewMode;

/// Pointer must be off the bar this long before it hides.
const BOTTOM_HIDE_DELAY_MS: u64 = 400;

#[component]
pub fn ReaderBottomBar(
    reader: ReaderState,
    /// The continuous reader's virtualizer, used for slider extent and scroll writes.
    virtualizer: StoredValue<Virtualizer, LocalStorage>,
) -> impl IntoView {
    let visible = RwSignal::new(false);
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    let total_size = virtualizer.with_value(|v| v.total_size());

    let show: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(h) = timer.get_value() {
            h.clear();
            timer.set_value(None);
        }
        visible.set(true);
    });
    let hide_later = move || {
        if let Some(h) = timer.get_value() {
            h.clear();
        }
        let h = set_timeout_with_handle(
            move || visible.set(false),
            Duration::from_millis(BOTTOM_HIDE_DELAY_MS),
        )
        .ok();
        timer.set_value(h);
    };

    let show_strip = show.clone();
    let show_bar = show;

    view! {
        // Hot strip: always mounted, always draggable — the bottom edge stays
        // grabable while the bar is hidden. PageNavigation itself renders "– / –"
        // when no document is open, so an empty-state hover is harmless.
        <div
            class="absolute inset-x-0 bottom-0 z-40 h-2"
            data-tauri-drag-region="true"
            on:mouseenter=move |_| show_strip()
        ></div>

        <div
            class="toolbar-glass absolute inset-x-0 bottom-0 z-40 flex h-10 items-center gap-3 px-3 transition-all duration-200 ease-out"
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| show_bar()
            on:mouseleave=move |_| hide_later()
            class=("translate-y-3", move || !visible.get())
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            <PageNavigation state=reader />
            <Show when=move || reader.viewer.mode.get() == ViewMode::Continuous>
                <input
                    type="range"
                    min="0"
                    max=move || {
                        let total = total_size.get();
                        let (_, vh) = reader.viewer.container_size.get();
                        (total - vh).max(0.0).to_string()
                    }
                    prop:value=move || reader.viewer.scroll_top.get().to_string()
                    on:input=move |ev| {
                        if let Ok(offset) = event_target_value(&ev).parse::<f64>() {
                            virtualizer
                                .with_value(|v| v.scroll_to_offset(offset, ScrollMode::Instant));
                        }
                    }
                    class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
                />
            </Show>
        </div>
    }
}
