//! The `/reader` route: sidebar + viewer slot + the shared titlebar (via the
//! chrome `TitleBar`), plus the floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use crate::components::shared::button::{Button, ButtonKind};
use crate::components::shared::icon::{Icon, IconName};
use crate::components::shared::segmented::{Segmented, SegmentedLabel};
use crate::components::shared::tooltip::Tooltip;

use pdf_engine::types::DocStatus;
use pdf_core::layout::{DocumentLayout, PAGE_GAP, ViewMode};
use pdf_core::math::FitMode;
use crate::components::layout::adaptive_toolbar::{AdaptiveToolbar, ToolbarItem};
use crate::components::sidebar::Sidebar;
use crate::components::layout::document_title::FloatingDocumentTitle;
use crate::components::layout::title_bar::AppTitleBar;
use crate::components::menus::appearance::appearance_entry;
use crate::components::layout::document_title::DocumentTitle;
use crate::components::reader::page_indicator::PageIndicator;
use crate::components::reader::reader_controls::ReaderControls;
use crate::components::reader::zoom_controls::zoom_entries;
use crate::services::document::{close_document, open_dialog};
use crate::state::AppState;
use crate::effects::reading_progress::reading_progress;
use crate::effects::fit::fit_effect;
use crate::effects::page_tracking::page_tracking;
use crate::effects::zoom::zoom_system;
use crate::state::SidebarMode;

fn view_mode_entry(state: AppState) -> ToolbarItem {
    let mode = state.reader.viewer.mode;

    // ── inline (what the bar shows) ──────────────────────────────
    // Compact icon-only segmented — same as before.
    let inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <Tooltip text="View mode".to_string()>
                <Segmented
                    options=vec![
                        (ViewMode::Single,
                         SegmentedLabel::Icon(IconName::SinglePage),
                         "Single page view"),
                        (ViewMode::Continuous,
                         SegmentedLabel::Icon(IconName::Continuous),
                         "Continuous scroll view"),
                    ]
                    value={mode.read_only()}
                    on_change=move |m: ViewMode| state.reader.viewer.mode.set(m)
                />
            </Tooltip>
        }
        .into_any()
    });

    ToolbarItem {
        id: "view-mode",
        // collapses AFTER fit (70), BEFORE zoom-step (80)
        priority: 75,
        keep_mounted: false,
        inline: inline.clone(),
        sizer: inline,
        // Menu: full-width segmented WITH text labels; picking one closes the menu.
        collapsed: Arc::new(move |done| {
            view! {
                <div class="w-full px-1 py-1">
                    <Segmented
                        full_width=true
                        options=vec![
                            (ViewMode::Single,
                             SegmentedLabel::IconText(IconName::SinglePage, "Single"),
                             "Single page view"),
                            (ViewMode::Continuous,
                             SegmentedLabel::IconText(IconName::Continuous, "Continuous"),
                             "Continuous scroll view"),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| {
                            state.reader.viewer.mode.set(m);
                            done.run(());
                        }
                    />
                </div>
            }
            .into_any()
        }),
    }
}

fn fit_entry(state: AppState) -> ToolbarItem {
    let inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <div class="flex items-center gap-1">
                <Tooltip text="Fit width (Cmd/Ctrl+0)".to_string()>
                    <Button
                        on_click=move |_| state.reader.viewer.fit.set(FitMode::Width)
                        kind=ButtonKind::Ghost
                        icon=IconName::FitWidth
                        title="Fit width (Cmd/Ctrl+0)".to_string()
                    />
                </Tooltip>
                <Tooltip text="Fit page".to_string()>
                    <Button
                        on_click=move |_| state.reader.viewer.fit.set(FitMode::Page)
                        kind=ButtonKind::Ghost
                        icon=IconName::FitPage
                        title="Fit page".to_string()
                    />
                </Tooltip>
            </div>
        }
        .into_any()
    });

    ToolbarItem {
        id: "fit",
        // collapses FIRST of the layout trio
        priority: 70,
        keep_mounted: false,
        inline: inline.clone(),
        sizer: inline,
        // Menu: two equal-half buttons; clicking closes the menu.
        collapsed: Arc::new(move |done| {
            view! {
                <div class="flex w-full items-center gap-1 px-1 py-1">
                    <button type="button"
                        on:click=move |_| {
                            state.reader.viewer.fit.set(FitMode::Width);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitWidth size=14 />
                        <span>"Fit width"</span>
                    </button>
                    <button type="button"
                        on:click=move |_| {
                            state.reader.viewer.fit.set(FitMode::Page);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitPage size=14 />
                        <span>"Fit page"</span>
                    </button>
                </div>
            }
            .into_any()
        }),
    }
}

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;
    // The ONE cached column layout for this reader session: every scroll,
    // render-window, zoom-anchor and jump query borrows it instead of
    // rebuilding the strip's prefix sums from the raw heights.
    let layout = Memo::new(move |_| DocumentLayout::new(&vs.document.page_heights.get(), PAGE_GAP));
    fit_effect(vs, state.ui.sidebar, layout);
    zoom_system(vs, layout);
    page_tracking(vs, layout);
    reading_progress(state);

    let status = state.reader.document.status;
    let mode = state.reader.viewer.mode;
    let state_sidebar = state;
    let is_ready = move || status.get() == DocStatus::Ready;

    let appearance_open = RwSignal::new(false);
    let collapsed_ids = RwSignal::new(Vec::<&'static str>::new());
    let overflow_ref: NodeRef<html::Div> = NodeRef::new();

    // AdaptiveToolbar is generic UI: it gets a ready flag and a refresh
    // counter instead of AppState. The page bumps the counter whenever
    // chrome state that affects the bar's geometry changes.
    let toolbar_ready = Signal::derive(move || status.get() == DocStatus::Ready);
    let toolbar_refresh = RwSignal::new(0u64);
    Effect::new(move |_| {
        _ = state.ui.sidebar.get();
        _ = state.reader.document.status.get();
        _ = state.reader.document.num_pages.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        toolbar_refresh.update(|n| *n += 1);
    });

    // LEFT slot: sidebar toggle (only while the sidebar is closed — the
    // sidebar's own chrome row owns it otherwise), Library, Open, DocumentTitle.
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=move || state.ui.sidebar.get() == SidebarMode::None>
                        <Tooltip text="Toggle sidebar".to_string()>
                            <Button
                                on_click=move |_| state.ui.sidebar.set(SidebarMode::Thumbs)
                                kind=ButtonKind::Ghost
                                icon=IconName::Sidebar
                                title="Toggle sidebar".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Show when=move || {
                        matches!(
                            state.reader.document.status.get(),
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
                <DocumentTitle state=state />
            </div>
        }
    };

    // RIGHT slot: view-mode, fit, zoom, appearance — collision-aware overflow.
    let right = move || {
        let mut entries = vec![view_mode_entry(state), fit_entry(state)];
        entries.extend(zoom_entries(state));          // zoom-step (80), readout (MAX)
        entries.push(appearance_entry(
            state,
            appearance_open,
            collapsed_ids,
            overflow_ref,
        ));                                          // MAX
        view! {
            <AdaptiveToolbar
                ready=toolbar_ready
                refresh=toolbar_refresh.into()
                entries=entries
                collapsed_ids=collapsed_ids
                overflow_ref=overflow_ref
            />
        }
    };

    view! {
        <AppTitleBar state=state left=left right=right>
            // overflow-hidden clips the hidden ReaderControls's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div class="relative flex h-full w-full flex-col overflow-hidden bg-paper text-ink">
                <div class="flex min-h-0 flex-1">
                    <Sidebar state=state_sidebar />
                    <main id="viewer-slot" class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <crate::components::pdf::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <crate::components::pdf::ContinuousView state=vs layout=layout />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocumentTitle state=state />
                        // Corner page counter, gated on a ready document and
                        // positioned by the page; the indicator itself is
                        // reusable UI with no knowledge of AppState.
                        <Show when=is_ready>
                            <div class="pointer-events-none absolute bottom-3 right-3 z-30">
                                <PageIndicator current=state.reader.viewer.page total=state.reader.document.num_pages />
                            </div>
                        </Show>
                        <ReaderControls state=state layout=layout />
                        // Floating search overlay (U4): mounted at the viewer
                        // slot; its top-14 offset clears the titlebar.
                        <crate::components::search::floating::FloatingSearch state=vs />
                    </main>
                </div>
            </div>
        </AppTitleBar>
    }
}
