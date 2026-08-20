//! Auto-hide "titlebar". Revealed by hovering the top edge; draggable via
//! `data-tauri-drag-region` whether visible or not (the hot strip covers the
//! hidden state). Kept MOUNTED while hidden (opacity/translate + inert), never
//! unmounted: DocTitle's ResizeObserver ids and the popovers must survive.
//!
//! The hot strip is always mounted and always draggable — that is what makes
//! the titlebar-less window grabable while the bar is hidden. The bar itself
//! fades/slides in on `mouseenter` of the strip, hides ~400ms after the
//! pointer leaves it, and stays pinned while any popover (zoom / appearance /
//! more) or the floating search is open (`held`).

use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use pdf_core::layout::ViewMode;
use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::segmented::{Segmented, SegmentedLabel};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_viewer::state::SidebarMode;
use crate::core::open_flow::{close_document, open_dialog};
use crate::core::state::AppState;
use super::appearance_menu::AppearanceMenu;
use super::doc_title::DocTitle;
use super::more_menu::MoreMenu;
use super::zoom_controls::ZoomControls;

/// Pointer must be off the bar this long before it hides.
const HIDE_DELAY_MS: u64 = 400;

#[component]
pub fn AutoHideToolbar(state: AppState) -> impl IntoView {
    let visible = RwSignal::new(false);
    let timer = StoredValue::new_local(None::<TimeoutHandle>);

    // Popover states lifted so an open menu pins the bar open.
    let zoom_open = RwSignal::new(false);
    let appearance_open = RwSignal::new(false);
    let more_open = RwSignal::new(false);
    let held = move || {
        zoom_open.get()
            || appearance_open.get()
            || more_open.get()
            || state.search.visible.get()
    };

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
            Duration::from_millis(HIDE_DELAY_MS),
        )
        .ok();
        timer.set_value(h);
    };

    let mode = state.viewer.mode;
    let mode_state = state;
    let vs = state.viewer_state();

    // `show` is used by both the hot strip and the bar, so the handlers each
    // take their own clone.
    let show_strip = show.clone();
    let show_bar = show;

    view! {
        // Hot strip: always mounted, always draggable. This is what makes
        // the titlebar-less window grabable while the bar is hidden.
        <div
            class="absolute inset-x-0 top-0 z-40 h-2"
            data-tauri-drag-region="true"
            on:mouseenter=move |_| show_strip()
        ></div>

        <div
            // DocTitle measurement anchors MUST keep these ids.
            id="toolbar-row"
            data-tauri-drag-region="true"
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| show_bar()
            on:mouseleave=move |_| {
                if !held() {
                    hide_later();
                }
            }
            class="toolbar-glass absolute inset-x-0 top-0 z-40 flex h-12 items-center gap-2 pr-3 transition-all duration-200 ease-out"
            class=("pl-[120px]", move || state.sidebar.get() == SidebarMode::None)
            class=("pl-3", move || state.sidebar.get() != SidebarMode::None)
            class=("-translate-y-3", move || !visible.get())
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            // LEFT GROUP (id kept for DocTitle math).
            <div id="toolbar-left-pre" class="flex shrink-0 items-center gap-1">
                <Show when=move || {
                    matches!(state.doc.status.get(), DocStatus::Ready | DocStatus::Opening)
                }>
                    <Tooltip text="Library".to_string()>
                        <Button
                            on_click=move |_| close_document(state)
                            kind=ButtonKind::Ghost
                            icon=IconName::Library
                            title="Close this book and return to the library".to_string()
                        />
                    </Tooltip>
                </Show>
                <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                    <Button
                        on_click=move |_| open_dialog(state)
                        kind=ButtonKind::Toolbar
                        icon=IconName::Open
                        label="Open".to_string()
                        title="Open PDF (Cmd/Ctrl+O)".to_string()
                    />
                </Tooltip>
            </div>

            <DocTitle state=state />

            // RIGHT GROUP.
            <div id="toolbar-right" class="ml-auto flex shrink-0 items-center gap-1">
                // Search + More live HERE only when the sidebar (which owns
                // them otherwise) is closed.
                <Show when=move || state.sidebar.get() == SidebarMode::None>
                    <Tooltip text="Search (Cmd/Ctrl+F)".to_string()>
                        <button
                            type="button"
                            data-search-chrome="true"
                            title="Search (Cmd/Ctrl+F)"
                            on:pointerdown=move |ev| ev.stop_propagation()
                            on:click=move |_| {
                                if state.search.visible.get() {
                                    pdf_viewer::effects::search_effects::dismiss_search(vs);
                                } else {
                                    pdf_viewer::effects::search_effects::resume_search(vs);
                                }
                            }
                            class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        >
                            <Icon name=IconName::Search size=16 />
                        </button>
                    </Tooltip>
                </Show>

                <Tooltip text="View mode".to_string()>
                    <Segmented
                        options=vec![
                            (
                                ViewMode::Single,
                                SegmentedLabel::Icon(IconName::SinglePage),
                                "Single page view",
                            ),
                            (
                                ViewMode::Continuous,
                                SegmentedLabel::Icon(IconName::Continuous),
                                "Continuous scroll view",
                            ),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| mode_state.viewer.mode.set(m)
                    />
                </Tooltip>
                <ZoomControls state=state open_ext=zoom_open />
                <Separator vertical=true />
                <AppearanceMenu state=state open_ext=appearance_open />
                <Show when=move || state.sidebar.get() == SidebarMode::None>
                    <MoreMenu state=state open_ext=more_open />
                </Show>
            </div>
        </div>
    }
}
