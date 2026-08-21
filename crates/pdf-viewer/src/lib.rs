//! Reusable Leptos PDF viewer: page canvases, continuous/single views,
//! thumbnails, outline, search overlay, and their effects. Depends on
//! `pdf-core` (pure math) and `pdf-engine` (the pdf.js bridge) — never on app
//! chrome or Tauri.
//!
//! The module tree below is private. The public API is this flat set of
//! re-exports, so the crate can reorganize internally without breaking
//! consumers.

mod components;
mod dom;
mod effects;
mod state;

pub use components::navigation::page_navigation::PageNavigation;
pub use components::pages::continuous::ContinuousView;
pub use components::pages::page_canvas::PageCanvas;
pub use components::pages::page_list::PageList;
pub use components::pages::single_page::SinglePageView;
pub use components::search::floating::FloatingSearch;
pub use components::search::panel::ResultList;
pub use components::shared::button::{Button, ButtonKind};
pub use components::shared::hue_picker::HuePicker;
pub use components::shared::icon::{Icon, IconName};
pub use components::shared::kbd::Kbd;
pub use components::shared::segmented::{Segmented, SegmentedLabel};
pub use components::shared::separator::Separator;
pub use components::shared::slider::Slider;
pub use components::shared::tooltip::Tooltip;
pub use components::sidebar::outline::OutlinePanel;
pub use components::sidebar::thumbnails::ThumbnailsPanel;
pub use dom::{by_id, page_list};
pub use effects::fit::fit_effect;
pub use effects::page_tracking::page_tracking;
pub use effects::search_effects::{dismiss_search, resume_search};
pub use effects::shortcuts::shortcuts;
pub use effects::zoom::{request_zoom, zoom_system};
pub use state::{
    DocumentState, SearchState, SidebarMode, TextureSignal, ViewerSignals, ViewerState,
};
