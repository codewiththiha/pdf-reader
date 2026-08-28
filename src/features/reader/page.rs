//! The `/reader` route: sidebar + viewer slot + the app title bar, plus the
//! floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use leptos::prelude::*;
use virtual_list::Viewport;
use virtual_list_leptos::{VirtualizerOptions, use_virtualizer};

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
use crate::effects::reader::fit_mode::fit_effect;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::services::document::close_document;
use crate::state::AppState;
use crate::state::SidebarMode;
use pdf_core::layout::{PAGE_GAP, RENDER_BUDGET, ViewMode};
use pdf_core::math::FitMode;
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;

    // Keep `css_heights` fully seeded from intrinsic sizes: it is the shared
    // measurement store backing the virtualizer and zoom-rescale path, not a
    // second layout model.
    Effect::new(move || {
        let count = vs.document.num_pages.get() as usize;
        let scale = vs.viewer.zoom.render.get();
        let empty_intrinsic = vs.document.metrics.intrinsic.with(|sizes| sizes.is_empty());
        let fallback = vs
            .document
            .page1_size
            .get()
            .map(|size| size.height)
            .unwrap_or(0.0);
        if count == 0 || scale <= 0.0 || (empty_intrinsic && fallback <= 0.0) {
            return;
        }
        vs.document.metrics.css_heights.update(|heights| {
            if heights.len() == count {
                return;
            }
            *heights = vs.document.metrics.intrinsic.with(|sizes| {
                (0..count)
                    .map(|index| {
                        sizes
                            .get(index)
                            .map(|size| size.height)
                            .filter(|height| *height > 0.0)
                            .unwrap_or(fallback)
                            * scale
                    })
                    .collect()
            });
        });
    });

    let count = Signal::derive(move || vs.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        let measured = vs
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied());
        if let Some(height) = measured.filter(|height| *height > 0.0) {
            return height + vs.viewer.page_gap.get_untracked();
        }
        let intrinsic = vs
            .document
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        let fallback = vs
            .document
            .page1_size
            .get_untracked()
            .map(|size| size.height)
            .unwrap_or(0.0);
        intrinsic.unwrap_or(fallback) * vs.viewer.zoom.render.get_untracked()
            + vs.viewer.page_gap.get_untracked()
    };
    let epoch = Signal::derive(move || {
        let count = vs.document.num_pages.get() as usize;
        let mut hasher = DefaultHasher::new();
        count.hash(&mut hasher);
        vs.document.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(&mut hasher);
            for size in sizes {
                size.width.to_bits().hash(&mut hasher);
                size.height.to_bits().hash(&mut hasher);
            }
        });
        hasher.finish()
    });
    let pinned_sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
    let initial_vh = {
        let (_, height) = vs.viewer.container_size.get_untracked();
        if height > 1.0 { height } else { 800.0 }
    };
    let virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, estimate)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .initial(Viewport::main_only(initial_vh), 0.0)
            .pinned(pinned_sig.into())
            .epoch(epoch),
    );

    // Horizontal virtualizer: created unconditionally (hook), bound only when the view mounts.
    let h_estimate = move |index: usize| {
        vs.document.metrics.intrinsic.with_untracked(|sizes| {
            sizes.get(index).map(|s| s.width).unwrap_or(0.0)
        }) * vs.viewer.zoom.layout.get_untracked()
            + 2.0 * vs.viewer.page_margin.get_untracked()
    };
    let h_virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, h_estimate)
            .axis(virtual_list_leptos::Axis::Horizontal)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .padding(0.0, 0.0)
            .initial(Viewport::new(1200.0, initial_vh), 0.0)
            .epoch(epoch),
    );
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer.clone());

    // The engine only sweeps its rasters inside render activity; after a
    // zoom-out or a mode flip nothing renders, so the big rasters would stay
    // pinned until the 30s idle timer. Sweep the moment scrolling settles
    // instead — both virtualizers, registered once (the views rebind the
    // SAME shared virtualizer on every mode flip).
    virtualizer.on_scroll_idle(|| pdf_engine::api::sweep());
    h_virtualizer.on_scroll_idle(|| pdf_engine::api::sweep());

    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            let mut pin = None;
            if vs.viewer.zoom_animating.get() {
                let dominant = v.dominant().get();
                pin = Some((dominant, dominant));
            }
            if let Some((first, last)) = vs.viewer.selected_pages.get() {
                let selected = (
                    first.saturating_sub(1) as usize,
                    last.saturating_sub(1) as usize,
                );
                pin = Some(match pin {
                    Some((a, b)) => (a.min(selected.0), b.max(selected.1)),
                    None => selected,
                });
            }
            pinned_sig.set(pin);
        });
    }

    // Seed margin from persisted settings once the reader mounts.
    {
        let m = state.settings.with_untracked(|st| st.layout.page_margin);
        vs.viewer.page_margin.set(m);
    }

    // No-gap pref → runtime gap + rescale.
    {
        let v = virtualizer.clone();
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
        let (v, hv) = (virtualizer.clone(), h_virtualizer.clone());
        Effect::new(move |_| {
            let m = state.settings.with(|st| st.layout.page_margin);
            if (vs.viewer.page_margin.get_untracked() - m).abs() < 1e-9 {
                return;
            }
            vs.viewer.page_margin.set(m);
            let scale = vs.viewer.zoom.layout.get_untracked();
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

    let engine = crate::viewer::engine::ViewerEngine::new(virtualizer.clone(), h_virtualizer.clone());
    fit_effect(vs, state.ui.sidebar, engine.clone());
    crate::viewer::resize_constraint::resize_constraint_effect(state, engine.clone());
    let zoom = crate::viewer::zoom::ZoomController::new(engine);
    crate::viewer::zoom::register(&zoom);
    zoom.drive(vs);
    navigation_sync(vs, virtualizer.clone(), h_virtualizer.clone());
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    let virtualizer_view = StoredValue::new_local(virtualizer.clone());
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
                                virtualizer=virtualizer_view.get_value()
                                h_virtualizer=h_virtualizer_view.get_value()
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
                            virtualizer=virtualizer_view
                        />
                        <crate::components::ai::selection_menu::SelectionMenu state=state />
                        <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
                    </main>
                </div>
            </div>
        </AppTitleBar>
    }
}
