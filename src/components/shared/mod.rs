//! Reusable UI primitives shared across the app: generic controls (button,
//! icon, slider, ...), the window-aware popover container, and the shared
//! option/menu building blocks.
//!
//! Contract: `shared` components must not know what a PDF reader is. No
//! `crate::state`, `crate::services`, `crate::effects`, or `pdf_engine`
//! imports below this point.

pub mod button;
pub mod hue_picker;
pub mod icon;
pub mod kbd;
pub mod menu_item;
pub mod option_button;
pub mod popover;
pub mod segmented;
pub mod separator;
pub mod slider;
pub mod tooltip;
