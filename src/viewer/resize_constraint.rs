//! Constrain a manual zoom to the window width on resize.
//!
//! When the reader has zoomed by hand (`fit == None`), the old fit effect
//! still watched window/sidebar resizes: a smaller space scaled the page down
//! to fit the width, and a growing space brought it back but *never above the
//! zoom the reader chose*. That behaviour (a real, user-visible nicety) was
//! lost when the fit effect began standing down for `fit == None` to stop
//! fighting a manual gesture.
//!
//! This effect restores it, gated by the `constrain_zoom_to_window` setting,
//! and only when there is no fit mode and no animation in flight. It stays on
//! the reader's *requested* zoom as the ceiling and snaps the layout to
//! [`constrained_scale`] when the space is tighter than the page wants.

use leptos::prelude::*;

use pdf_core::layout::TOOLBAR_H;
use pdf_core::math::{constrained_scale, fit_scale, FitMode};

use crate::state::AppState;
use crate::viewer::engine::ViewerEngine;

/// Watch container resizes while the reader has manually zoomed. Runs once
/// from the reader shell, alongside `fit_effect`.
pub fn resize_constraint_effect(state: AppState, engine: ViewerEngine) {
    Effect::new(move |_| {
        // Only when the reader asked for it.
        if !state.settings.with(|s| s.layout.constrain_zoom_to_window) {
            return;
        }
        // Only when the reader zoomed by hand (no fit mode).
        let reader = state.reader;
        let fit = reader.viewer.fit.get();
        if fit != FitMode::None {
            return;
        }
        // Never fight an animation that is already moving the layout.
        if reader.viewer.zoom_animating.get() {
            return;
        }

        let mode = reader.viewer.mode.get();
        let (cw, ch) = reader.viewer.container_size.get();
        let margin = reader.viewer.page_margin.get();
        let page = reader.viewer.page.get();

        let Some(p1) = reader.document.page1_size.get() else {
            return;
        };
        let (pw, ph) = reader.document.metrics.intrinsic.with(|pages| {
            let i = page.saturating_sub(1) as usize;
            match pages.get(i) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });

        let horizontal = mode == pdf_core::layout::ViewMode::ScrollHorizontal;
        let cw_eff = (cw - 2.0 * margin).max(1.0);
        let ch_eff = if mode.is_paginated() || horizontal {
            ch.max(1.0)
        } else {
            (ch - TOOLBAR_H).max(1.0)
        };
        let spread = matches!(mode, pdf_core::layout::ViewMode::Spread);
        let (pw_eff, ph_eff) = if spread { (pw * 2.0, ph) } else { (pw, ph) };
        let pad = if mode.is_paginated() || horizontal { 0.0 } else { TOOLBAR_H };

        if cw_eff <= 1.0 {
            // Container not measured yet.
            return;
        }

        let fit_w = fit_scale(
            FitMode::Width,
            cw_eff,
            ch_eff,
            pw_eff,
            ph_eff,
            pad,
            reader.viewer.zoom.level.get_untracked(),
        );
        let desired = reader.viewer.zoom.requested.get();
        let target = constrained_scale(desired, fit_w);

        let cur = reader.viewer.zoom.layout.get_untracked();
        if (target - cur).abs() < 0.0005 {
            return;
        }

        // Snap the layout and the settled scale in lockstep, so the page rides
        // the same relayout a gesture would and never fights it.
        let factor = target / cur;
        engine.relayout_scale(&reader, factor);
        reader.viewer.zoom.layout.set(target);
        reader.viewer.zoom.level.set(target);
        reader.viewer.zoom.render.set(target);
    });
}
