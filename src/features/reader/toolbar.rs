//! The reader titlebar's collision-aware entries: view mode, fit, zoom,
//! appearance. The route composes the toolbar; this module owns the item
//! definitions so `page.rs` stays a coordinator, not a toolbar factory.

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use crate::components::layout::adaptive_toolbar::ToolbarItem;
use crate::components::menus::appearance::appearance_entry;
use crate::components::reader::zoom_controls::zoom_entries;
use crate::components::shared::button::{Button, ButtonVariant};
use crate::components::shared::icon::{Icon, IconName};
use crate::components::shared::segmented::{SegmentOption, Segmented, SegmentedLabel};
use crate::components::shared::tooltip::Tooltip;
use crate::state::AppState;

fn view_mode_entry(state: AppState) -> ToolbarItem {
    let mode = state.reader.viewer.mode;

    // ── inline (what the bar shows) ──────────────────────────────
    // Compact icon-only segmented — same as before.
    let inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <Tooltip text="View mode">
                <Segmented
                    options=vec![
                        SegmentOption {
                            value: ViewMode::Single,
                            label: SegmentedLabel::Icon(IconName::SinglePage),
                            title: "Single page view",
                        },
                        SegmentOption {
                            value: ViewMode::Continuous,
                            label: SegmentedLabel::Icon(IconName::Continuous),
                            title: "Continuous scroll view",
                        },
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
                            SegmentOption {
                                value: ViewMode::Single,
                                label: SegmentedLabel::IconText(IconName::SinglePage, "Single"),
                                title: "Single page view",
                            },
                            SegmentOption {
                                value: ViewMode::Continuous,
                                label: SegmentedLabel::IconText(IconName::Continuous, "Continuous"),
                                title: "Continuous scroll view",
                            },
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
                <Tooltip text="Fit width (Cmd/Ctrl+0)">
                    <Button
                        on_click=move |_| state.reader.viewer.fit.set(FitMode::Width)
                        variant=ButtonVariant::Ghost
                        title="Fit width (Cmd/Ctrl+0)"
                    >
                        <Icon name=IconName::FitWidth size=18 />
                    </Button>
                </Tooltip>
                <Tooltip text="Fit page">
                    <Button
                        on_click=move |_| state.reader.viewer.fit.set(FitMode::Page)
                        variant=ButtonVariant::Ghost
                        title="Fit page"
                    >
                        <Icon name=IconName::FitPage size=18 />
                    </Button>
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

/// The reader bar's entries, in collapse-priority order: view mode, fit,
/// zoom step + readout, appearance (essential).
pub fn reader_toolbar_entries(
    state: AppState,
    appearance_open: RwSignal<bool>,
    collapsed_ids: RwSignal<Vec<&'static str>>,
    overflow_ref: NodeRef<html::Div>,
) -> Vec<ToolbarItem> {
    let mut entries = vec![view_mode_entry(state), fit_entry(state)];
    entries.extend(zoom_entries(state));          // zoom-step (80), readout (MAX)
    entries.push(appearance_entry(
        state,
        appearance_open,
        collapsed_ids,
        overflow_ref,
    ));                                          // MAX
    entries
}
