//! The one full-window modal scrim: the dark field, the centring, the layer, the
//! click-outside-to-close and the Escape key.
//!
//! Every sheet in the app — the reader's settings, the import sheet, the remove
//! receipt and the collision question — opens on the SAME surface, and each used
//! to write its own `fixed inset-0 … bg-black/45` div plus its own
//! `use_modal_escape` call. Four copies is how a scrim ends up with four
//! different z-layers, four different click-outside rules and one sheet that
//! ignores Escape while its neighbours honour it.
//!
//! What stays with the caller is the PANEL — the width, the max-height, the
//! header, the scroll region — because those are the sheet's own design. The
//! scrim only positions and dismisses.
//!
//! Dismissal is a `Callback` rather than the open signal written directly, so a
//! sheet can close itself through the same door the backdrop uses; and Escape is
//! bound here for the same reason — a scrim that answers a click but ignores the
//! keyboard is half a dismissal rule.

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use app_chrome::floating::dismiss::use_modal_escape;

/// The shared scrim. `open` decides whether anything renders at all, and
/// `on_close` is what the backdrop and Escape call — defaulting to closing
/// `open`, which is why a sheet whose dismissal means more than flipping the
/// flag (the collision sheet's Cancel drops the queue behind the question)
/// passes its own.
///
/// Children are `leptos::children::ChildrenFn` rather than the `Children` a
/// component takes by default, and are called INSIDE the `when`: `Show`'s
/// children block must be an `Fn`, because it re-runs each time the sheet
/// opens, and only the `Rc`-backed children can be called from within one. It
/// also means a sheet's panel is built when it is shown rather than at the
/// scrim's mount — the collision sheet reads the ask it is about to print, and
/// a scrim that never opened must not have paid for the markup.
#[component]
pub fn ModalScrim(
    open: RwSignal<bool>,
    #[prop(optional)] on_close: Option<Callback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let close = on_close.unwrap_or_else(|| Callback::new(move |_| open.set(false)));
    use_modal_escape(open);

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| close.run(())
            >
                {children()}
            </div>
        </Show>
    }
}
