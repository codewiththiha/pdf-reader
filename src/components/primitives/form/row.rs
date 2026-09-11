//! A labelled row: a name on the left, whatever answers it on the right.
//!
//! The smallest unit a settings panel or a sheet is made of, and the reason it is
//! a primitive rather than a `<div>` each caller spells: the gap, the padding and
//! the label's colour are one look, and a panel whose rows were four divs would be
//! four chances for one of them to drift a pixel. It lives here rather than in the
//! settings feature because the library's import sheet and removal receipt are
//! built out of the same rows — two features importing a component from a third
//! is a primitive with the wrong address.

use leptos::prelude::*;

/// One labelled row.
#[component]
pub fn Row(
    /// The name on the left. `&'static str` because every row's label is a
    /// sentence the code knows, not a value the reader typed.
    label: &'static str,
    /// The control, value or switch that answers it.
    children: Children,
) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 px-4 py-3.5">
            <span class="text-sm text-ink">{label}</span>
            {children()}
        </div>
    }
}
