//! The single owner of the relationship between pages, scroll position, and
//! scale.
//!
//! Before this module, views, zoom, fit, and navigation each reached into the
//! virtualizers and the DOM directly, and zoom duplicating the axis-branching
//! relayout across two code paths was the root of the races. Everything in the
//! viewer that must move geometry (rescale a strip, jump to a page, report a
//! rendered size) goes through [`ViewerEngine`]; nothing outside it should
//! touch a virtualizer.

pub mod engine;
pub mod zoom;
