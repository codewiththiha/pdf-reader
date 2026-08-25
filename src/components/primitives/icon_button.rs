//! The standard square icon button (hover-only background). The titlebar
//! pin, the toolbar "…" overflow trigger and the More-menu trigger all used
//! identical raw markup; this owns it once. Disabled, tone and size variants
//! come from here so one-offs stop re-implementing them.

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

/// Semantic tone of an icon button.
///
/// `Accent`/`Danger` have no current user (the pin and toolbar triggers are
/// quiet); they are the contract for the destructive/active button sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum IconButtonTone {
    #[default]
    Default,
    Accent,
    Danger,
}

/// Size of the button box (and its touch target).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)] // `Sm` lands with the compact sidebar rows
pub enum IconButtonSize {
    /// 36px (h-9 w-9) — the toolbar standard.
    #[default]
    Md,
    /// 32px (h-8 w-8) — compact rows.
    Sm,
}

#[component]
pub fn IconButton(
    icon: IconName,
    #[prop(into, optional)] title: Option<String>,
    on_click: impl Fn() + 'static,
    /// Icon glyph size in px.
    #[prop(default = 18)] size: u16,
    /// Optional toggle state: renders `aria-pressed` plus the accent/ink
    /// colour swap (the titlebar pin).
    #[prop(optional)] pressed: Option<Signal<bool>>,
    #[prop(default = false)] disabled: bool,
    #[prop(default = IconButtonTone::Default)] tone: IconButtonTone,
    #[prop(default = IconButtonSize::Md)] size_variant: IconButtonSize,
) -> impl IntoView {
    let pressed_sig = pressed.unwrap_or_else(|| Signal::derive(|| false));
    let box_class = match size_variant {
        IconButtonSize::Md => "btn-icon inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg",
        IconButtonSize::Sm => "btn-icon inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
    };
    let tone_class = match tone {
        IconButtonTone::Default => "",
        IconButtonTone::Accent => "text-accent",
        IconButtonTone::Danger => "text-red-400",
    };

    view! {
        <button
            type="button"
            title=title
            aria-pressed=pressed.map(|_| move || pressed_sig.get().to_string())
            disabled=disabled
            on:click=move |_| on_click()
            class=format!(
                "{box_class} border border-transparent bg-transparent transition-colors hover:bg-line \
                 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
                 disabled:pointer-events-none disabled:opacity-50 {tone_class}"
            )
            class=("text-accent", move || pressed_sig.get() && tone == IconButtonTone::Default)
            class=("text-ink", move || !pressed_sig.get() && tone == IconButtonTone::Default)
        >
            <Icon name=icon size=size />
        </button>
    }
}
