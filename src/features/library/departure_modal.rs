//! “Moving this shelf makes a copy.”
//!
//! The question a hand gets when it takes a read-at-place shelf off the seat
//! its folder's tree names for it — the shelf's own departure, and the one
//! move in the library that asks BEFORE it runs (`crate::services::library::arrange`).
//! A book's departure is the move itself and reports afterwards; a shelf's
//! departure copies every read-at-place book standing on the departing rungs
//! at once, and the cost of a whole tree is a sentence before the bytes
//! rather than a receipt after them.
//!
//! The sentence says what the copy is: the shelf lands where the drag meant,
//! under the counter name the row promises, the books it reads at their place
//! become the library's own copies — bytes into the store, and names,
//! highlights and places in them travelling with the rows — and everything
//! else inside rides along as it is. And it says what is NOT lost: the folder
//! on disk is untouched, its ledger remembers every book that left, so the
//! rescans stay silent and the next import of the folder re-mints the
//! original tree on the seats the disk names, brings the books back in their
//! old names and lights them up — on the nested rung, inside the nesting,
//! when a nested rung is what departed.
//!
//! Two answers, and a third when the drop landed inside the shelf's FAMILY:
//! pay the cost, put the shelf back where its folder names — no copies at
//! all — or leave it where the tree put it. A read-at-place shelf lives on
//! the seat its directory stands on, so a move inside the tree it belongs to
//! never has to cost a copy, and the sheet says which way home each mover
//! has: the displaced folder folds back into the family tree, and an
//! off-seat rung reseats on the seat the disk names.
//!
//! Cancel is the sheet's own close — the button, the backdrop, the Escape,
//! the lane — and every one of them means what the Cancel button says: the
//! departures do not land. The clean half of the gesture — the books and the
//! reader's own shelves that landed before the sheet rose — keeps its
//! landing, which is what Cancel has always meant on the collision sheet.

use leptos::prelude::*;

use library_core::text::plural;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::arrange::{ReturnPath, ShelfDepartureAsk};
use crate::services::library::{answer_departure_return, cancel_departure, confirm_departure};
use crate::state::AppState;

#[component]
pub(crate) fn ShelfDepartureModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_departure_open;

    // A close that came from the lane registry, the Escape key or the shell's
    // backdrop wrote only the boolean; the question goes with it, so a
    // gesture that was dismissed can never land behind the reader's back.
    Effect::new(move |_| {
        if !open.get() {
            state.library.shelf_departure.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="Moving this shelf makes a copy"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.shelf_departure.get()?;
                let info = Info::of(&ask);
                Some(view! { <DepartureSheet state=state info=info /> }.into_any())
            }}
        </ModalShell>
    }
}

/// Everything the sheet prints, read once per ask.
///
/// The folder sheet's rule: a `view!` body is a builder, not a place to
/// compute. The sentences are built here, per answer of the ask, and the body
/// only builds views out of them.
struct Info {
    heading: String,
    subtitle: String,
    /// One sentence per departing shelf: what it is called, which folder reads
    /// it, how many books the copy costs, and the name the copy takes at the
    /// level it lands on.
    lines: Vec<String>,
    /// The sentence every ask shares: what travels with the copies, what stays
    /// on disk, and how the original comes back.
    promise: String,
    confirm_label: &'static str,
    /// One sentence per mover that has a way home, and whether the sheet
    /// offers the answer at all: the third button rides a list with something
    /// in it.
    return_lines: Vec<String>,
    return_label: &'static str,
}

impl Info {
    fn of(ask: &ShelfDepartureAsk) -> Self {
        let one = ask.departing.len() == 1;
        let heading = match ask.departing.first() {
            Some(first) if one => first.name.clone(),
            Some(_) => plural(ask.departing.len(), "shelf", "shelves"),
            // The ask is never raised empty; an empty one still gets a heading
            // rather than a panic, because a sheet that could not render would
            // trap the reader under a backdrop.
            None => plural(0, "shelf", "shelves"),
        };
        let books: usize = ask.departing.iter().map(|each| each.books).sum();
        let subtitle = if books == 0 {
            "Read at place — moving makes it the library's own".to_string()
        } else {
            format!(
                "Read at place — moving copies {}",
                plural(books, "book", "books")
            )
        };
        let lines = ask
            .departing
            .iter()
            .map(|each| {
                let copy = format!("The copy will be named “{}”.", each.copy_name);
                if each.books == 0 {
                    format!(
                        "“{}” is read at its place inside “{}”. {copy}",
                        each.name, each.folder_name
                    )
                } else {
                    format!(
                        "“{}” is read at its place inside “{}”, with {} standing on \
                         the rungs that leave. {copy}",
                        each.name,
                        each.folder_name,
                        plural(each.books, "book", "books")
                    )
                }
            })
            .collect();
        let promise = if books == 0 {
            "Nothing inside is read at its place, so no bytes are copied — the shelf \
             itself becomes the library's own. The folder on disk is untouched: import \
             it again and its shelves come back on the seats the disk names, lit up."
                .to_string()
        } else {
            "The copied books become the library's own: their bytes move into the \
             library's store, and their names, highlights and places in them travel \
             with them. The folder on disk is untouched, and its ledger remembers \
             every book that left — the rescans stay silent, and the next import of \
             the folder brings its shelves back on the seats the disk names, with the \
             books in their old names, lit up where they return."
                .to_string()
        };
        let return_lines = ask
            .returns
            .iter()
            .map(|each| match &each.path {
                ReturnPath::Reclaim { family_name, .. } => format!(
                    "“{}” goes back inside “{}”, on the rung its directory names — \
                     no copies are made, and the folder keeps reading its files \
                     where they stand.",
                    each.name, family_name
                ),
                ReturnPath::Reseat { family_name, .. } => format!(
                    "“{}” goes back to the seat “{}” names for it — no copies \
                     are made, and the folder keeps reading its files where \
                     they stand.",
                    each.name, family_name
                ),
            })
            .collect::<Vec<_>>();
        Self {
            heading,
            subtitle,
            lines,
            promise,
            confirm_label: if one {
                "Move as a copy"
            } else {
                "Move as copies"
            },
            return_label: if ask.returns.len() == 1 {
                "Put it back in its place"
            } else {
                "Put them back in their places"
            },
            return_lines,
        }
    }
}

#[component]
fn DepartureSheet(state: AppState, info: Info) -> impl IntoView {
    let Info {
        heading,
        subtitle,
        lines,
        promise,
        confirm_label,
        return_lines,
        return_label,
    } = info;
    let has_return = !return_lines.is_empty();

    view! {
        <>
            <SheetHeader
                heading=heading
                subtitle=subtitle
                on_close=Callback::new(move |_| cancel_departure(state))
            />
            <SheetBody>
                {lines
                    .into_iter()
                    .map(|line| {
                        view! { <p class="text-xs text-muted">{line}</p> }
                    })
                    .collect::<Vec<_>>()}
                <p class="text-xs text-muted mt-3">{promise}</p>
                {has_return.then(move || {
                    view! {
                        <div class="mt-3 border-t border-line pt-3">
                            {return_lines
                                .into_iter()
                                .map(|line| {
                                    view! { <p class="text-xs text-muted">{line}</p> }
                                })
                                .collect::<Vec<_>>()}
                        </div>
                    }
                })}
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| cancel_departure(state)
                    variant=ButtonVariant::Toolbar
                    title="Leave the shelf where the folder's tree put it"
                >
                    <span>"Cancel"</span>
                </Button>
                {has_return.then(move || {
                    view! {
                        <Button
                            on_click=move |_| answer_departure_return(state)
                            variant=ButtonVariant::Toolbar
                            title="Return each shelf to the place its folder names; nothing is copied"
                        >
                            <span>{return_label}</span>
                        </Button>
                    }
                })}
                <Button
                    on_click=move |_| confirm_departure(state)
                    variant=ButtonVariant::Primary
                    title="Copy the shelf and its read-at-place books, and move the copies"
                >
                    <span>{confirm_label}</span>
                </Button>
            </SheetFooter>
        </>
    }
}
