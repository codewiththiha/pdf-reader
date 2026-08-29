//! Native traffic lights: on while the bar is pinned/hovered, or while the
//! rail is painted. A short hide grace absorbs the end-of-close hover-band
//! handoff: the rail releases chrome, then the expanded band can immediately
//! put the stationary pointer back over the bar.
//!
//! TWO HOSTS, ONE PAIR OF LIGHTS. The lights are native and pinned to the
//! window's top-left, so whichever surface owns that corner is the one they
//! sit on: the title bar's 88px gutter when the rail is down, the rail's
//! own header gutter when it is up — docked or floating, the header
//! reserves the same 88px either way. The shell controller's
//! `rail_present` therefore lights them on its own (for the whole paint —
//! open, or the close slide still running — and independent of the
//! pointer, because the rail's header is not hover-gated), while the bar's
//! hover only does so where the controller's `bar_gutter` says the bar
//! still has a gutter to offer.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::shell::controller::ShellController;
use crate::components::shell::titlebar::root::TitleBarCtx;

#[component]
pub fn TrafficLights() -> impl IntoView {
    let shell = use_context::<ShellController>()
        .expect("the page provides the shell controller");
    let ctx = use_context::<TitleBarCtx>();
    let last_sent = StoredValue::new_local(None::<bool>);
    let hide_grace = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = shell.rail_present().get() || (shell.bar_gutter().get() && ctx.visible.get());
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
