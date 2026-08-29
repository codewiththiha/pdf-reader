//! Native traffic lights: on while the bar is pinned/hovered, or while the
//! sidebar is painted. A short hide grace absorbs the end-of-close hover-band
//! handoff: the rail releases chrome, then the expanded band can immediately
//! put the stationary pointer back over the bar.
//!
//! TWO HOSTS, ONE PAIR OF LIGHTS. The lights are native and pinned to the
//! window's top-left, so whichever surface owns that corner is the one they sit
//! on: the title bar's 88px gutter when the rail is down, the rail's own header
//! gutter when it is up — docked or floating, the header reserves the same 88px
//! either way. `present` therefore lights them on its own, while the bar's
//! hover only does so where the bar still has a gutter to offer.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::app_shell::title_bar::TitleBarCtx;

#[component]
pub fn TrafficLights(
    /// The rail is painted, so its header is hosting the lights. Lit for the
    /// whole paint — open, or the close slide still running — and independent
    /// of the pointer, because the rail's header is not hover-gated.
    #[prop(into)] present: Signal<bool>,
    /// The title bar itself reserves the 88px gutter. Off in overlay mode: the
    /// bar keeps its full width there and its leading control moves into the
    /// space the lights would have taken, so a hover must not bring them back
    /// on top of it. The rail still hosts them when it is up.
    #[prop(into)] bar_gutter: Signal<bool>,
) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let last_sent = StoredValue::new_local(None::<bool>);
    let hide_grace = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = present.get() || (bar_gutter.get() && ctx.visible.get());
        if on {
            // Never send the end-of-slide false if hover re-enters on the
            // following frame.
            if let Some(h) = hide_grace.get_value() {
                h.clear();
                hide_grace.set_value(None);
            }
            if last_sent.get_value() == Some(true) {
                return;
            }
            last_sent.set_value(Some(true));
            wasm_bindgen_futures::spawn_local(async move {
                pdf_engine::api::set_traffic_lights(true).await;
            });
        } else {
            // Initial false also receives the grace so native defaults are
            // eventually hidden; an already-hidden or pending hide does not
            // schedule another command.
            if last_sent.get_value() == Some(false) || hide_grace.get_value().is_some() {
                return;
            }
            let handle = set_timeout_with_handle(
                move || {
                    hide_grace.set_value(None);
                    last_sent.set_value(Some(false));
                    wasm_bindgen_futures::spawn_local(async move {
                        pdf_engine::api::set_traffic_lights(false).await;
                    });
                },
                Duration::from_millis(120),
            )
            .ok();
            hide_grace.set_value(handle);
        }
    });

    on_cleanup(move || {
        if let Some(h) = hide_grace.get_value() {
            h.clear();
        }
    });
}
