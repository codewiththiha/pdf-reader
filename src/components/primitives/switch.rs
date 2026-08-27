//! Toggle switch control.

use leptos::prelude::*;

#[component]
pub fn Switch(
    checked: Signal<bool>,
    on_change: Callback<bool>,
    #[prop(into, optional)] title: Option<String>,
    #[prop(into, default = Signal::derive(|| false))] disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            role="switch"
            aria-checked=move || checked.get().to_string()
            title=title.clone()
            aria-label=title
            prop:disabled=move || disabled.get()
            on:click=move |_| on_change.run(!checked.get_untracked())
            class="relative inline-flex h-6 w-11 shrink-0 items-center rounded-full border \
transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
disabled:cursor-not-allowed disabled:opacity-45"
            class=("border-transparent bg-accent/80", move || checked.get())
            class=("border-line bg-line", move || !checked.get())
        >
            <span
                class="inline-block h-4 w-4 transform rounded-full bg-white/90 shadow \
transition-transform"
                class=("translate-x-6", move || checked.get())
                class=("translate-x-1", move || !checked.get())
            ></span>
        </button>
    }
}
