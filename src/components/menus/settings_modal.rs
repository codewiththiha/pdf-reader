//! Centered reader settings modal: Layout / Theme tabs.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::appearance::BaseMode;
use pdf_core::settings::{FloatingLabelStyle, GlossColor, PageIndicatorStyle};

use crate::components::app_shell::toolbar_popover::MenuPopover;
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::menu_item::MenuItem;
use crate::components::primitives::section_label::SectionLabel;
use crate::components::primitives::separator::Separator;
use crate::components::primitives::switch::Switch;
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Layout,
    Theme,
}

#[component]
pub fn SettingsModal(state: AppState, open: RwSignal<bool>) -> impl IntoView {
    let tab = RwSignal::new(Tab::Layout);
    // Escape closes (outside-click is owned by the backdrop).
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
                    class="flex max-h-[85vh] w-[min(92vw,620px)] flex-col rounded-2xl border border-line bg-surface shadow-2xl"
                    on:click=move |ev| ev.stop_propagation()
                >
                    // ── Tab row: icon-only idle, icon+label when active ──
                    <div class="flex items-center gap-1 px-4 pb-2 pt-4">
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

#[component]
fn TabButton(tab: RwSignal<Tab>, t: Tab, icon: IconName, label: &'static str) -> impl IntoView {
    view! {
        <button
            type="button"
            on:click=move |_| tab.set(t)
            aria-pressed=move || (tab.get() == t).to_string()
            class="flex h-9 items-center justify-center rounded-lg transition-all \
focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            class=("gap-2 bg-accent-soft px-4 text-sm font-medium text-accent", move || tab.get() == t)
            class=("w-9 text-muted hover:text-ink", move || tab.get() != t)
        >
            <Icon name=icon size=17 />
            {move || (tab.get() == t).then(|| view! { <span>{label}</span> })}
        </button>
    }
}

#[component]
fn Row(label: &'static str, children: Children) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 px-4 py-3.5">
            <span class="text-sm text-ink">{label}</span>
            {children()}
        </div>
    }
}

/// Readest-style "Page Number ▾" value dropdown.
#[component]
fn StyleSelect<T>(
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
            <MenuPopover
                open=open
                anchor=root_ref
                width=190
                class="p-1".to_string()
            >
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

#[component]
fn LayoutTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    let indicator_off = Signal::derive(move || !s.with(|st| st.layout.page_indicator));
    let label_off = Signal::derive(move || !s.with(|st| st.layout.floating_label));
    view! {
        <SectionLabel text="Reader chrome" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Page Indicator">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.page_indicator))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.page_indicator = v);
                    })
                    title="Floating page indicator".to_string()
                />
            </Row>
            <Row label="Indicator Style">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.page_indicator_style))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.page_indicator_style = v);
                    })
                    options=vec![
                        (PageIndicatorStyle::PageNumber, "Page Number"),
                        (PageIndicatorStyle::Percentage, "Percentage"),
                    ]
                    label_of=|v: &PageIndicatorStyle| match v {
                        PageIndicatorStyle::PageNumber => "Page Number",
                        PageIndicatorStyle::Percentage => "Percentage",
                    }
                    disabled=indicator_off
                />
            </Row>
            <Row label="Floating Label">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.floating_label))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.floating_label = v);
                    })
                    title="Floating document label".to_string()
                />
            </Row>
            <Row label="Label Content">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.floating_label_style))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.floating_label_style = v);
                    })
                    options=vec![
                        (FloatingLabelStyle::FileName, "File Name"),
                        (FloatingLabelStyle::Title, "Document Title"),
                        (FloatingLabelStyle::Chapter, "Current Chapter"),
                    ]
                    label_of=|v: &FloatingLabelStyle| match v {
                        FloatingLabelStyle::FileName => "File Name",
                        FloatingLabelStyle::Title => "Document Title",
                        FloatingLabelStyle::Chapter => "Current Chapter",
                    }
                    disabled=label_off
                />
            </Row>
            <Row label="Progress Bar">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.progress_bar))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.progress_bar = v);
                    })
                    title="Reading progress bar".to_string()
                />
            </Row>
            <Row label="No Gap">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.no_gap))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.no_gap = v);
                    })
                    title="Remove the spacing between pages in scroll view".to_string()
                />
            </Row>
        </div>
    }
}

#[component]
fn ThemeTab(state: AppState) -> impl IntoView {
    let s = state.settings;
    view! {
        <SectionLabel text="Theme Mode" />
        <div class="flex items-center justify-end gap-2 rounded-xl border border-line px-4 py-3">
            {BaseMode::all()
                .into_iter()
                .map(|b| {
                    let icon = match b {
                        BaseMode::Light => IconName::Sun,
                        BaseMode::Dim => IconName::Dim,
                        BaseMode::Dark => IconName::Moon,
                    };
                    let active = Signal::derive(move || s.with(|st| st.appearance.base) == b);
                    view! {
                        <button
                            type="button"
                            title=b.label()
                            aria-pressed=move || active.get().to_string()
                            on:click=move |_| {
                                s.update(|st| {
                                    st.appearance.base = b;
                                    st.touch_appearance();
                                })
                            }
                            class="flex h-10 w-10 items-center justify-center rounded-full transition-colors \
focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            class=("bg-accent-soft text-accent", move || active.get())
                            class=("text-muted hover:text-ink", move || !active.get())
                        >
                            <Icon name=icon size=18 />
                        </button>
                    }
                })
                .collect_view()}
        </div>
        <div class="mt-5"><SectionLabel text="Highlight Colors" /></div>
        <div class="rounded-xl border border-line">
            <div class="grid grid-cols-6 gap-2 px-4 py-4">
                {GlossColor::all()
                    .into_iter()
                    .map(|c| {
                        let active = Signal::derive(move || s.with(|st| st.gloss_color) == c);
                        let bg = match c.hex() {
                            Some(h) => h.to_string(),
                            None => "var(--color-accent)".to_string(),
                        };
                        view! {
                            <button
                                type="button"
                                title=c.label()
                                aria-pressed=move || active.get().to_string()
                                on:click=move |_| s.update(|st| st.gloss_color = c)
                                class="flex flex-col items-center gap-1.5 rounded-lg py-1 \
focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            >
                                <span
                                    class="h-8 w-8 rounded-full border-2 border-line"
                                    style=format!("background-color:{bg}")
                                    class=(
                                        "ring-2 ring-accent ring-offset-2 ring-offset-surface",
                                        move || active.get(),
                                    )
                                ></span>
                                <span class="text-xs text-muted">{c.label()}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </div>
            <div class="border-t border-line">
                <Row label="Opacity">
                    <span class="flex items-center gap-3">
                        <span class="text-sm tabular-nums text-ink">
                            {move || format!("{:.1}", s.with(|st| st.gloss_opacity))}
                        </span>
                        <span class="flex gap-1.5">
                            <IconButton
                                icon=IconName::Minus
                                size=14
                                title="Less opaque"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || {
                                    s.update(|st| {
                                        st.gloss_opacity = (st.gloss_opacity - 0.1).clamp(0.1, 1.0);
                                    })
                                }
                            />
                            <IconButton
                                icon=IconName::Plus
                                size=14
                                title="More opaque"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || {
                                    s.update(|st| {
                                        st.gloss_opacity = (st.gloss_opacity + 0.1).clamp(0.1, 1.0);
                                    })
                                }
                            />
                        </span>
                    </span>
                </Row>
            </div>
        </div>
        <div class="mt-5"><Separator vertical=false /></div>
        <p class="mt-2 text-xs text-muted">
            "Colour, tint, textures and presets live in the palette menu on the title bar."
        </p>
    }
}
