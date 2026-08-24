//! Card targeting: where the sprung box wants to be right now. The heavy
//! lifting — side choice, viewport clamping, shrink-to-fit — lives in
//! [`pdf_core::gloss::place_card`] so it is unit-testable on the host.

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, place_card};

use crate::components::ai::types::GlossPhase;

/// Preferred card width before viewport clamping.
pub const CARD_WIDTH: f64 = 360.0;
/// Card corner radius — the spring morphs the chip's pill into this.
pub const CARD_RADIUS: f64 = 18.0;
/// Gap between the highlighter stroke and the card's near edge.
pub const CARD_GAP: f64 = 16.0;
/// Viewport margin the expanded card must stay inside.
pub const CARD_MARGIN: f64 = 12.0;

/// Side-aware placement: the card goes on whichever side of the highlight has
/// more free space, never covering the stroke, vertically centered on the
/// mark and clamped into the viewport margin.
pub fn expanded_target(
    anchor: Signal<Option<GlossBox>>,
    content_height: RwSignal<f64>,
    viewport: RwSignal<(f64, f64)>,
) -> Memo<Option<GlossBox>> {
    Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        // Measured height is the full scroll column (header + separator +
        // body + paddings) — no chrome guess. See the measure twin.
        let h = content_height.get();
        Some(place_card(
            a,
            CARD_WIDTH,
            h,
            vw,
            vh,
            CARD_RADIUS,
            CARD_GAP,
            CARD_MARGIN,
        ))
    })
}

/// Expanded box is always f(live_anchor) + stored_offset, so a dragged card
/// still glides with the page on scroll. Compact/processing hug the mark.
pub fn spring_target(
    anchor: Signal<Option<GlossBox>>,
    gphase: RwSignal<GlossPhase>,
    drag_offset: RwSignal<Option<(f64, f64)>>,
    expanded: Memo<Option<GlossBox>>,
    viewport: RwSignal<(f64, f64)>,
) -> Memo<Option<GlossBox>> {
    Memo::new(move |_| {
        let a = anchor.get()?;
        match gphase.get() {
            GlossPhase::Expanded => {
                let mut e = expanded.get().unwrap_or(a);
                if let Some((dx, dy)) = drag_offset.get() {
                    let (vw, vh) = viewport.get();
                    e.x = (e.x + dx).clamp(CARD_MARGIN, (vw - e.w - CARD_MARGIN).max(CARD_MARGIN));
                    e.y = (e.y + dy).clamp(CARD_MARGIN, (vh - e.h - CARD_MARGIN).max(CARD_MARGIN));
                }
                Some(e)
            }
            _ => Some(a),
        }
    })
}
