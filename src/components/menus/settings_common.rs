//! Shared pieces of the reader settings modal: the tab switcher, the labelled
//! `Row` wrapper, and the generic `StyleSelect` dropdown. `LayoutTab` and
//! `ThemeTab` (in their own files) build on these; `SettingsModal` is the
//! shell that hosts them.

use leptos::html;
use leptos::prelude::*;

use crate::components::app_shell::toolbar_popover::MenuPopover;
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::menu_item::MenuItem;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Layout,
    Theme,
}

#[component]
pub(crate) fn TabButton(
    tab: RwSignal<Tab>,
    t: Tab,
    icon: IconName,
    label: &'static str,
) -> impl IntoView {
    let class = move || {
        let base = "flex h-9 items-center justify-center rounded-lg transition-all \
focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
        if tab.get() == t {
            format!("{base} gap-2 bg-accent-soft px-4 text-sm font-medium text-accent")
        } else {
            format!("{base} w-9 text-muted hover:text-ink")
        }
    };
    view! {
        <button
            type="button"
            on:click=move |_| tab.set(t)
            aria-pressed=move || (tab.get() == t).to_string()
            class=class
        >
            <Icon name=icon size=17 />
            {move || (tab.get() == t).then(|| view! { <span>{label}</span> })}
        </button>
    }
}

#[component]
pub(crate) fn Row(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 px-4 py-3.5">
            <span class="text-sm text-ink">{label}</span>
            {children()}
        </div>
    }
}

#[component]
pub(crate) fn StyleSelect<T>(
    value: Signal<T>,
    on_change: Callback<T>,
    options: Vec<(T, &'static str)>,
    label_of: fn(&T) -> &'static str,
    disabled: Signal<bool>,
) -> impl IntoView
where
    T: Clone + Copy + PartialEq + Send + Sync + 'static,
{
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();
    let opts = StoredValue::new(options);
    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                prop:disabled=move || disabled.get()
                on:click=move |_| open.set(!open.get())
                class="flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-ink \
hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
disabled:cursor-not-allowed disabled:opacity-45"
            >
                <span>{move || label_of(&value.get())}</span>
                <Icon name=IconName::ChevronDown size=12 class="text-muted" />
            </button>
            <MenuPopover open=open anchor=root_ref width=190 class="p-1".to_string()>
                {opts.with_value(|opts| {
                    opts.iter()
                        .map(|(v, l)| {
                            let v = *v;
                            let label = (*l).to_string();
                            view! {
                                <MenuItem
                                    label=label
                                    selected=Signal::derive(move || value.get() == v)
                                    on_click=move || {
                                        on_change.run(v);
                                        open.set(false);
                                    }
                                >
                                    <span class="ml-auto inline-flex w-4 shrink-0 justify-center text-accent">
                                        {move || {
                                            (value.get() == v).then(|| {
                                                view! { <Icon name=IconName::Check size=14 /> }
                                            })
                                        }}
                                    </span>
                                </MenuItem>
                            }
                        })
                        .collect_view()
                })}
            </MenuPopover>
        </div>
    }
}
