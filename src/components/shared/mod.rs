//! Reusable UI primitives shared across the app: generic controls (button,
//! icon, slider, ...), the collision-aware toolbar group, the window-aware
//! popover container, and the shared option/menu building blocks.

pub mod adaptive_group;
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

pub(crate) use button::{Button, ButtonKind};
pub(crate) use hue_picker::HuePicker;
pub(crate) use icon::{Icon, IconName};
pub(crate) use kbd::Kbd;
pub(crate) use segmented::{Segmented, SegmentedLabel};
pub(crate) use separator::Separator;
pub(crate) use slider::Slider;
pub(crate) use tooltip::Tooltip;
