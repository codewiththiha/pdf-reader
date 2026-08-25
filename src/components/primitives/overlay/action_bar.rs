//! Floating action bar: a fixed pill (bottom-right by default) for bulk
//! selection workflows — count readout, contextual actions, done/dismiss.
//! The gloss selection bar is its first consumer; annotation selection,
//! library item selection and future bulk ops can reuse it.

use leptos::prelude::*;

/// A floating selection action bar.
#[component]
pub fn ActionBar(
    /// Whether the bar is visible.
    visible: Signal<bool>,
    children: ChildrenFn,
    /// Extra classes on the pill (positioning, surface name…).
    #[prop(optional, into)]
    class: Option<String>,
    #[prop(default = "toolbar")]
    role: &'static str,
    #[prop(optional, into)]
    aria_label: Option<String>,
) -> impl IntoView {
    let base = format!(
        "fixed bottom-5 right-5 {} flex items-center gap-1 rounded-full border border-line \
         bg-surface py-1.5 pl-4 pr-1.5 shadow-[var(--gloss-shadow-menu)]",
        crate::components::primitives::floating::types::z::SELECTION_BAR
    );
    let pill_class = match class {
        Some(c) => format!("{base} {c}"),
        None => base,
    };
    // Static class + label, parked in signals so the Show closure (an `Fn`)
    // can read them without moving non-Copy Strings out of its environment.
    let class_sig = RwSignal::new(pill_class);
    let aria_label_sig = RwSignal::new(aria_label);

    view! {
        <Show when=move || visible.get()>
            <div
                class=move || class_sig.get()
                role=role
                aria-label=move || aria_label_sig.get()
            >
                {children()}
            </div>
        </Show>
    }
}
