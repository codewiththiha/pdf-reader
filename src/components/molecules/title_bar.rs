//! Hover-reveal titlebar. The h-12 band is ALWAYS mounted: it is the hover
//! trigger AND the grab area (`data-tauri-drag-region`), so the window is
//! draggable from that band even while the bar is invisible. The visible
//! bar fades in inside the band. Visibility is shared (prop signal) so the
//! sidebar identity row and the native traffic lights appear/disappear with
//! it (the reader view syncs `chrome_visible` to `set_traffic_lights`).
//!
//! Grab note: Tauri v2 starts a drag only when the mousedown lands on an
//! element that itself carries `data-tauri-drag-region` (child buttons keep
//! receiving clicks), and it requires the `core:window:allow-start-dragging`
//! capability — see src-tauri/capabilities/default.json.

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
pub fn TitleBar(state: AppState, chrome_visible: RwSignal<bool>) -> impl IntoView {
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
        chrome_visible.set(true);
    });
    let hide_later = move || {
        if let Some(h) = timer.get_value() {
            h.clear();
        }
        let h = set_timeout_with_handle(
            move || chrome_visible.set(false),
            Duration::from_millis(HIDE_DELAY_MS),
        )
        .ok();
        timer.set_value(h);
    };

    // Copyable, so each class toggle below can read it.
    let visible: Signal<bool> = Signal::derive(move || chrome_visible.get());
    let sidebar_open = move || state.sidebar.get() != SidebarMode::None;

    let show_strip = show.clone();
    let show_bar = show;

    let vs = state.viewer_state();
    let mode = state.viewer.mode;
    let mode_state = state;

    view! {
        // Grab + hover band: always mounted, spans the whole window.
        <div
            class="absolute inset-x-0 top-0 z-50 h-12"
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
                // pl-20 clears the native traffic lights (x:20 + ~54px) with a
                // small gap. Tune to pl-[84px] for a touch more air.
                class="toolbar-glass flex h-full items-center gap-2 pl-20 pr-3 transition-opacity duration-200"
                class=("opacity-0", move || !visible.get())
                class=("pointer-events-none", move || !visible.get())
            >
                // LEFT: sidebar toggle (real panel glyph) + name. DocTitle sits
                // OUTSIDE #toolbar-left-pre on purpose: its measurement reads
                // the left group's width, and the group must not contain the
                // label it is sizing (a feedback loop).
                <div class="flex min-w-0 items-center gap-1">
                    <div id="toolbar-left-pre" class="flex shrink-0 items-center gap-1">
                        <Show when=move || state.doc.status.get() == DocStatus::Ready>
                            <Tooltip text="Toggle sidebar".to_string()>
                                <Button
                                    on_click=move |_| {
                                        let next = if sidebar_open() {
                                            SidebarMode::None
                                        } else {
                                            SidebarMode::Thumbs
                                        };
                                        state.sidebar.set(next);
                                    }
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
                    <DocTitle state=state />
                </div>

                // RIGHT: unchanged control set.
                <div id="toolbar-right" class="ml-auto flex shrink-0 items-center gap-1">
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
