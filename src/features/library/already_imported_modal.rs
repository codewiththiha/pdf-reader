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
//! And a third, which is the FOLD's report: an import that found a folder
//! standing outside the family its directory names — a rung removed and
//! imported on its own, a departure's original asked back — put it back on
//! the rung the disk names instead of asking about it, folded the folder
//! that was reading it into the tree, and names here the shelf that went
//! home. An import is an ask, and a member outside its family is an ask
//! answered.
//!
//! The highlight rides the CLOSE rather than the open, and every way out —
//! the button, the backdrop, Escape, the lane — ends on it: a light that
//! burns its seconds behind a modal nobody has dismissed is a light nobody
//! sees.
//!
//! A stored import never lands here. The library's own copies are the
//! library's to make another of, and that is a question — the folder sheet's.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict;
use crate::services::library::reveal_shelf;
use crate::state::library::{AlreadyNote, NoteKind};
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
        let Some(note) = state.library.already_imported.get_untracked() else {
            return;
        };
        state.library.already_imported.set(None);
        reveal_shelf(state, &note.shelf_id);
    });

    view! {
        <ModalShell
            open=open
            aria_label="This folder is already imported"
            width="min(92vw, 400px)"
        >
            {move || {
                let AlreadyNote { name, kind, .. } = state.library.already_imported.get()?;
                let sublabel = kind.sublabel().to_string();
                // Three sentences, one shelf light. The gate's is for a pick
                // that never walked — a rung inside a tree the library reads
                // in place; the report's is for a re-import that DID walk and
                // found every book already standing; the fold's is for an
                // import that put a shelf back inside the family its
                // directory names, and the light lands where it stands now.
                let sentence = match &kind {
                    NoteKind::NothingNew => {
                        "The import walked the folder again and found nothing new — every \
                         book is already on the shelf. The shelf lights up when you close \
                         this."
                            .to_string()
                    }
                    NoteKind::Gated => {
                        "The library already reads this folder in place — one folder, one \
                         shelf, so nothing was imported. If you picked a subfolder, the \
                         light is on its shelf inside the tree. It lights up when you close \
                         this."
                            .to_string()
                    }
                    NoteKind::Returned => {
                        format!(
                            "“{name}” went back inside the folder it belongs to, onto the \
                             shelf its directory names. Nothing was copied, and nothing on \
                             disk moved. It lights up where it stands now when you close \
                             this."
                        )
                    }
                };
                Some(view! {
                    <>
                        <SheetHeader
                            heading=name.clone()
                            subtitle=sublabel
                            on_close=Callback::new(move |_| conflict::close_already_imported(state))
                        />
                        <SheetBody>
                            <p class="text-xs text-muted">{sentence}</p>
                        </SheetBody>
                        <SheetFooter>
                            <Button
                                on_click=move |_| conflict::close_already_imported(state)
                                variant=ButtonVariant::Primary
                                title="Close and light the shelf up"
                            >
                                <span>"Show the shelf"</span>
                            </Button>
                        </SheetFooter>
                    </>
                })
            }}
        </ModalShell>
    }
}
