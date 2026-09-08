//! The no-books prompt.
//!
//! One glyph, one button and one optional line, and the button opens the same two
//! rows the shelf's add card does: an empty library has exactly two ways in, and
//! a third affordance here would be a third thing to keep in step with them. The
//! drop hint is only printed inside the desktop shell, because a browser build
//! has no filesystem to drop from.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::features::library::add_menu::AddMenu;
use crate::state::AppState;

#[component]
pub(crate) fn EmptyState(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let has_tauri = tauri_bridge::has_tauri();
    // Nothing to file onto: an empty library has no shelves yet, and "All" is not
    // one.
    let target: Signal<Option<String>> = Signal::derive(|| None);

    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <div node_ref=anchor class="relative flex max-w-md flex-col items-center gap-4 text-center">
                <Icon name=IconName::Library size=40 class="text-muted" />
                <Button
                    on_click=move |_| open.set(!open.get_untracked())
                    variant=ButtonVariant::Primary
                    active=Signal::derive(move || open.get())
                    title="Import books"
                >
                    <Icon name=IconName::Plus size=17 />
                    <span>"Import books"</span>
                </Button>
                {has_tauri.then(|| {
                    view! {
                        // The kinds come out of the format registry rather than
                        // out of a sentence someone has to remember to update.
                        <p class="text-xs text-muted">
                            {move || {
                                format!(
                                    "…or drop a {} file anywhere in the window",
                                    reader_core::format::kind_list()
                                )
                            }}
                        </p>
                    }
                })}
                <AddMenu state=state open=open anchor=anchor target=target />
            </div>
        </div>
    }
}
