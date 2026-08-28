//! Fit-mode effect: recomputes the render scale while FitMode::Width/Page is
//! active. The LAYOUT follows every `container_size` frame (a sidebar slide
//! must not let the stretched DOM and the virtualized heights diverge); the
//! crisp RENDER is debounced 180ms after the size settles, so a slide yields
//! exactly one re-render.
//!
//! Fit acts only while a fit mode is active. A manual zoom clears the fit mode
//! (see `ZoomController::zoom_to`), so `fit == None` is what lets `fit_effect`
//! stand down and stop fighting the gesture; there is no separate
//! "gesture owns the layout" flag anymore.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::math::{fit_scale, FitMode};

use crate::state::{ReaderState, SidebarMode};
use crate::viewer::engine::ViewerEngine;
use crate::viewer::zoom::commit_scale;

/// Must be called once from the app root (ReaderPage).
pub fn fit_effect(
    state: ReaderState,
    // Sidebar open/close re-runs fit so the page re-centers (app chrome
    // state passed in explicitly).
    sidebar: RwSignal<SidebarMode>,
    engine: ViewerEngine,
) {
    // Last page we computed a fit for. Doubles as the first-run marker: it
    // starts at 0 and is only ever written with a real (>= 1) page once the
    // document is open, so "no fit computed yet" and "the document is
    // opening" are the same state. A page-only change must NOT follow the
    // layout on the same frame (that would zoom on every row boundary while
    // the reader is scrolling); it waits for the existing debounce.
    let last_fit_page: StoredValue<u32> = StoredValue::new(0);

    Effect::new(move |_| {
        // No more degrading FitMode::Page to Width in spread modes: a spread
        // must fit as TWO pages (width) AND one page height, i.e. min().
        let fit = state.viewer.fit.get();
        let mode = state.viewer.mode.get();
        let (cw, ch) = state.viewer.container_size.get();
        let margin = state.viewer.page_margin.get();
        let page = state.viewer.page.get();
        let _animating = state.viewer.zoom_animating.get();
        // Reading the sidebar makes this effect re-run when it opens/closes,
        // because that changes the available container width.
        let _ = sidebar.get();

        // A manual zoom clears the fit mode (see `ZoomController::zoom_to`),
        // so `fit == None` is the concrete signal that the reader is zooming
        // by hand. A fit mode is a deliberate choice the reader can override
        // with a manual gesture; when there is none, this effect stands down
        // entirely rather than re-deriving a fit-width target that fights the
        // gesture as it animates (the old code imposed that ceiling and made
        // a manual zoom in a scrolling mode snap back toward fit).
        if fit == FitMode::None {
            return;
        }
        let Some(p1) = state.document.page1_size.get() else {
            return;
        };
        let (pw, ph) = state.document.metrics.intrinsic.with(|pages| {
            let i = page.saturating_sub(1) as usize;
            match pages.get(i) {
                Some(s) if s.width > 0.0 && s.height > 0.0 => (s.width, s.height),
                _ => (p1.width, p1.height),
            }
        });
        // Margins shrink the usable width.
        let horizontal = mode == ViewMode::ScrollHorizontal;
        let cw_eff = (cw - 2.0 * margin).max(1.0);
        // Horizontal joins the paginated modes here: the strip owns the full
        // window height and the auto-hiding title bar overlays it, exactly
        // like two-page mode. Reserving TOOLBAR_H for a bar that hides left a
        // permanent dead band above the pages.
        let ch_eff = if mode.is_paginated() || horizontal {
            ch.max(1.0)
        } else {
            (ch - TOOLBAR_H).max(1.0)
        };
        // Only Dual renders a true two-page SPREAD. Horizontal lays out
        // individual pages in a strip — each virtual item is one page — so
        // doubling the page width there halved the fit scale and "Fit
        // Width" zoomed out to half the size it should be.
        let spread = matches!(mode, ViewMode::Spread);
        let (pw_eff, ph_eff) = if spread { (pw * 2.0, ph) } else { (pw, ph) };
        let pad = if mode.is_paginated() || horizontal { 0.0 } else { TOOLBAR_H };

        let prev_page = last_fit_page.get_value();
        let first_run = prev_page == 0;
        let page_changed = prev_page != 0 && prev_page != page;
        last_fit_page.set_value(page);

        // A manual zoom (fit == None) is only re-checked when the *window* changes.
        // Page turns must never re-fit, or a zoomed reader gets snapped back to
        // fit-width on every arrow press (this ignored the Auto Scale = off rule).
        if fit == FitMode::None && page_changed {
            return;
        }

        // `fit == None` already returned above, so the target is always a
        // genuine fit here. `set_fit`/`zoom_to` (via the controller) keep the
        // `requested` value in sync so leaving a fit mode never resurrects a
        // stale gesture scale.
        let target = if horizontal {
            // The horizontal strip's only real constraint is viewport
            // HEIGHT: several pages are visible at once, so "fit width" has
            // no single-page meaning here. Width follows from the page's
            // aspect ratio, which is how fixed-layout horizontal readers
            // scale their spreads. The height is the FULL window, so pages
            // fill the band the title bar would otherwise steal.
            if ch_eff <= 1.0 {
                // Container not measured yet; fitting to it would slam the
                // page to the minimum scale.
                return;
            }
            let t = pdf_core::math::clamp_scale((ch_eff - pad).max(1.0) / ph_eff.max(1.0));
            state.viewer.zoom.requested.set(t);
            t
        } else {
            let t = fit_scale(
                fit,
                cw_eff,
                ch_eff,
                pw_eff,
                ph_eff,
                pad,
                state.viewer.zoom.level.get_untracked(),
            );
            state.viewer.zoom.requested.set(t);
            t
        };

        // --- the sidebar slide ------------------------------------------------
        // The `<aside>` animates its width over 300ms, so `container_size`
        // arrives as a burst of per-frame values. FREEZING the scale through
        // that burst (an earlier approach) keeps the page host wider than the
        // content box it now has to fit in — and because the host is a flex
        // child, the browser SQUISHES it: width shrinks, the inline height
        // doesn't, and a letter page goes from a 0.77 aspect to 0.61. The page
        // is visibly distorted, then snaps back at the end.
        //
        // Following the slide in `display_scale` ALONE is just as wrong: the
        // stretch effect re-sizes every mounted host to the new scale in the
        // same frame, but the virtualizer's heights — and therefore every
        // item top — would still be at the OLD scale, so pages gap and
        // overlap for the whole slide and only snap flush at the commit
        // render. The layout must move in LOCKSTEP with the stretched DOM:
        // same frame, same factor, through the same relayout a zoom gesture
        // uses.
        //
        // The rapid-fire rescales that implies are safe: `zoom_animating` is
        // held true for the whole slide (the cheap stretched raster shows,
        // renders are suspended, the DOM scroll echo is gated), and
        // `relayout_to` re-asserts its scroll write on the next animation
        // frame — once the spacer has actually been laid out at the new
        // height — so the stale-scrollHeight clamp cannot compound across
        // the burst.
        //
        // A manual gesture that just landed leaves `fit == None` (see the
        // early return above), so its scale is never reconciled against a fit
        // ceiling here — a manual zoom can grow past the fit and simply
        // overflow and scroll, as in every desktop reader.
        // Sidebar / window changes follow the layout on every frame so the
        // page does not squish. A PAGE change must not: scrolling through a
        // mixed-size book would zoom on every row boundary. Those wait for
        // the debounce below, which fires once the reader pauses.
        if !first_run && !page_changed {
            let cur = state.viewer.zoom.layout.get_untracked();
            if (target - cur).abs() >= 0.0005 {
                // Sidebar slide / window resize / refit: one dance. Display
                // leads (the stretch effect follows it), the layout moves in
                // the same frame, and the debounce below commits the crisp
                // render once the size settles.
                state.viewer.zoom_animating.set(true);
                engine.relayout_scale(&state, target / cur);
                state.viewer.zoom.layout.set(target);
            }
        }

        // Nothing to do? Then do NOTHING — do not arm the timer.
        //
        // This effect TRACKS `zoom_animating`, and the timer's quiet path below
        // writes it. Arming a timer when the layout is already settled
        // therefore fed the effect its own output: timer fires -> writes
        // `zoom_animating` -> effect re-runs -> arms another timer -> forever,
        // a self-sustaining 180ms loop that re-rendered the page endlessly.
        //
        // It showed up after zooming in past the fit in a NARROW window and
        // then widening it: the page ends up at a scale that already fits and
        // is already rendered, so every run took the quiet path. The page never
        // moved, which is why the width looked stable while the reader saw
        // constant flicker. A zoom click "fixed" it only because a gesture ends
        // in `commit_scale`, whose echo makes the next run return early and
        // breaks the cycle.
        let settled = (target - state.viewer.zoom.render.get_untracked()).abs() < 0.0005
            && (target - state.viewer.zoom.layout.get_untracked()).abs() < 0.0005;
        if settled && !state.viewer.zoom_animating.get_untracked() {
            return;
        }

        // Debounce: each `container_size` change re-runs this effect and clears
        // the previous timer, so the commit fires once the size has been stable
        // for ~180ms — one render per slide or per resize drag, at the end.
        let timer_engine = engine.clone();
        let handle = set_timeout_with_handle(
            move || {
                if first_run {
                    // Opening a document: no layout to animate from.
                    commit_scale(state, target);
                    return;
                }
                // A page-change refit skipped the per-frame relayout (so
                // scrolling a mixed-size book does not zoom on every row).
                // Do that relayout NOW, before the crisp render, or the
                // heights stay at the old scale and the scroll teleports.
                let cur = state.viewer.zoom.layout.get_untracked();
                if (target - cur).abs() >= 0.0005 {
                    timer_engine.relayout_scale(&state, target / cur);
                    state.viewer.zoom.layout.set(target);
                }
                let prev = state.viewer.zoom.render.get_untracked();
                if (target - prev).abs() >= 0.0005 {
                    commit_scale(state, target);
                } else if state.viewer.zoom_animating.get_untracked() {
                    // Already rendered at this scale (e.g. the sidebar returned
                    // to where it started): just release the gate.
                    //
                    // Guarded because a Leptos `set` notifies even when the
                    // value is unchanged, and this effect tracks this signal —
                    // an unconditional write here is a self-retrigger.
                    state.viewer.zoom_animating.set(false);
                }
            },
            Duration::from_millis(180),
        )
        .ok();
        on_cleanup(move || {
            if let Some(h) = handle {
                h.clear();
            }
        });
    });
}
