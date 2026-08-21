//! The application's component system, organized by what a component is
//! used for:
//!
//!   * `shared`   — generic UI (button, icon, popover, ...); must never
//!     know what a PDF reader is
//!   * `layout`   — reusable structural application chrome (title bar,
//!     adaptive toolbar, document titles)
//!   * `menus`    — menu features (appearance, more)
//!   * `overlays` — transient UI (toast, drag feedback)
//!   * `reader`   — reader-only controls (zoom, page indicator, ...)
//!   * `sidebar`  — the app sidebar (composition shell + panel hosts)
//!   * `pdf`      — UI whose purpose is displaying PDF documents
//!   * `search`   — search presentation shared by reader surfaces
//!
//! Import discipline: callers import from the owning group
//! (`use crate::components::shared::Button`), which keeps each
//! component's origin visible. `shared` must not reach upward into
//! `state`/`services`/`effects`/`pdf_engine`.

mod dom;
pub mod layout;
pub mod menus;
pub mod overlays;
pub mod pdf;
pub mod reader;
pub mod search;
pub mod shared;
pub mod sidebar;
