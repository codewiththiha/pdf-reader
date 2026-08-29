//! The application's title bar: wires the generic [`TitleBar`] shell to the
//! app (pin persistence, the native traffic lights, the two left insets — the
//! band's and the row's light gutter — and the search hold).

use leptos::children::ViewFn;
use leptos::prelude::*;

use crate::components::app_shell::title_bar::TitleBar;
use crate::components::sidebar::shell::SidebarChromeCtx;
use crate::components::app_shell::traffic_lights::TrafficLights;
use crate::state::AppState;
use crate::state::SidebarMode;
use crate::storage::save_settings;

#[component]
pub fn AppTitleBar(
    state: AppState,
    #[prop(into)] left: ViewFn,
    /// Centered overlay passed through to the generic title bar. Defaults to empty.
    #[prop(into, default = ViewFn::from(|| ()))]
    center: ViewFn,
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
    // The sidebar owns the left inset while it is PAINTED: not merely while its
    // mode is open (the mode flips to `None` on the close click, but the aside
    // keeps sliding for 300ms — the reader page publishes that window as
    // `present`). Pages without a sidebar fall back to the raw mode.
    let chrome = use_context::<SidebarChromeCtx>();
    let rail_painted = Signal::derive(move || match chrome {
        Some(chrome) => chrome.present.get(),
        None => state.ui.sidebar.get() != SidebarMode::None,
    });
    // Overlay mode floats the rail OVER the page instead of docking it. It is a
    // reader-page setting: the library has no rail, so its bar keeps the gutter
    // and its lights.
    let overlay = Signal::derive(move || match chrome {
        Some(_) => state.settings.with(|st| st.layout.sidebar_overlay),
        None => false,
    });
    // Only a DOCKED rail takes the band's left edge. An overlay rail mounts
    // above the bar (see `features/reader/rail.rs`) and covers its corner, so
    // the band keeps the full window width and reads as one bar either way.
    let band_inset = Signal::derive(move || rail_painted.get() && !overlay.get());
    // The 88px gutter is the native lights' home. A docked rail takes that
    // corner over, and overlay mode has no lights at all, so in both cases the
    // leading control moves left into the space they would have occupied.
    let lights_gutter = Signal::derive(move || !overlay.get() && !rail_painted.get());
    // Not the same question as `lights_gutter`, and the difference matters:
    // this one asks whether the BAR has a gutter to offer at all in this layout
    // mode, regardless of what is covering it. Overlay mode answers no — the
    // bar keeps its full width and its leading control sits in the space the
    // lights would have taken, so a hover must not put them back on top of it.
    // The floating rail still hosts them from its own header while it is up,
    // which is `present`'s job and not this signal's.
    let bar_gutter = Signal::derive(move || !overlay.get());

    view! {
        <TitleBar
            pinned=pinned
            on_pin_change=on_pin_change
            extra_hold=extra_hold
            band_inset=band_inset
            lights_gutter=lights_gutter
            left=left
            center=center
            right=right
        >
            {show_traffic_lights
                .then(|| view! { <TrafficLights present=rail_painted bar_gutter=bar_gutter /> })}
            {children()}
        </TitleBar>
    }
}
