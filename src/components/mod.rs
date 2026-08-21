//! The application's component system, organized by what a component is
//! used for:
//!
//!   * `shared`   — generic UI (button, icon, popover, ...)
//!   * `chrome`   — window chrome (title bar, floating title)
//!   * `menus`    — menu features (appearance, more)
//!   * `overlays` — transient UI (toast, drag feedback)
//!   * `reader`   — reader-only controls (zoom, page indicator, ...)
//!   * `sidebar`  — the app sidebar
//!   * `pdf`      — UI whose purpose is displaying PDF documents
//!
//! The public pieces are re-exported here, so callers get a logical API
//! without caring which folder owns a component.

pub mod chrome;
mod metrics;
pub mod menus;
pub mod overlays;
pub mod pdf;
pub mod reader;
pub mod shared;
pub mod sidebar;

// Shared primitives.
pub(crate) use shared::adaptive_group::{AdaptiveGroup, OverflowRow, ToolbarEntry};
pub(crate) use shared::menu_item::MenuItem;
pub(crate) use shared::option_button::OptionButton;
pub(crate) use shared::popover::Popover;
pub(crate) use shared::{
    Button, ButtonKind, HuePicker, Icon, IconName, Kbd, Segmented, SegmentedLabel, Separator,
    Slider, Tooltip,
};

// Window chrome.
pub(crate) use chrome::floating_title::{DocumentTitle, FloatingDocumentTitle};
pub(crate) use chrome::title_bar::{AppTitleBar, TitleBarCtx};

// Menus.
pub(crate) use menus::appearance::{appearance_entry, AppearanceMenu};
pub(crate) use menus::more::MoreMenu;

// Overlays.
pub(crate) use overlays::drag_overlay::DragOverlay;
pub(crate) use overlays::toast::ToastHost;

// Reader controls.
pub(crate) use reader::page_indicator::PageIndicator;
pub(crate) use reader::reader_controls::ReaderControls;
pub(crate) use reader::zoom_controls::zoom_entries;

// Sidebar.
pub(crate) use sidebar::Sidebar;

// PDF viewing.
pub(crate) use pdf::{
    ContinuousView, FloatingSearch, OutlinePanel, PageCanvas, PageList, PageNavigation,
    SinglePageView, ThumbnailsPanel,
};
