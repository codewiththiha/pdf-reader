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
pub mod menus;
pub mod overlays;
pub mod pdf;
pub mod reader;
pub mod shared;
pub mod sidebar;

// Shared primitives.
pub use shared::adaptive_group::{AdaptiveGroup, OverflowRow, ToolbarEntry};
pub use shared::menu_item::MenuItem;
pub use shared::option_button::OptionButton;
pub use shared::popover::Popover;
pub use shared::{
    Button, ButtonKind, HuePicker, Icon, IconName, Kbd, Segmented, SegmentedLabel, Separator,
    Slider, Tooltip,
};

// Window chrome.
pub use chrome::floating_title::{DocumentTitle, FloatingTitle};
pub use chrome::title_bar::{TitleBar, TitleBarCtx};

// Menus.
pub use menus::appearance::{appearance_entry, AppearanceMenu};
pub use menus::more::MoreMenu;

// Overlays.
pub use overlays::drag_overlay::DragOverlay;
pub use overlays::toast::ToastHost;

// Reader controls.
pub use reader::page_indicator::PageIndicator;
pub use reader::reader_controls::ReaderControls;
pub use reader::zoom_controls::zoom_entries;

// Sidebar.
pub use sidebar::Sidebar;

// PDF viewing.
pub use pdf::{
    ContinuousView, FloatingSearch, OutlinePanel, PageCanvas, PageList, PageNavigation,
    SinglePageView, ThumbnailsPanel,
};
