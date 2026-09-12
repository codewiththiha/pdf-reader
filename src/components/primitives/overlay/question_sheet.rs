//! The question sheet: the one face every "the library has a question" sheet
//! wears — a heading with the queue's count on it, the question in one
//! sentence, the answers as a divided list of [`ChoiceRow`]s, the
//! apply-to-all switch when questions queue, and a Cancel that means the same
//! thing everywhere.
//!
//! Four sheets were drawing this from scratch — the name collision's, the
//! folder merge's, the covered file's and the folder's own — and the copies
//! had already drifted once (two of them grew the apply-to-all row, two never
//! needed it). The question's WORDS and its answers stay the sheet's: this
//! component is the skeleton they hang on, composed of the same
//! [`SheetHeader`], [`SheetBody`] and [`SheetFooter`] a custom-shaped sheet
//! (the import's, the removal receipt's) still composes itself.
//!
//! It renders the sheet's INSIDE, not its [`ModalShell`]: one host shell
//! (the conflict modal's) dispatches between three of these by the question's
//! kind, and a shell per question would be three lane registrations where
//! one was asked for.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;

use super::sheet::{SheetBody, SheetFooter, SheetHeader};

/// The "apply to all" row: one switch that gives every waiting question of
/// the same kind the answer being clicked. Rendered only when questions are
/// actually waiting behind this one — a switch offering to answer nothing is
/// a control that lies about its reach.
#[component]
pub fn ApplyToAll(
    /// How many MORE questions wait behind the one on screen.
    waiting: usize,
    /// The switch's own state, owned by the sheet whose answers read it.
    checked: RwSignal<bool>,
) -> impl IntoView {
    let label = format!("Apply to all {}", waiting + 1);
    view! {
        <div class="mt-3 flex items-center justify-between gap-3 rounded-xl border border-line px-3 py-2">
            <span class="text-xs text-muted">{label}</span>
            <Switch
                checked=Signal::derive(move || checked.get())
                on_change=Callback::new(move |on| checked.set(on))
                title="Give every waiting question this same answer"
                    .to_string()
            />
        </div>
    }
}

/// One question sheet's inside: header, question, answers, the queue's
/// switch, and the Cancel.
#[component]
pub fn QuestionSheet(
    /// The title — usually the arriving name.
    #[prop(into)]
    heading: String,
    /// The muted line under it: where the collision is, and how many more
    /// waits behind this one.
    #[prop(into)]
    subtitle: String,
    /// The question, in one sentence.
    #[prop(into)]
    question: String,
    /// What the ✕, the backdrop, the Escape key and the Cancel button all
    /// do: the sheet's one close, so all four end in one place.
    on_close: Callback<()>,
    /// What cancelling promises, as the button's tooltip — "leave the shelf
    /// as it is" for a placement, "import nothing" for a folder.
    #[prop(default = "Leave the shelf as it is".to_string())]
    cancel_title: String,
    /// The apply-to-all row's facts, for the sheets whose questions queue:
    /// how many wait, and the switch the answers read.
    #[prop(optional)]
    apply_all: Option<(usize, RwSignal<bool>)>,
    /// The answers: [`ChoiceRow`](crate::components::primitives::menu::choice_row::ChoiceRow)s,
    /// in the order the sheet means them to be read. Passed as the component's
    /// inner content, which is what the `children` name is for.
    children: Children,
) -> impl IntoView {
    view! {
        <>
            <SheetHeader heading=heading subtitle=subtitle on_close=on_close />
            <SheetBody>
                <p class="text-xs text-muted">{question}</p>
                <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                    {children()}
                </div>
                {apply_all
                    .filter(|(waiting, _)| *waiting > 0)
                    .map(|(waiting, checked)| {
                        view! { <ApplyToAll waiting checked /> }
                    })}
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| on_close.run(())
                    variant=ButtonVariant::Ghost
                    title=cancel_title
                >
                    <span>"Cancel"</span>
                </Button>
            </SheetFooter>
        </>
    }
}
