//! A choice row: a name, and the one line under it that says what choosing it
//! does.
//!
//! Not [`MenuItem`](super::menu_item::MenuItem), which is a command — an icon, a
//! label, an optional trailing slot, one line tall. This is an ANSWER to a
//! question a sheet is asking, and the difference is the note: a merge, a replace
//! and an "as new" are three consequences the reader has to be able to read
//! before the click, so the note wraps to as many lines as it needs and there is
//! no icon column competing with it. A glyph beside a sentence is decoration the
//! reader has to look past.
//!
//! It lives here rather than in the collision sheet that first needed it because
//! the folder sheet reached sideways into a sibling feature file to borrow it:
//! two sheets in one feature importing a row shape from a third file in the same
//! feature is a primitive with the wrong address.

use leptos::prelude::*;

/// One answer on a sheet: its name, and the line that promises what it does.
#[component]
pub fn ChoiceRow(
    /// The answer's own name — "Merge", "Add as new", "Replace".
    label: &'static str,
    /// What choosing it does, in the sheet's own words. Wraps; a promise the
    /// reader has to take on faith is not a promise.
    #[prop(into)]
    note: String,
    on_click: Callback<()>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="flex w-full flex-col gap-0.5 px-3.5 py-2.5 text-left transition-colors \
                   hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            on:click=move |_| on_click.run(())
        >
            <span class="text-sm text-ink">{label}</span>
            <span class="text-xs text-muted">{note}</span>
        </button>
    }
}
