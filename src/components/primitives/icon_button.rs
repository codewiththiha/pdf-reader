//! Icon-only button: the toolbar's square ghost button. `pressed` renders
//! the accent/ink toggle (titlebar pin); `disabled` is the plain
//! disabled:opacity-50 the other controls use.
//!
//! The borderless look is deliberate and shared: toolbar icon buttons sit
//! on glass/paper and only lift on hover, while the bordered
//! [`Button`](super::button::Button) variant is for labeled actions.

use leptos::prelude::*;

use super::icon::{Icon, IconName};

#[component]
pub fn IconButton(
    icon: IconName,
    on_click: impl Fn() + 'static,
    #[prop(into, optional)]
    title: Option<String>,
    #[prop(default = 18)]
    size: u16,
    /// Optional toggle state: renders `aria-pressed` plus the accent/ink
    /// colour swap (the titlebar pin).
    #[prop(optional)]
    pressed: Option<Signal<bool>>,
    /// Reactive so callers can wire `Signal::derive(...)` from their state;
    /// defaults to always-enabled.
    #[prop(into, default = Signal::derive(|| false))]
    disabled: Signal<bool>,
    /// Marks the button as search chrome for the floating search's
    /// outside-dismiss exclusion (pointerdown on this button must not close
    /// the bar the click is about to toggle). Rendered as
    /// `data-search-chrome="true"`, matching the exclusion selector.
    #[prop(default = false)]
    data_search_chrome: bool,
) -> impl IntoView {
    let pressed_sig = pressed.unwrap_or_else(|| Signal::derive(|| false));
    let box_class = "btn-icon inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg";

    view! {
        <button
            type="button"
            title=title
            data-search-chrome=move || if data_search_chrome { "true" } else { "" }
            aria-pressed=pressed.map(|_| move || pressed_sig.get().to_string())
            prop:disabled=move || disabled.get()
            on:click=move |_| on_click()
            class=format!(
                "{box_class} border border-transparent bg-transparent transition-colors hover:bg-line \
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
                 disabled:pointer-events-none disabled:opacity-50"
            )
            class=("text-accent", move || pressed_sig.get())
            class=("text-ink", move || !pressed_sig.get())
        >
            <Icon name=icon size=size />
        </button>
    }
}
