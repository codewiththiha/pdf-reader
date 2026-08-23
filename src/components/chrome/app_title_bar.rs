//! The application's title bar: wires the generic [`TitleBar`] shell to the
//! app (pin persistence, native traffic lights, sidebar inset, search hold).

use leptos::children::ViewFn;
use leptos::prelude::*;

use crate::components::chrome::title_bar::{TitleBar, TitleBarCtx};
use crate::state::AppState;
use crate::state::SidebarMode;
use crate::storage::save_settings;

#[component]
pub fn AppTitleBar(
    state: AppState,
    #[prop(into)] left: ViewFn,
    #[prop(into)] right: ViewFn,
    /// Native traffic lights. The generic TitleBar does not know about them;
    /// this wrapper decides whether to mount the effect.
    #[prop(default = true)]
    show_traffic_lights: bool,
    children: Children,
) -> impl IntoView {
    let pinned = RwSignal::new(state.settings.with(|s| s.titlebar_pinned));
    let on_pin_change = Callback::new(move |p: bool| {
        state.settings.update(|s| s.titlebar_pinned = p);
        if let Err(e) = save_settings(&state.settings.with(|s| s.clone())) {
            e.report();
        }
    });
    // The open floating search holds the bar (like an open popover).
    let extra_hold = Signal::derive(move || state.reader.search.visible.get());
    // The sidebar owns the left inset while open.
    let band_inset = Signal::derive(move || state.ui.sidebar.get() != SidebarMode::None);

    view! {
        <TitleBar
            pinned=pinned
            on_pin_change=on_pin_change
            extra_hold=extra_hold
            band_inset=band_inset
            left=left
            right=right
        >
            {show_traffic_lights.then(|| view! { <TrafficLights state=state /> })}
            {children()}
        </TitleBar>
    }
}

/// Native traffic lights: on while pinned/hovered, or while an open sidebar
/// owns them (its chrome row is always visible). Mounted inside the
/// [`TitleBar`] children so it can read the shared [`TitleBarCtx`].
#[component]
fn TrafficLights(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = ctx.visible.get() || state.ui.sidebar.get() != SidebarMode::None;
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::set_traffic_lights(on).await;
        });
    });
}
