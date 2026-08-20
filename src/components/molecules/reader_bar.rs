//! Hover-revealed reader titlebar. Mounted INSIDE `main#viewer-slot` so it
//! spans only the reader area: hovering the sidebar never reveals it. When the
//! sidebar is open it contains no traffic lights and no sidebar toggle (those
//! live in the sidebar's always-visible chrome row). Visibility is owned by
//! ReaderView because the traffic lights follow `sidebar_open || bar_visible`.
//!
//! Grab note: Tauri v2 starts a drag only when the mousedown lands on an
//! element that itself carries `data-tauri-drag-region`, so the band, the bar
//! and every non-interactive child (the left/right group wrappers and the
//! title span in DocTitle) carry it, while buttons stay clickable.

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
pub fn ReaderBar(state: AppState, visible: RwSignal<bool>) -> impl IntoView {
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

    // Copyable closure: used by the toggle's Show and the pl toggles.
    let sidebar_closed = move || state.sidebar.get() == SidebarMode::None;

    let show_strip = show.clone();
    let show_bar = show;

    let vs = state.viewer_state();
    let mode = state.viewer.mode;
    let mode_state = state;

    view! {
        // Hot band: the whole 48px reader-top area is the hover trigger + grab
        // zone, always mounted.
        <div
            class="absolute inset-x-0 top-0 z-40 h-12"
            data-tauri-drag-region="true"
            on:mouseenter=move |_| show_strip()
            on:mouseleave=move |_| {
                if !held() {
                    hide_later();
                }
            }
        >
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
                class="toolbar-glass flex h-full items-center gap-2 pr-3 transition-opacity duration-200"
                // pl-20 clears the native lights when they live over this bar
                // (sidebar closed); pl-3 when the sidebar owns the lights.
                class=("pl-20", sidebar_closed)
                class=("pl-3", move || !sidebar_closed())
                class=("opacity-0", move || !visible.get())
                class=("pointer-events-none", move || !visible.get())
            >
                // LEFT GROUP. Drag-region on the wrapper so its gaps drag too.
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=sidebar_closed>
                        <Tooltip text="Toggle sidebar".to_string()>
                            <Button
                                on_click=move |_| state.sidebar.set(SidebarMode::Thumbs)
                                kind=ButtonKind::Ghost
                                icon=IconName::Sidebar
                                title="Toggle sidebar".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Show when=move || {
                        matches!(
                            state.doc.status.get(),
                            DocStatus::Ready | DocStatus::Opening
                        )
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

                // The label span itself carries the drag attribute (see
                // doc_title.rs) so grabbing the filename drags the window.
                <DocTitle state=state />

                // RIGHT GROUP.
                <div
                    id="toolbar-right"
                    data-tauri-drag-region="true"
                    class="ml-auto flex shrink-0 items-center gap-1"
                >
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
                    <MoreMenu state=state open_ext=more_open />
                </div>
            </div>
        </div>
    }
}
