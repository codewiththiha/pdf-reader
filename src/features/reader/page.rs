//! The `/reader` route: sidebar + viewer slot + the app title bar, plus the
//! floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::app_shell::app_title_bar::AppTitleBar;
use crate::components::app_shell::document_title::CenteredDocTitle;
use crate::components::app_shell::floating_document_title::FloatingDocumentTitle;
use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::menus::settings_modal::SettingsModal;
use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use crate::components::primitives::hooks::use_timeout::use_hover_visibility;
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::tooltip::Tooltip;
use crate::components::sidebar::Sidebar;
use crate::components::sidebar::document_info::BookInfo;
use crate::components::sidebar::header::SidebarHeader;
use crate::components::sidebar::outline_view::SidebarOutline;
use crate::components::sidebar::shell::{SidebarChromeCtx, request_reveal_active, sidebar_paint};
use crate::components::sidebar::switcher::PanelSwitcher;
use crate::components::sidebar::thumbnails_view::SidebarThumbs;
use crate::components::viewer_controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer_controls::page_indicator::PageIndicator;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::features::reader::use_reader_virtualizers;
use crate::services::document::close_document;
use crate::state::AppState;
use crate::state::SidebarMode;
use pdf_core::layout::{PAGE_GAP, ViewMode};
use pdf_core::math::FitMode;
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;

    let rv = use_reader_virtualizers(vs);

    // Seed margin from persisted settings once the reader mounts.
    {
        let m = state.settings.with_untracked(|st| st.layout.page_margin);
        vs.viewer.page_margin.set(m);
    }

    // No-gap pref → runtime gap + rescale.
    {
        let v = rv.virtualizer.clone();
        Effect::new(move |_| {
            let no_gap = state.settings.with(|st| st.layout.no_gap);
            let gap = if no_gap { 0.0 } else { PAGE_GAP };
            if (vs.viewer.page_gap.get_untracked() - gap).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_gap.set(gap);
            let heights = vs.document.metrics.css_heights.with_untracked(|h| h.clone());
            v.rescale(1.0, move |i| heights.get(i).copied().unwrap_or(0.0) + gap);
        });
    }

    // Page margin pref — cross-axis for vertical, main-axis for horizontal.
    {
        let (v, hv) = (rv.virtualizer.clone(), rv.h_virtualizer.clone());
        Effect::new(move |_| {
            let m = state.settings.with(|st| st.layout.page_margin);
            if (vs.viewer.page_margin.get_untracked() - m).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_margin.set(m);
            let scale = vs.viewer.zoom.committed.get_untracked();
            let gap = vs.viewer.page_gap.get_untracked();
            let heights = vs.document.metrics.css_heights.with_untracked(|h| h.clone());
            let widths = vs
                .document
                .metrics
                .intrinsic
                .with_untracked(|w| w.iter().map(|s| s.width).collect::<Vec<f64>>());
            // Vertical: margin is cross-axis; sizes unchanged aside from gap.
            v.rescale(1.0, move |i| heights.get(i).copied().unwrap_or(0.0) + gap);
            // Horizontal: margin is main-axis.
            hv.rescale(1.0, move |i| widths.get(i).copied().unwrap_or(0.0) * scale + 2.0 * m);
        });
    }

    let prev_mode = StoredValue::new(vs.viewer.mode.get_untracked());
    Effect::new(move |_| {
        let mode = vs.viewer.mode.get();
        let prev = prev_mode.get_value();
        if mode == prev {
            return;
        }
        prev_mode.set_value(mode);
        let auto = state.settings.with(|s| s.layout.auto_scale);
        if matches!(mode, ViewMode::Spread | ViewMode::ScrollHorizontal) || (auto && mode.is_paginated()) {
            vs.viewer.fit.set(FitMode::Width);
        }
    });

    let engine = crate::viewer::engine::ViewerEngine::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom controller is created and driven here, and lives exactly as
    // long as this page's reactive owner. Everything downstream only posts
    // commands; nothing else writes a zoom scale or rescales a strip.
    let zoom = crate::viewer::zoom::ZoomController::new(engine);
    zoom.drive(vs);
    navigation_sync(vs, rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom sources come last, after the controller that consumes them:
    // fit recomputation on window/mode/page/margin changes, and the
    // shrink-to-fit re-resolution on resizes of a manually zoomed reader.
    crate::effects::reader::zoom_watchers::fit_watcher(vs, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::resize_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    reading_progress(state);

    let status = state.reader.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;
    let paint = sidebar_paint(state.ui.sidebar);

    let overlay_sb = Signal::derive(move || state.settings.with(|st| st.layout.sidebar_overlay));
    let sb_hover = use_hover_visibility(Duration::from_millis(250), move || !overlay_sb.get());
    let last_panel = RwSignal::new(SidebarMode::Thumbs);
    Effect::new(move |_| {
        let m = state.ui.sidebar.get();
        if m != SidebarMode::None {
            last_panel.set(m);
        }
    });
    let prev_vis = StoredValue::new_local(false);
    Effect::new(move |_| {
        // edge-triggered hover open/close
        let vis = sb_hover.visible.get();
        let was = prev_vis.get_value();
        prev_vis.set_value(vis);
        if !overlay_sb.get() {
            return;
        }
        if vis && !was && state.ui.sidebar.get() == SidebarMode::None {
            state.ui.sidebar.set(last_panel.get());
        } else if !vis && was && state.ui.sidebar.get() != SidebarMode::None {
            state.ui.sidebar.set(SidebarMode::None);
        }
    });
    provide_context(SidebarChromeCtx {
        present: Signal::derive(move || paint.present.get() && !overlay_sb.get()),
    });

    let sb_request_show = RwSignal::new(0u32);
    let sb_request_hide = RwSignal::new(0u32);

    let show_fn = sb_hover.show.clone();
    Effect::new(move |_| {
        if sb_request_show.get() > 0 {
            show_fn();
        }
    });
    let hide_fn = sb_hover.hide_later.clone();
    Effect::new(move |_| {
        if sb_request_hide.get() > 0 {
            hide_fn();
        }
    });

    let settings_open = RwSignal::new(false);
    let show_indicator = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || state.settings.with(|st| st.layout.progress_bar));

    // Left: sidebar toggle + Library stay put. Open is gone. Title is centered;
    // right is appearance + settings + the 3-dash view menu.
    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=move || state.ui.sidebar.get() == SidebarMode::None>
                    <Tooltip text="Toggle sidebar">
                        <Button
                            on_click=move |_| state.ui.sidebar.set(SidebarMode::Thumbs)
                            variant=ButtonVariant::Ghost
                            title="Toggle sidebar"
                        >
                            <Icon name=IconName::Sidebar size=18 />
                        </Button>
                    </Tooltip>
                </Show>
                <Show when=move || {
                    matches!(
                        state.reader.document.status.get(),
                        DocStatus::Ready | DocStatus::Opening
                    )
                }>
                    <Tooltip text="Library">
                        <Button
                            on_click=move |_| close_document(state)
                            variant=ButtonVariant::Ghost
                            title="Close this book and return to the library"
                        >
                            <Icon name=IconName::Library size=18 />
                        </Button>
                    </Tooltip>
                </Show>
            </div>
        }
    };
    let center = move || view! { <CenteredDocTitle state=state /> };
    let right = move || {
        view! {
            <AppearanceMenu state=state />
            <IconButton
                icon=IconName::Settings
                title="Reader settings"
                on_click=move || settings_open.set(true)
            />
            <ReaderMenu state=state settings_open=settings_open />
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            // overflow-hidden clips the hidden ReaderBottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("blend", move || state.settings.with(|st| st.layout.blend_mode))
            >
                <div class="relative flex min-h-0 flex-1">
                    <Show when=move || overlay_sb.get() && state.ui.sidebar.get() == SidebarMode::None>
                        <div
                            class="absolute inset-y-0 left-0 z-[var(--z-bar)] w-1.5"
                            on:mouseenter=move |_| sb_request_show.update(|n| *n += 1)
                        />
                    </Show>
                    <div
                        class=move || {
                            if overlay_sb.get() {
                                "absolute inset-y-0 left-0 z-[var(--z-popover)] shadow-2xl transition-transform duration-300 ease-in-out"
                            } else {
                                "contents"
                            }
                        }
                        class=(
                            "-translate-x-full",
                            move || overlay_sb.get() && state.ui.sidebar.get() == SidebarMode::None,
                        )
                        on:mouseenter=move |_| sb_request_show.update(|n| *n += 1)
                        on:mouseleave=move |_| sb_request_hide.update(|n| *n += 1)
                    >
                        <Sidebar
                            mode=state.ui.sidebar
                            header=move || view! { <SidebarHeader reader=vs sidebar=state.ui.sidebar /> }
                            info_row=move || view! { <BookInfo reader=vs covers=state.library.covers /> }
                            panels=move || view! {
                                <SidebarOutline
                                    state=vs
                                    sidebar=state.ui.sidebar
                                    shown=paint.show_outline
                                    outro=paint.is_closed
                                    intro=paint.intro
                                />
                                <SidebarThumbs
                                    state=vs
                                    sidebar=state.ui.sidebar
                                    // Cells mount with the aside. Cached thumbs
                                    // blit during the slide; cold cells retain their
                                    // skeleton until their capped render completes.
                                    live=Signal::derive(move || paint.thumbs_live.get())
                                    shown=paint.show_thumbs
                                    outro=paint.is_closed
                                    intro=paint.intro
                                />
                            }
                            footer=move || view! {
                                <PanelSwitcher
                                    mode=state.ui.sidebar
                                    thumbs_active=paint.thumbs_active
                                    outline_active=paint.outline_active
                                    on_reveal=request_reveal_active
                                />
                            }
                        />
                    </div>
                    <main
                        id=VIEWER_SLOT_ID
                        class="relative min-w-0 flex-1 overflow-hidden"
                        class=("no-page-shadow", move || !state.settings.with(|st| st.layout.page_shadow))
                    >
                        <Show when=is_ready>
                            <crate::components::document::Viewer
                                state=vs
                                virtualizer=rv.virtualizer_view.get_value()
                                h_virtualizer=rv.h_virtualizer_view.get_value()
                                progress_visible=progress_visible
                            />
                        </Show>
                        <FloatingDocumentTitle state=state />
                        // Corner page counter, gated on a ready document and
                        // positioned by the page; the indicator itself is
                        // reusable UI with no knowledge of AppState.
                        <Show when=move || is_ready() && show_indicator.get()>
                            <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", crate::components::primitives::floating::types::z::CONTROLS)>
                                <PageIndicator
                                    current=state.reader.viewer.page
                                    total=state.reader.document.num_pages
                                    style=indicator_style
                                    hidden=Signal::derive(move || state.reader.gloss.selection_active.get())
                                />
                            </div>
                        </Show>
                        <SettingsModal state=state open=settings_open />
                        <ReaderBottomBar
                            reader=vs
                        />
                        <crate::components::search::floating_search::FloatingSearch
                            state=vs
                            virtualizer=rv.virtualizer_view
                        />
                        <crate::components::ai::selection_menu::SelectionMenu state=state />
                        <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
                    </main>
                </div>
            </div>
        </AppTitleBar>
    }
}
