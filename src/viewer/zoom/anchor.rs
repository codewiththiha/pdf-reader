//! THE zoom focus and the stage origin: the two things a zoom transaction
//! has to know about position.
//!
//! The focus is a document-logical position captured before a zoom changes
//! any geometry and restored after the new geometry commits. It is
//! deliberately NOT built from the virtualizer's dominant item — that is
//! the value most likely to move while a transaction is in flight. It is
//! built from `viewer.page` plus the actual scroll position and the actual
//! page geometry, expressed as fractions, so it stays valid even if pages
//! mount and unmount around it. There is exactly ONE focus per
//! transaction: the page under the viewport centre. The virtualizer is
//! then just the tool that translates the logical focus back into a
//! physical scroll offset at commit time.
//!
//! The stage origin is the presentation-side counterpart: the content point
//! that sits under the viewport centre when the transaction opens, and that
//! the stage transform pivots on. Capture and restore are logical; the
//! pivot is geometric — two answers to the same question, "where are the
//! reader's eyes?".

use pdf_core::layout::{ViewMode, TOOLBAR_H};

use leptos::prelude::*;

use crate::components::primitives::hooks::dom::{h_page_list, page_list};
use crate::state::reader::{ReaderState, ZoomFocus};
use crate::viewer::engine::ViewerEngine;

/// Capture where the reader's eyes are, immediately before a transaction
/// opens. The main axis resolves around the viewport CENTRE (the point a
/// zoom should hold under the reader's eyes), expressed as a fraction
/// through `viewer.page`'s extent.
pub(crate) fn capture_focus(engine: &ViewerEngine, state: &ReaderState) -> ZoomFocus {
    let page = state.viewer.page.get_untracked().max(1);
    let mode = state.viewer.mode.get_untracked();
    match mode {
        ViewMode::ScrollVertical => ZoomFocus {
            page,
            main_fraction: fraction_through(
                &engine.vertical,
                (page - 1) as usize,
                state.document.num_pages.get_untracked() as usize,
            ),
            cross_fraction: 0.0,
        },
        ViewMode::ScrollHorizontal => ZoomFocus {
            page,
            main_fraction: fraction_through(
                &engine.horizontal,
                (page - 1) as usize,
                state.document.num_pages.get_untracked() as usize,
            ),
            cross_fraction: cross_axis_fraction(),
        },
        // Paginated modes have no strip scroll; the page IS the position and
        // the layouts remount on `page` directly.
        _ => ZoomFocus {
            page,
            main_fraction: 0.0,
            cross_fraction: 0.0,
        },
    }
}

/// The stage pivot for a transaction: the content point (main, cross) under
/// the viewport centre, read from the DOM scroller. The stage transform
/// pivots here, so the surface scales in place around the reader's eyes
/// while the scroll geometry itself stays frozen. Paginated modes return
/// `(0, 0)` — their stage centres on the viewport (`50% 50%`), which needs
/// no coordinates.
pub(crate) fn stage_origin(mode: ViewMode) -> (f64, f64) {
    match mode {
        ViewMode::ScrollVertical => {
            // The stage's top edge sits TOOLBAR_H below the scroller's
            // content origin (the strip scrolls under a fixed toolbar), so
            // the stage-local centre is the viewport centre minus that band.
            page_list()
                .map(|el| {
                    let vh = el.client_height() as f64;
                    (0.0, el.scroll_top() as f64 + vh * 0.5 - TOOLBAR_H)
                })
                .unwrap_or((0.0, 0.0))
        }
        ViewMode::ScrollHorizontal => h_page_list()
            .map(|el| {
                let cw = el.client_width() as f64;
                let ch = el.client_height() as f64;
                (
                    el.scroll_left() as f64 + cw * 0.5,
                    el.scroll_top() as f64 + ch * 0.5,
                )
            })
            .unwrap_or((0.0, 0.0)),
        _ => (0.0, 0.0),
    }
}

/// Fraction of the viewport centre's position through item `index`'s extent,
/// read entirely from the virtualizer's own coordinate space (offsets,
/// viewport extent, scroll offset all agree there). `0.0` when geometry is
/// empty or degenerate — the safe "top of the page" answer.
fn fraction_through(v: &virtual_list_leptos::Virtualizer, index: usize, count: usize) -> f64 {
    if count == 0 || index >= count {
        return 0.0;
    }
    let scroll = v.scroll_offset().get_untracked();
    let viewport = v.viewport().get_untracked().main.max(1.0);
    let centre = scroll + viewport * 0.5;
    fraction_at(v, index, count, centre)
}

/// `fraction` of the way through item `index`'s extent at the offset
/// `centre`, in the virtualizer's coordinate space.
fn fraction_at(v: &virtual_list_leptos::Virtualizer, index: usize, count: usize, centre: f64) -> f64 {
    let start = v.offset_of(index);
    let end = item_end(v, index, count);
    let extent = end - start;
    if extent <= 0.0 {
        return 0.0;
    }
    ((centre - start) / extent).clamp(0.0, 1.0)
}

/// End offset of item `index`: the next item's start, or the spacer total
/// for the last item (`offset_of` past the end answers the layout total, so
/// both paths agree in this coordinate space).
fn item_end(v: &virtual_list_leptos::Virtualizer, index: usize, count: usize) -> f64 {
    if index + 1 < count {
        v.offset_of(index + 1)
    } else {
        v.total_size().get_untracked()
    }
}

/// The scroll offset that puts `fraction` of the way through item `index`
/// at the viewport centre, against the geometry the virtualizer holds NOW
/// (the caller runs this immediately after the commit rescale, so "now" is
/// the new scale).
pub(crate) fn restore_offset(
    v: &virtual_list_leptos::Virtualizer,
    index: usize,
    count: usize,
    fraction: f64,
) -> f64 {
    if count == 0 || index >= count {
        return v.scroll_offset().get_untracked();
    }
    let viewport = v.viewport().get_untracked().main.max(1.0);
    let start = v.offset_of(index);
    let extent = (item_end(v, index, count) - start).max(0.0);
    start + fraction.clamp(0.0, 1.0) * extent - viewport * 0.5
}

/// The horizontal strip's cross-axis (vertical) position as a fraction of
/// the overflow, read from the DOM scroller. A fraction, never a pixel: the
/// old code's `scrollTop = 0` reset at the overflow boundary is exactly the
/// bug this replaces — zooming out now eases the position toward 0 as the
/// overflow shrinks, and zooming back in returns it.
fn cross_axis_fraction() -> f64 {
    let Some(el) = h_page_list() else {
        return 0.0;
    };
    let vh = el.client_height() as f64;
    let top = el.scroll_top() as f64;
    carry_fraction(top, (el.scroll_height() as f64 - vh).max(0.0))
}

/// Map an absolute cross-axis position at one overflow size to the
/// equivalent position at another. Pure — unit-tested against the
/// overflow-boundary regression.
pub(crate) fn carry_fraction(old_top: f64, old_max: f64) -> f64 {
    if old_max <= 0.0 || !old_max.is_finite() {
        return 0.0;
    }
    (old_top / old_max).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_deep_in_the_overflow_is_carried_proportionally() {
        // 37% of a 1000px overflow band.
        assert!((carry_fraction(370.0, 1000.0) - 0.37).abs() < 1e-12);
    }

    #[test]
    fn crossing_the_boundary_where_overflow_disappears_yields_zero_not_garbage() {
        // THE BUG THIS LOCKS OUT: the old code reset scrollTop to 0 the
        // moment there was no overflow left, throwing the position away and
        // making zoom-in unable to restore it. A fraction degrades to 0
        // continuously as the overflow shrinks.
        assert_eq!(carry_fraction(500.0, 0.0), 0.0);
        assert_eq!(carry_fraction(500.0, f64::NAN), 0.0);
        // And once there was no overflow to begin with, 0 is the honest answer.
        assert_eq!(carry_fraction(0.0, 800.0), 0.0);
    }

    #[test]
    fn zooming_out_and_back_in_returns_the_same_fraction() {
        // 30% into the vertical overflow of a horizontal strip.
        let frac = carry_fraction(300.0, 1000.0);
        assert!((frac - 0.3).abs() < 1e-12);
        // Zoom out until the band is half as tall, then back: the fraction,
        // and therefore the reader's place, survives the round trip.
        let band = |max: f64| frac * max;
        assert!((band(500.0) - 150.0).abs() < 1e-9);
        assert!((band(1000.0) - 300.0).abs() < 1e-9);
    }

    #[test]
    fn an_over_scrolled_position_clamps_into_the_band() {
        assert_eq!(carry_fraction(5000.0, 1000.0), 1.0);
    }
}
