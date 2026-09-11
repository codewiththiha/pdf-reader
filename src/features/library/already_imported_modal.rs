//! "That folder is already a shelf here."
//!
//! The answer a read-at-place import gets when the ground it picked is ground
//! the library already reads: the very folder of an in-place tree, or a
//! subfolder inside one (`crate::services::library::import::covered_shelf`).
//! A linked shelf IS the OS folder — one folder is one shelf, and there is no
//! second instance to make — so this is not a question with answers but a
//! sentence with a highlight: the modal says the folder is already in the
//! library, and closing it navigates to the shelf the reader meant and lights
//! it up, wherever in the tree it hangs.
//!
//! The note has a second sentence, and it is earned rather than gated: a
//! re-import of the tree's OWN root runs the reconciliation walk first — new
//! files join the tree, logged books come back — and only a walk that found
//! nothing new raises this modal, to say so and light the shelf.
//!
//! The highlight rides the CLOSE rather than the open, and every way out —
//! the button, the backdrop, Escape, the lane — ends on it: a light that
//! burns its seconds behind a modal nobody has dismissed is a light nobody
//! sees.
//!
//! A stored import never lands here. The library's own copies are the
//! library's to make another of, and that is a question — the folder sheet's.

use leptos::prelude::*;

use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::services::library::conflict;
use crate::services::library::reveal_shelf;
use crate::state::AppState;

#[component]
pub(crate) fn AlreadyImportedModal(state: AppState) -> impl IntoView {
    let open = state.library.already_imported_open;

    // Any close ends on the highlight: take the shelf the note named, light
    // it, and clear the note so the modal can never reopen onto a folder
    // somebody already acknowledged.
    Effect::new(move |_| {
        if open.get() {
            return;
        }
        let Some((shelf_id, _, _)) = state.library.already_imported.get_untracked() else {
            return;
        };
        state.library.already_imported.set(None);
        reveal_shelf(state, &shelf_id);
    });

    view! {
        <ModalShell
            open=open
            aria_label="This folder is already imported"
            width="min(92vw, 400px)"
        >
            {move || {
                let (_, name, nothing_new) = state.library.already_imported.get()?;
                let tooltip = name.clone();
                let sublabel = if nothing_new {
                    "Nothing new to import"
                } else {
                    "Already in the library"
                };
                // Two sentences, one shelf light. The gate's is for a pick
                // that never walked — a rung inside a tree the library reads
                // in place; the report's is for a re-import that DID walk and
                // found every book already standing.
                let sentence = if nothing_new {
                    "This folder is already in the library, and the library reads it where it \
                     stands. The import walked it again and found nothing new: every book it \
                     holds is on the shelf already, no removed or moved-away book came back, \
                     and nothing was copied, moved, or asked. Close this and the shelf lights \
                     up for you."
                } else {
                    "This folder is already imported — the library reads it where it stands, \
                     and a folder it reads in place cannot be imported twice. Nothing was \
                     copied, moved, or asked. Close this and the shelf it is on lights up for \
                     you; if the folder you picked is a subfolder of that shelf, the light \
                     is on the shelf inside the tree."
                };
                Some(view! {
                    <>
                        <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
                            <span class="min-w-0 flex-1">
                                <span class="block truncate text-sm font-semibold text-ink" title=tooltip>
                                    {name}
                                </span>
                                <span class="mt-0.5 block text-xs text-muted">
                                    {sublabel}
                                </span>
                            </span>
                            <IconButton
                                icon=IconName::Close
                                title="Close"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || conflict::close_already_imported(state)
                            />
                        </header>

                        <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
                            <p class="text-xs text-muted">{sentence}</p>
                        </div>

                        <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
                            <Button
                                on_click=move |_| conflict::close_already_imported(state)
                                variant=ButtonVariant::Primary
                                title="Close and light the shelf up"
                            >
                                <span>"Show the shelf"</span>
                            </Button>
                        </footer>
                    </>
                })
            }}
        </ModalShell>
    }
}
