//! Centered reader settings modal shell: the Layout / Theme tab switcher.
//! The tab bodies live in `settings_layout_tab` and `settings_theme_tab`.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::menus::settings_common::{Tab, TabButton};
use crate::components::menus::settings_layout_tab::LayoutTab;
use crate::components::menus::settings_theme_tab::ThemeTab;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::state::AppState;

#[component]
pub fn SettingsModal(
    state: AppState,
    open: RwSignal<bool>,
    #[prop(default = "min(92vw, 620px)")] width: &'static str,
    #[prop(default = "min(76vh, 640px)")] height: &'static str,
) -> impl IntoView {
    let tab = RwSignal::new(Tab::Layout);
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let h = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            if let Ok(kev) = ev.dyn_into::<web_sys::KeyboardEvent>() {
                if kev.key() == "Escape" {
                    open.set(false);
                }
            }
        });
        on_cleanup(move || h.remove());
    });
    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| open.set(false)
            >
                <div
                    class="flex flex-col rounded-2xl border border-line bg-surface shadow-2xl"
                    style=format!("width:{width};height:{height}")
                    on:click=move |ev| ev.stop_propagation()
                >
                    <div class="flex shrink-0 items-center gap-1 px-4 pb-2 pt-4">
                        <TabButton tab=tab t=Tab::Layout icon=IconName::Layout label="Layout" />
                        <TabButton tab=tab t=Tab::Theme icon=IconName::Palette label="Theme" />
                        <div class="ml-auto">
                            <IconButton
                                icon=IconName::Close
                                title="Close"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || open.set(false)
                            />
                        </div>
                    </div>
                    <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
                        {move || match tab.get() {
                            Tab::Layout => view! { <LayoutTab state=state /> }.into_any(),
                            Tab::Theme => view! { <ThemeTab state=state /> }.into_any(),
                        }}
                    </div>
                </div>
            </div>
        </Show>
    }
}
