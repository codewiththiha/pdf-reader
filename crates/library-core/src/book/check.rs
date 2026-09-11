//! The path check: what a walk's measurement does to the rows it lands on.
//!
//! A file that moved inside a watched tree is the same book at a new address, and
//! a book that was migrated from the previous schema carries a placeholder identity
//! nothing ever measured. The check is where the walk's measurement replaces the
//! placeholder, and where a row that turns out to be a twin of another is folded
//! into it.

use super::{Book, Row, book_rows, book_rows_mut};

/// Apply one path check to every book at that address, and say which books it
/// touched.
///
/// Every book at the address, for [`record_read`]'s reason: two rows of one
/// file — a duplicate the reader kept — share the address's fate, and a check
/// that healed one and left the other pending would hold every watched
/// folder's rescan off forever. An independent row is no exception, and this
/// is the one place its opt-out stops: whether the file resolves is a fact
/// about the file, so a private book of a deleted file is a missing book like
/// any other.
///
/// Three outcomes, and the difference between them is the whole reason a
/// library survives a moved folder:
///
///   * the address resolved — the fingerprint becomes the measurement, the
///     pending mark clears, and the book is not missing;
///   * the address did not resolve — the book becomes `missing` and keeps its
///     row, its resume point and every shelf it is on, so a relink can heal it
///     and the reader can still see what they lost. The pending mark clears here
///     too: it means "no check has run", not "the check came back good", and a
///     book left pending forever would hold every watched folder's rescan off;
///   * there is no book at that address — nothing to do.
///
/// Returns the ids the check CHANGED, empty in the third case and for a pass
/// over rows nothing moved, so a startup sweep of a healthy library writes no
/// state at all.
pub fn apply_check(rows: &mut [Row], check: &crate::wire::PathCheck) -> Vec<String> {
    let measured = check.fingerprint();
    let mut touched = Vec::new();
    for book in book_rows_mut(rows).filter(|b| b.path() == check.path) {
        let changed = match measured {
            Some(fp) => {
                let changed = book.fp != fp || book.missing || book.fp_pending;
                book.heal(fp);
                changed
            }
            None => {
                // Going missing is news; being told twice is not. A book that
                // was still owed its first measurement is news too, because the
                // pending mark is what holds a watched folder's rescan off.
                let changed = !book.missing || book.fp_pending;
                book.missing = true;
                book.fp_pending = false;
                changed
            }
        };
        if changed {
            touched.push(book.id.clone());
        }
    }
    touched
}

/// Add an imported book at the END of the library's order, or return the id of
/// the one already there. Content identity decides, not the address: the same
/// file reached through a second watched folder is the same book (see
/// [`crate::ledger`]).
///
/// The first row wins when a shelf holds duplicates the reader asked to keep —
/// an import that re-finds a file the library already has twice resolves to
/// the row the library lists first, and which shelf the book then lands on is
/// a question the app's conflict sheet asks BEFORE this is reached: a
/// fingerprint already FILED on a shelf is a duplicate/replace/merge choice and
/// not an add, so an import that reaches this with one is either a row nobody
/// has filed — the library's own row for that content, which the arrival
/// becomes a second membership of — or a second file of one content inside a
/// single batch.
///
/// Appended rather than pushed to the front, because an import arrives in the
/// order the walk produced — depth-first and alphabetical, which is the order the
/// folder itself lists them — and filing 400 books at the front one at a time
/// would hand the reader that order backwards. A book the reader OPENED goes to
/// the front instead, through [`record_read`]: that is a "what was I reading"
/// question, and this is a "what is on the shelf" one.
pub fn add_book(rows: &mut Vec<Row>, book: Book) -> String {
    // A shared row and never an independent one: an independent row is a
    // reader's private book of the file, and an import that resolved to it
    // would file that private book on a shelf the question never mentioned.
    // A content only a private row holds is a content this import adds, which
    // is what the reader asked for by importing it again. A link has no
    // fingerprint at all, so it is never the answer either.
    if let Some(existing) = book_rows(rows).find(|b| b.fp == book.fp && !b.independent) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    rows.push(Row::Book(book));
    id
}

/// Remove a ROW by id, returning it — a book or a link, since a shelf holds
/// both and a removal is the same act on either. The caller decides what else
/// the removal implies: a folder ledger tombstone ([`crate::ledger::tombstone`]),
/// a shelf membership ([`crate::shelf::forget`]), the store copy when the app
/// owns the bytes, and — for a book — every link that pointed at it, which is
/// [`drop_dangling_links`]'s job and not this one.
pub fn remove_row(rows: &mut Vec<Row>, id: &str) -> Option<Row> {
    let at = rows.iter().position(|r| r.id() == id)?;
    Some(rows.remove(at))
}

/// Drop every link whose target is no longer a book in the list.
///
/// A link is a pointer, and a pointer at nothing is a row that renders, is
/// clicked, and does nothing — the one failure mode a link has. Called after
/// any removal that could have taken a book a link was pointing at, and by
/// [`sanitize`] on every load, which is what makes a hand-edited blob or a
/// library written by a build that removed rows differently still open on a
/// shelf with no dead rows in it.
///
/// A link may also point at a SHELF — the folder link a merged import leaves
/// behind — and a shelf id is never a book id (the letter prefixes are
/// disjoint, [`crate::id::is_shelf`]), so this sweep keeps every shelf pointer:
/// which shelves are gone is a question about the shelf list, and
/// [`drop_dead_shelf_links`] is the sweep that can answer it.
pub fn drop_dangling_links(rows: &mut Vec<Row>) {
    // Owned ids, so the set does not hold a borrow of the list the retain below
    // is about to walk mutably.
    let books: std::collections::HashSet<String> =
        book_rows(rows).map(|b| b.id.clone()).collect();
    rows.retain(|r| {
        !matches!(r, Row::Link { target, .. }
            if !books.contains(target) && !crate::id::is_shelf(target))
    });
}

/// Drop every link that points at a shelf the list no longer holds — the
/// shelf half of [`drop_dangling_links`], which needs the shelf list to answer
/// and so lives one call away from it: [`crate::blob::sanitize`] runs it with
/// both lists in hand, and a shelf being taken apart runs it over the rows the
/// moment its pointer would go dead.
pub fn drop_dead_shelf_links(rows: &mut Vec<Row>, shelves: &[crate::shelf::Shelf]) {
    rows.retain(|r| match r {
        Row::Link { target, .. } if crate::id::is_shelf(target) => {
            shelves.iter().any(|s| &s.id == target)
        }
        _ => true,
    });
}
