//! Book dragging: what a card carries, and what a drop target reads.
//!
//! The payload is a book ID and nothing else, because a drag between shelves is
//! a change to a list of ids — see `crate::services::library::arrange`, whose one
//! rule is that an in-app move never touches the filesystem. A drag that carried
//! a path would invite somebody to act on it.
//!
//! The window's file-drop machinery in `crate::effects::app::drag_drop` already
//! flips its `internal` flag on any `dragstart`, so a book being filed never
//! raises the "drop a document" overlay: the two kinds of drag are told apart by
//! the app, not by each target.

use leptos::prelude::*;

/// The drag type a book card writes and a shelf reads. Namespaced, because a
/// drop target also has to refuse the `text/plain` and `Files` drags that arrive
/// from outside the window.
pub const BOOK_MIME: &str = "application/x-pdfreader-book";

/// Start a drag of `book_id`.
pub fn begin(ev: &leptos::ev::DragEvent, book_id: &str) {
    let Some(transfer) = ev.data_transfer() else {
        return;
    };
    // `set_data` is fallible in this web-sys and a failure is not worth a
    // branch: a drag the browser will not carry is a drag that simply does not
    // drop anywhere.
    _ = transfer.set_data(BOOK_MIME, book_id);
    transfer.set_effect_allowed("move");
}

/// The book a drag is carrying, or `None` for any other kind of drag. An empty
/// answer is a refusal rather than an error: a file dragged in from the desktop
/// is the window's business, not a shelf's.
pub fn dragged(ev: &leptos::ev::DragEvent) -> Option<String> {
    let transfer = ev.data_transfer()?;
    let id = transfer.get_data(BOOK_MIME).ok()?;
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// Claim a dragover as ours, so the browser offers a drop. Without this the
/// cursor says "no" and `drop` never fires, which is why every target calls it
/// before deciding anything else.
pub fn accept(ev: &leptos::ev::DragEvent) -> bool {
    if dragged(ev).is_none() {
        return false;
    }
    ev.prevent_default();
    if let Some(transfer) = ev.data_transfer() {
        transfer.set_drop_effect("move");
    }
    true
}

/// The order the page is showing, so a drop on a card can name the index it
/// landed at. Provided by `crate::features::library::content` and read by the
/// cards and the tiles; a card cannot work the index out from its own DOM
/// without counting siblings, which is a second definition of the order.
#[derive(Clone, Copy)]
pub struct ShelfOrder(pub Signal<Vec<library_core::book::Book>>);

/// The card a book is being held over. One signal for the whole page, so exactly
/// one insertion line is drawn at a time and leaving a card clears the one
/// before it without either card knowing about the other.
#[derive(Clone, Copy)]
pub struct DropTarget(pub RwSignal<Option<String>>);
