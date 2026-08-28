//! Target resolution: turning a `ZoomCommand` plus the current context
//! into one concrete scale. This is the only place fit maths and the
//! shrink-to-fit ceiling live; manual steps, fit modes and window
//! constraints all resolve here so they cannot drift apart.
//!
//! Resolution also records the intent: a manual zoom writes `desired` and
//! clears the fit mode (a fit ceiling must never fight a gesture), a fit
//! refresh writes `desired` to the resolved fit, and a window constraint
//! deliberately leaves `desired` untouched — the reader's chosen zoom is
//! the ceiling it returns to when space comes back.

use leptos::prelude::*;

use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::math::{clamp_scale, constrained_scale, fit_scale, nearest_zoom, FitMode};

use crate::state::reader::{ZoomCommand, ReaderState};

use super::config::{profile_for, SETTLED_EPSILON};

/// Resolve a command to the scale it wants, or `None` when it must stand
/// down (no fit mode, an unmeasured container or an unmeasured document).
///
/// `in_flight` is the target of a transition already running; manual steps
/// chain from it so a fast `+ +` advances two presets rather than resolving
/// the same one twice. Every read here is untracked — the caller's effect
/// subscribes to the command signal and nothing else.
pub(crate) fn resolve(state: &ReaderState, cmd: ZoomCommand, in_flight: Option<f64>) -> Option<f64> {
    let zoom = state.viewer.zoom;
    let profile = profile_for(state.viewer.mode.get_untracked());
    match cmd {
        ZoomCommand::Set(scale) => {
            let target = profile.clamp(scale);
            zoom.desired.set(target);
            state.viewer.fit.set(FitMode::None);
            Some(target)
        }
        ZoomCommand::Step(dir) => {
            // Step from the in-flight target while a tween runs, else from
            // the settled scale. Mid-animation values are deliberately
            // avoided: nearest_zoom would usually round to the preset the
            // tween is already heading towards and swallow the press.
            let base = in_flight.unwrap_or_else(|| zoom.display.get_untracked());
            let target = profile.clamp(nearest_zoom(base, dir));
            // At the end of the ladder `nearest_zoom` answers with the same
            // preset it was given, so there is nowhere to go. Bail BEFORE
            // recording any intent: writing `desired` and clearing the fit
            // mode here would silently drop the reader out of Fit Width just
            // because they leaned on a zoom button that had nothing left to
            // do. The coordinator bails on an unchanged target too; this is
            // what keeps the state untouched.
            if (target - base).abs() < SETTLED_EPSILON {
                return None;
            }
            zoom.desired.set(target);
            state.viewer.fit.set(FitMode::None);
            Some(target)
        }
        ZoomCommand::Refit => {
            // A refit with no fit mode active would resolve to the current
            // scale AND clobber `desired` — the watcher never posts one, but
            // the resolver must be safe on its own terms.
            if state.viewer.fit.get_untracked() == FitMode::None {
                return None;
            }
            let dims = FitDims::of(state)?;
            let target = dims.fit(state.viewer.fit.get_untracked(), zoom.display.get_untracked());
            let target = profile.clamp(target);
            zoom.desired.set(target);
            Some(target)
        }
        ZoomCommand::Constrain => {
            if state.viewer.fit.get_untracked() != FitMode::None {
                return None; // a fit mode owns the scale while it is active
            }
            let dims = FitDims::of(state)?;
            let fit_w = dims.fit_width(zoom.display.get_untracked());
            let desired = zoom.desired.get_untracked();
            Some(profile.clamp(constrained_scale(desired, fit_w)))
        }
    }
}

/// The plain-geometry inputs of a fit computation, separated from the
/// reactive state so the arithmetic is unit-testable on the host.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FitDims {
    /// Usable container width (margins removed), `>= 1`.
    pub cw_eff: f64,
    /// Usable container height (`>= 1`; the toolbar band is reserved only
    /// for the vertical strip, which scrolls under a fixed bar).
    pub ch_eff: f64,
    /// Effective page width (doubled in spread mode).
    pub pw_eff: f64,
    /// Effective page height.
    pub ph_eff: f64,
    /// Vertical air a fit must leave in scrolling-vertical mode.
    pub pad: f64,
    /// Whether the strip runs horizontally (its only real constraint is the
    /// viewport height: several pages are visible at once, so "fit width"
    /// has no single-page meaning there).
    pub horizontal: bool,
}

impl FitDims {
    /// Collect the fit inputs from the reader state. `None` while the
    /// container or the document is still unmeasured — fitting to a
    /// placeholder would slam the page to the minimum scale.
    pub(crate) fn of(state: &ReaderState) -> Option<Self> {
        let mode = state.viewer.mode.get_untracked();
        let (cw, ch) = state.viewer.container_size.get_untracked();
        let margin = state.viewer.page_margin.get_untracked();
        let page = state.viewer.page.get_untracked().max(1);
        let p1 = state.document.page1_size.get_untracked()?;

        // The page under the reader's eyes, not page 1: a landscape plate in
        // an otherwise-portrait book must fit on its own terms.
        let (pw, ph) = state.document.metrics.intrinsic.with_untracked(|sizes| {
            match sizes.get((page - 1) as usize) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });

        let horizontal = mode == ViewMode::ScrollHorizontal;
        let cw_eff = (cw - 2.0 * margin).max(1.0);
        // The horizontal strip joins the paginated modes: it owns the full
        // window height and the auto-hiding title bar overlays it. Reserving
        // that band would leave a permanent dead strip above the pages.
        let ch_eff = if mode.is_paginated() || horizontal {
            ch.max(1.0)
        } else {
            (ch - TOOLBAR_H).max(1.0)
        };
        // Only the spread renders a true two-page spread; the horizontal
        // strip lays out one page per virtual item.
        let spread = matches!(mode, ViewMode::Spread);
        let (pw_eff, ph_eff) = if spread { (pw * 2.0, ph) } else { (pw, ph) };
        let pad = if mode.is_paginated() || horizontal {
            0.0
        } else {
            TOOLBAR_H
        };

        if cw_eff <= 1.0 || ch_eff <= 1.0 {
            return None;
        }

        Some(Self {
            cw_eff,
            ch_eff,
            pw_eff,
            ph_eff,
            pad,
            horizontal,
        })
    }

    /// The scale a fit mode wants. In the horizontal strip both fit modes
    /// resolve to the height fit: width follows from the page aspect, which
    /// is how fixed-layout horizontal readers scale their spreads.
    pub fn fit(&self, fit: FitMode, current: f64) -> f64 {
        if self.horizontal {
            return clamp_scale(self.ch_eff.max(1.0) / self.ph_eff.max(1.0));
        }
        fit_scale(
            fit,
            self.cw_eff,
            self.ch_eff,
            self.pw_eff,
            self.ph_eff,
            self.pad,
            current,
        )
    }

    /// The fit-width ceiling a manually zoomed page is constrained by.
    pub fn fit_width(&self, current: f64) -> f64 {
        self.fit(FitMode::Width, current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dims(horizontal: bool, spread: bool, cw: f64, ch: f64, pw: f64, ph: f64, pad: f64) -> FitDims {
        FitDims {
            cw_eff: cw,
            ch_eff: ch,
            pw_eff: if spread { pw * 2.0 } else { pw },
            ph_eff: ph,
            pad,
            horizontal,
        }
    }

    #[test]
    fn the_horizontal_strip_fits_the_viewport_height_for_both_fit_modes() {
        // 600px of height for a 792px-tall letter page: ~0.758.
        let d = dims(true, false, 1200.0, 600.0, 612.0, 792.0, 0.0);
        let by_width = d.fit(FitMode::Width, 1.0);
        let by_page = d.fit(FitMode::Page, 1.0);
        assert!((by_width - 600.0 / 792.0).abs() < 1e-9);
        assert_eq!(by_width, by_page);
    }

    #[test]
    fn a_spread_fits_two_pages_across() {
        let single = dims(false, false, 1024.0, 768.0, 612.0, 792.0, 0.0);
        let spread = dims(false, true, 1024.0, 768.0, 612.0, 792.0, 0.0);
        assert!((spread.fit(FitMode::Width, 1.0) - single.fit(FitMode::Width, 1.0) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_toolbar_band_is_reserved_only_where_the_strip_scrolls_under_it() {
        // Vertical: fit-page leaves the toolbar band as air on the height.
        let vertical = dims(false, false, 1000.0, 800.0, 500.0, 700.0, 56.0);
        assert!((vertical.fit(FitMode::Page, 1.0) - (800.0 - 56.0) / 700.0).abs() < 1e-9);
        // fit_scale subtracts the pad from the constrained axis in both
        // modes, so the width fit keeps it as side air too.
        assert!((vertical.fit(FitMode::Width, 1.0) - (1000.0 - 56.0) / 500.0).abs() < 1e-9);
    }

    #[test]
    fn fit_width_ceilings_a_manual_zoom_from_above() {
        let d = dims(false, false, 800.0, 600.0, 612.0, 792.0, 0.0);
        let fit_w = d.fit_width(2.0);
        // The ceiling is exactly the fit-width scale, and `constrained_scale`
        // (unit-tested in pdf-core) takes the min against it.
        assert!((fit_w - 800.0 / 612.0).abs() < 1e-9);
        assert!(constrained_scale(5.0, fit_w) <= fit_w + 1e-12);
    }
}
