//! The Animations tab: one switch per motion the reader models, so a reader who
//! wants the rail to slide but not the page to chase it can have exactly that.
//!
//! This tab is only OFFERED while the master switch (Layout → Animations) is
//! on, and the rows here are never the whole story: the master is ANDed into
//! every one of them by `Motion::from_prefs`, which is what the reader's own
//! pipeline reads. A detail switch therefore never lies about state — it is
//! simply unreachable while the master is off, and the tab that would show it
//! is gone.
//!
//! What each row turns off is the INTERPOLATION, never the change: the end
//! frame still arrives, in the frame it is asked for — or, for a follow that
//! was riding a burst of container sizes, once the burst goes quiet.

use leptos::prelude::*;

use crate::components::primitives::section_label::SectionLabel;
use crate::components::primitives::switch::Switch;
use crate::components::settings::common::Row;
use crate::state::AppState;

#[component]
pub(crate) fn AnimationsTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    view! {
        <SectionLabel text="Reader motion" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Sidebar Slide">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.sidebar_slide))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.sidebar_slide = v);
                    })
                    title="Tween the rail's width as it opens and closes. Off, it appears at its \
                           new width."
                        .to_string()
                />
            </Row>
            <Row label="Canvas Follows Sidebar">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.canvas_sidebar))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.canvas_sidebar = v);
                    })
                    title="Re-fit the page on every frame of the rail's slide. Off, it takes the \
                           new width in one step, when the rail stops."
                        .to_string()
                />
            </Row>
            <Row label="Canvas Follows Window">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.canvas_resize))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.canvas_resize = v);
                    })
                    title="Re-fit the page while the window is dragged. Off, it re-fits once, when \
                           the drag ends."
                        .to_string()
                />
            </Row>
            <Row label="Zoom In / Out">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.zoom))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.zoom = v);
                    })
                    title="Ease a zoom to its new scale. Off, every zoom lands on the first frame."
                        .to_string()
                />
            </Row>
            <Row label="Scroll To Page">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.scroll_jumps))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.scroll_jumps = v);
                    })
                    title="Glide the column to a page or a search hit. Off, it lands there."
                        .to_string()
                />
            </Row>
        </div>
        <p class="mt-2 text-xs text-muted">
            "Anything switched off still changes — it just changes in one frame. The master switch \
             is Animations, in the Layout tab, and it outranks everything here."
        </p>
    }
}
