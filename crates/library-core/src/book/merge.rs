//! How two books that turn out to be one fold back into one.
//!
//! One content is one identity and one identity is one row, so a duplicate is not
//! a second book to keep — it is a row to fold, and folding has to keep whichever
//! of the two the reader got further in, whichever name is real, and both shelves'
//! memberships.

use super::{Book, ReadPoint};

/// Fold one book into another: `gone` dissolves and `survivor` keeps its id,
/// its name and its address, taking everything the survivor does not already
/// know.
///
/// One function and no table of policies, because there is one fold and one
/// question that asks for it — a row moved onto a level that already holds its
/// name — and the rule per field is the one a reader means by "these are the
/// same book":
///
///   * the place in it is the FURTHER of the two, and on a page tie the deeper
///     stream fraction, because a merge must never send a reader backwards;
///   * the page COUNT is the best either row ever knew even when the resume
///     point came from the other — a merged book that knew 300 pages must not
///     go back to not knowing;
///   * a name and an author fill a gap and never overwrite, so the title a
///     document gave at first open survives a fold with a row that had none;
///   * the stamps keep the first join and the last read, with `0` ("never") as
///     a gap the other side fills rather than as the dawn of time;
///   * a measurement beats a placeholder, and an address is dead only when
///     BOTH rows say so;
///   * the survivor's identity is untouched — its id, its pipeline, its address
///     and its independence are what every shelf membership and every key in
///     storage already names, and a fold that moved them would orphan both.
///
/// The marks are NOT a field of a `Book`: they live in the app's own storage
/// under a key this crate cannot see, so the caller folds them
/// (`services::library::conflict` does, before it drops the row). A fold
/// that forgot would be a merge that deleted one side's highlights.
pub fn fold_books(survivor: &mut Book, gone: &Book) {
    // The resume point travels as one unit: a page without its count is a
    // position the progress bar cannot draw, and a fraction without its page is
    // half a stream reading.
    let mine = ReadPoint {
        page: survivor.page,
        num_pages: survivor.num_pages,
        fraction: survivor.fraction,
    };
    let theirs = ReadPoint {
        page: gone.page,
        num_pages: gone.num_pages,
        fraction: gone.fraction,
    };
    let point = further_point(mine, theirs).settled();
    survivor.page = point.page;
    survivor.fraction = point.fraction;
    survivor.num_pages = point.num_pages.max(survivor.num_pages).max(gone.num_pages);
    if crate::text::non_blank(survivor.title.as_deref()).is_none()
        && let Some(title) = crate::text::non_blank(gone.title.as_deref())
    {
        survivor.title = Some(title.to_string());
    }
    if survivor.author.is_none() {
        survivor.author = crate::text::non_blank(gone.author.as_deref()).map(str::to_string);
    }
    survivor.added_ms = earliest_known(survivor.added_ms, gone.added_ms);
    survivor.last_read_ms = survivor.last_read_ms.max(gone.last_read_ms);
    survivor.missing = survivor.missing && gone.missing;
    // The pending flag is the survivor's OWN before it is folded, and the fold
    // that reads it after overwriting it can never take a measurement: two
    // placeholders stay one, but a placeholder yields to a row that has been
    // weighed, because an unweighed row has nothing to defend its guess with.
    let was_pending = survivor.fp_pending;
    survivor.fp_pending = was_pending && gone.fp_pending;
    if was_pending && !gone.fp_pending {
        survivor.fp = gone.fp;
    }
}

/// The point that got FURTHER: the higher page, and on a page tie the deeper
/// stream fraction — a stream reader got somewhere a page count of zero cannot
/// name. A full tie keeps the survivor's own, which is what makes "the point
/// came from the other row" a fact worth reporting rather than a coin toss.
pub fn further_point(mine: ReadPoint, theirs: ReadPoint) -> ReadPoint {
    use std::cmp::Ordering;
    match theirs.page.cmp(&mine.page) {
        Ordering::Greater => theirs,
        Ordering::Less => mine,
        Ordering::Equal => match (mine.fraction, theirs.fraction) {
            (_, None) => mine,
            (None, Some(_)) => theirs,
            (Some(a), Some(b)) => {
                if b > a {
                    theirs
                } else {
                    mine
                }
            }
        },
    }
}

/// The earlier of two stamps, with `0` as the gap it is: a migrated row carries
/// no join stamp, and a fold that treated zero as the epoch would date every
/// book it touched to 1970.
fn earliest_known(mine: u64, theirs: u64) -> u64 {
    match (mine, theirs) {
        (0, other) | (other, 0) => other,
        _ => mine.min(theirs),
    }
}
