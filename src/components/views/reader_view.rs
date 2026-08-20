//! The `/reader` route: sidebar + viewer slot + the shared titlebar (via the
//! chrome `TitleBarProvider`), plus the floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::IconName;
use pdf_viewer::components::atoms::segmented::{Segmented, SegmentedLabel};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;

use pdf_engine::types::DocStatus;
use pdf_core::layout::ViewMode;
use crate::components::chrome::floating_doc_title::FloatingDocTitle;
use crate::components::chrome::titlebar_provider::TitleBarProvider;
use crate::components::molecules::appearance_menu::AppearanceMenu;
use crate::components::molecules::doc_title::DocTitle;
use crate::components::molecules::more_menu::MoreMenu;
use crate::components::molecules::zoom_controls::ZoomControls;
use crate::core::open_flow::{close_document, open_dialog};
use crate::core::state::AppState;
use crate::effects::reading_progress::reading_progress;
use pdf_viewer::effects::fit::{fit_effect, zoom_system};
use pdf_viewer::effects::page_tracking::page_tracking;
use pdf_viewer::state::SidebarMode;

#[component]
pub fn ReaderView(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.viewer_state();
    fit_effect(vs);
    zoom_system(vs);
    page_tracking(vs);
    reading_progress(state);

    let status = state.doc.status;
    let mode = state.viewer.mode;
    let state_sidebar = state;
    let is_ready = move || status.get() == DocStatus::Ready;

    // LEFT slot: sidebar toggle (only while the sidebar is closed — the
    // sidebar's own chrome row owns it otherwise), Library, Open, DocTitle.
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=move || state.sidebar.get() == SidebarMode::None>
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
                <DocTitle state=state />
            </div>
        }
    };

    // RIGHT slot: search, view mode, zoom, appearance, more.
    let right = move || {
        view! {
            <div
                id="toolbar-right"
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
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
                        on_change=move |m: ViewMode| state.viewer.mode.set(m)
                    />
                </Tooltip>
                <ZoomControls state=state />
                <Separator vertical=true />
                <AppearanceMenu state=state />
                <MoreMenu state=state />
            </div>
        }
    };

    view! {
        <TitleBarProvider state=state left=left right=right>
            // overflow-hidden clips the hidden BottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div class="relative flex h-full w-full flex-col overflow-hidden bg-paper text-ink">
                <div class="flex min-h-0 flex-1">
                    <crate::components::organisms::sidebar::Sidebar state=state_sidebar />
                    <main id="viewer-slot" class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <pdf_viewer::components::single_page_view::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <pdf_viewer::components::continuous_view::ContinuousView state=vs />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocTitle state=state />
                        <crate::components::molecules::page_pill::PagePill state=state />
                        <crate::components::molecules::bottom_bar::BottomBar state=state />
                        // Floating search overlay (U4): mounted at the viewer
                        // slot; its top-14 offset clears the titlebar.
                        <pdf_viewer::components::floating_search::FloatingSearch state=vs />
                    </main>
                </div>
            </div>
        </TitleBarProvider>
    }
}


