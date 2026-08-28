//! Fit-mode effect: recomputes the render scale while FitMode::Width/Page is
//! active. The LAYOUT follows every `container_size` frame (a sidebar slide
//! must not let the stretched DOM and the virtualized heights diverge); the
//! crisp RENDER is debounced 180ms after the size settles, so a slide yields
//! exactly one re-render.
//!
//! The zoom machinery (gesture animation, anchoring, commit) lives in the
//! sibling `zoom` module; `fit_effect` hands scale changes to it via
//! `request_zoom`/`commit_scale` and reads `gesture_owns_layout` /
//! `take_commit_echo` to stay out of a gesture's way.

use std::time::Duration;

use leptos::prelude::*;

use pdf_core::layout::{TOOLBAR_H, ViewMode};
use pdf_core::math::{constrained_scale, fit_scale, FitMode};

use crate::state::{ReaderState, SidebarMode};
use crate::viewer::engine::ViewerEngine;

use super::zoom::{commit_scale, gesture_owns_layout, take_commit_echo};

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
        if gesture_owns_layout() {
            return;
        }
        let just_committed = take_commit_echo();
        let _sidebar_open = sidebar.get() != SidebarMode::None;
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
        } else if fit != FitMode::None {
            let t = fit_scale(
                fit,
                cw_eff,
                ch_eff,
                pw_eff,
                ph_eff,
                pad,
                state.viewer.zoom.level.get_untracked(),
            );
            // A fit mode IS a deliberate choice, so it owns the ceiling too.
            // Without this, leaving fit mode would resurrect a `desired_scale`
            // from some earlier gesture and the page would jump to it.
            state.viewer.zoom.requested.set(t);
            t
        } else if cw_eff > 1.0 {
            let fit_w = fit_scale(
                FitMode::Width,
                cw_eff,
                ch_eff,
                pw_eff,
                ph_eff,
                pad,
                state.viewer.zoom.level.get_untracked(),
            );
            let desired = state.viewer.zoom.requested.get_untracked();
            constrained_scale(desired, fit_w)
        } else {
            // Container not measured yet: a zero width would "fit" nothing and
            // slam the page to the minimum scale.
            return;
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
        if just_committed {
            // A gesture just landed: leave it exactly where the reader put it.
            //
            // The shrink-to-fit ceiling answers "the space got smaller", NOT
            // "the reader asked for more". Reconciling here applied the ceiling
            // to the gesture itself, so from a fit-width start every `+` was
            // computed, animated, and then immediately undone — the zoom
            // control looked broken because the page could never grow past the
            // window. Zooming in past the fit is deliberate and allowed; the
            // page simply overflows and scrolls, as in every desktop reader.
            //
            // The ceiling still applies on the next real container change,
            // which is the case it was written for.
            return;
        }
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
