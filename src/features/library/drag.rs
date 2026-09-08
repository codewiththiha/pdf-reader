//! What a card carries when it is dragged, and what a drop target reads.
//!
//! Two payloads and nothing else: a book id, and a shelf id. A drag between
//! shelves is a change to a list of ids and a drag of one shelf into another is
//! a change to one `parent` — see `crate::services::library::arrange`, whose one
//! rule is that an in-app move never touches the filesystem. A drag that carried
//! a path would invite somebody to act on it.
//!
//! The two are namespaced MIME types rather than one payload with a kind on it,
//! because a target has to be able to refuse one and accept the other: a book
//! card offers an insertion index to a book and nothing at all to a shelf, while
//! a folder card offers membership to a book and nesting to a shelf.
//!
//! The window's file-drop machinery in `crate::effects::app::drag_drop` already
//! flips its `internal` flag on any `dragstart`, so a book being filed — or a
//! shelf being nested — never raises the "drop a document" overlay: the two
//! kinds of drag are told apart by the app, not by each target.

use leptos::prelude::*;

use library_core::shelf::Shelf;

/// The drag type a book card writes and a shelf reads. Namespaced, because a
/// drop target also has to refuse the `text/plain` and `Files` drags that arrive
/// from outside the window.
pub const BOOK_MIME: &str = "application/x-pdfreader-book";

/// The drag type a folder card writes and another folder card reads. Separate
/// from [`BOOK_MIME`] so a target can tell "file this book here" from "nest this
/// shelf inside that one" without parsing a payload.
pub const FOLDER_MIME: &str = "application/x-pdfreader-folder";

/// Start a drag of `book_id`.
pub fn begin(ev: &leptos::ev::DragEvent, book_id: &str) {
    write(ev, BOOK_MIME, book_id);
}

/// Start a drag of the shelf `shelf_id`.
pub fn begin_folder(ev: &leptos::ev::DragEvent, shelf_id: &str) {
    write(ev, FOLDER_MIME, shelf_id);
}

fn write(ev: &leptos::ev::DragEvent, mime: &str, payload: &str) {
    let Some(transfer) = ev.data_transfer() else {
        return;
    };
    // `set_data` is fallible in this web-sys and a failure is not worth a
    // branch: a drag the browser will not carry is a drag that simply does not
    // drop anywhere.
    _ = transfer.set_data(mime, payload);
    transfer.set_effect_allowed("move");
}

/// Read one namespaced payload. An empty answer is a refusal rather than an
/// error: a file dragged in from the desktop is the window's business, not a
/// shelf's.
fn read(ev: &leptos::ev::DragEvent, mime: &str) -> Option<String> {
    let transfer = ev.data_transfer()?;
    let payload = transfer.get_data(mime).ok()?;
    if payload.is_empty() {
        None
    } else {
        Some(payload)
    }
}

/// The book a drag is carrying, or `None` for any other kind of drag.
pub fn dragged(ev: &leptos::ev::DragEvent) -> Option<String> {
    read(ev, BOOK_MIME)
}

/// The shelf a drag is carrying, or `None` for any other kind of drag.
pub fn dragged_folder(ev: &leptos::ev::DragEvent) -> Option<String> {
    read(ev, FOLDER_MIME)
}

/// Whether the drag data names one of ours.
///
/// Asked of the TYPE LIST rather than of the payload, because a dragover runs
/// with the drag data store protected: `getData` answers with an empty string
/// until the drop, so a target that read the payload to decide whether to claim
/// would decide "not mine" on every engine that enforces that — and a dragover
/// nobody claims is a `drop` that never fires. The payload itself is read on the
/// drop, where the store is readable again. The payload is asked for first,
/// though, so an engine that does answer early gives the precise answer rather
/// than the permissive one.
fn carries(ev: &leptos::ev::DragEvent, mime: &str) -> bool {
    let Some(transfer) = ev.data_transfer() else {
        return false;
    };
    if let Ok(payload) = transfer.get_data(mime)
        && !payload.is_empty()
    {
        return true;
    }
    transfer
        .types()
        .iter()
        .any(|kind| kind.as_string().is_some_and(|name| name == mime))
}

/// Claim a dragover as ours, so the browser offers a drop. Without this the
/// cursor says "no" and `drop` never fires, which is why every target calls one
/// of these three before deciding anything else.
fn claim(ev: &leptos::ev::DragEvent, ours: bool) -> bool {
    if !ours {
        return false;
    }
    ev.prevent_default();
    if let Some(transfer) = ev.data_transfer() {
        transfer.set_drop_effect("move");
    }
    true
}

/// Claim a dragover carrying a BOOK. What a card and a row call: the insertion
/// line they draw is an index in a list of books, and offering it to a shelf
/// being nested would promise a position this drop does not honour.
pub fn accepts_book(ev: &leptos::ev::DragEvent) -> bool {
    claim(ev, carries(ev, BOOK_MIME))
}

/// Claim a dragover carrying a SHELF. What a folder card calls before it asks
/// `library_core::shelf::can_nest`, so a folder that would close a loop is not
/// offered a drop at all.
pub fn accepts_folder(ev: &leptos::ev::DragEvent) -> bool {
    claim(ev, carries(ev, FOLDER_MIME))
}

/// Claim a dragover carrying either. What the containers call — the grid's empty
/// space and a folder card both take a book and a shelf.
pub fn accept(ev: &leptos::ev::DragEvent) -> bool {
    claim(ev, carries(ev, BOOK_MIME) || carries(ev, FOLDER_MIME))
}

/// The order the page is showing, so a drop on a card can name the index it
/// landed at. Provided by `crate::features::library::content` and read by the
/// cards and the rows; a card cannot work the index out from its own DOM without
/// counting siblings, which is a second definition of the order.
#[derive(Clone, Copy)]
pub struct ShelfOrder(pub Signal<Vec<library_core::book::Book>>);

/// The shelves the page is showing at this level, in the order it shows them.
///
/// Provided beside [`ShelfOrder`] and for the same reason: the grid renders the
/// folders before the books, the selection bar's "All" has to mean everything on
/// screen rather than everything in the library, and a card that derived the
/// level itself would be a second answer to "what is here".
#[derive(Clone, Copy)]
pub struct FolderOrder(pub Signal<Vec<Shelf>>);

/// The card a book is being held over. One signal for the whole page, so exactly
/// one insertion line is drawn at a time and leaving a card clears the one
/// before it without either card knowing about the other.
///
/// Namespaced by value rather than by signal: a folder card writes
/// `"folder:{id}"` where a book card writes the bare id, and a shelf id is never
/// one. Both markers are driven by the one signal so only one is ever lit.
#[derive(Clone, Copy)]
pub struct DropTarget(pub RwSignal<Option<String>>);

impl DropTarget {
    /// The marker a folder card claims, so its own lift is distinguishable from
    /// the insertion line a book card draws.
    pub fn folder(shelf_id: &str) -> String {
        format!("folder:{shelf_id}")
    }

    /// Clear the marker, but only when it is still ours: the target being
    /// entered has already written itself, and undoing that here would leave no
    /// marker at all for the frame between the two events.
    pub fn release(&self, marker: &str) {
        self.0.update(|at| {
            if at.as_deref() == Some(marker) {
                *at = None;
            }
        });
    }
}
