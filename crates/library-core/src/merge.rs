//! Two rows that turn out to be the same book: the rule that folds them into
//! one.
//!
//! The library normally holds one row per content identity — but a reader who
//! was asked "this shelf already has that book" and answered *keep both* has
//! two, and a reader who later answers *merge* needs the two folded back
//! together without losing what either one knew. This module is that fold.
//!
//! ## The shape of the rule
//!
//! Every field of a [`Book`] is merged by a named [`Policy`], and
//! [`merge_books`] is the table applied: the EXISTING row is the survivor (it
//! keeps its id, because ids are what shelves and covers and the drag payload
//! speak), and the incoming row dissolves into it. One function per policy,
//! each pure and each tested, so the day a book grows a field the merge grows
//! a line — the struct literal in [`merge_books`] does not compile until the
//! new field has been given an answer, and [`POLICIES`] is where the answer is
//! written down.
//!
//! ## What the policies are
//!
//! The resume point is the one readers care about, and its rule is the one the
//! merge exists for: **the book that got further wins** — the reader reached
//! page 300 in one copy and page 12 in the other, so the merged book resumes
//! at 300. Ties go to the deeper stream fraction, and then to the existing
//! row. Everything else follows one of two instincts: facts about the FILE
//! (page count, fingerprint, format) take the best measurement either row
//! has, and facts about the READER (name, author, stamps) keep whichever side
//! knows more or, when both know, the survivor.
//!
//! The marks a reader made inside the book — the gloss highlights and the AI
//! answers hanging off them — are NOT fields of a `Book`: they live in the
//! app's own storage, keyed by address, and the app layer unions them under
//! the `Union` policy below (both sides' marks, deduped by spot). This crate
//! stays free of that dependency; the policy is named here so the table tells
//! the whole truth about a merge.
//!
//! ## What the fold reports
//!
//! [`merge_books`] answers with the merged row AND a [`MergeNotes`]: what the
//! fold kept and where the survivor's resume point came from. The notes exist
//! because a merge is the one answer a reader can never inspect after the
//! fact — the two rows became one — so the surface that offers it says what
//! it will keep BEFORE the click (the conflict sheet's Merge row is built
//! from a dry run of exactly this). The resume half is computed here, beside
//! the rule it reports; the marks half is the app layer's to fill in, because
//! the marks live in the app's storage and this crate has no window into it.

use serde::{Deserialize, Serialize};

use crate::book::{Book, Fingerprint, Origin, ReadPoint};

/// How one field of a book is combined when two rows of the same book fold
/// into one. The vocabulary of the merge — [`merge_books`] is each variant
/// applied to the field [`POLICIES`] names it for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Policy {
    /// The surviving row's value wins outright. What identity fields take:
    /// the survivor IS the existing row, so its id and its pipeline are the
    /// merge's.
    Existing,
    /// The dissolving row's value wins outright. No field takes this today;
    /// it is the mirror of [`Policy::Existing`] and the answer a future field
    /// may want ("the newer measurement replaces the old").
    Incoming,
    /// The value showing the reader got FURTHER wins: the higher page, and on
    /// a page tie the deeper stream fraction. The resume point's rule — a
    /// merge must never send a reader backwards.
    Furthest,
    /// The earlier of two stamps wins, with `0` ("never") treated as a gap
    /// the other side fills rather than as the dawn of time.
    Earliest,
    /// The later of two stamps wins.
    Latest,
    /// The existing value wins unless it is empty, and then the incoming one
    /// fills the gap — the rule [`crate::book::record_read`] already keeps for
    /// titles and authors, extended to a second row.
    FillGap,
    /// A measurement beats a placeholder: whichever side actually weighed the
    /// file has the truth, and a fingerprint still pending its first check
    /// does not.
    Measured,
    /// Both sides' values are kept, deduped by identity. The gloss marks'
    /// policy — applied in the app layer, which owns that storage (see the
    /// module docs).
    Union,
}

/// The field-by-field table [`merge_books`] implements, as data: what a merge
/// does with every field a `Book` has, in the order the struct declares them.
///
/// Kept beside the function on purpose — a UI that ever wants to TELL a reader
/// what merging means reads this instead of re-deriving it, and a reviewer
/// reads one list instead of walking a function. The gloss rows are the app
/// layer's side of the merge, named here so the table is the whole story.
pub const POLICIES: &[(&str, Policy)] = &[
    ("id", Policy::Existing),
    ("fp", Policy::Measured),
    ("title", Policy::FillGap),
    ("author", Policy::FillGap),
    ("format", Policy::Existing),
    ("origin", Policy::Existing), // unless the survivor's address died — see `live_origin`
    ("added_ms", Policy::Earliest),
    ("last_read_ms", Policy::Latest),
    ("page", Policy::Furthest),
    ("num_pages", Policy::Furthest), // the winner's count, and never less than either knew
    ("fraction", Policy::Furthest),
    ("missing", Policy::Existing),   // only missing when BOTH addresses are dead
    ("fp_pending", Policy::Existing), // only pending when NEITHER was measured
    ("gloss marks", Policy::Union),
    ("gloss answers", Policy::Union), // ride the marks' ids, so unioning marks unions them
];

/// What a fold kept, and where the survivor's values came from — the report
/// [`merge_books`] answers with beside the merged row.
///
/// The sheet-facing half of the merge: a Merge row that promises "4
/// highlights kept · resumes at page 12 (the further of the two)" is built
/// from a dry run of the very fold it offers, so the promise and the write
/// cannot drift. The resume fields are filled by [`merge_books`] itself; the
/// mark counts are the app layer's to fill, because the marks live in the
/// app's own address-keyed storage (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeNotes {
    /// Marks the survivor's address already held — the ones nothing had to
    /// move for the fold to keep.
    pub marks_kept: usize,
    /// Marks that travelled over from an address the fold leaves behind.
    pub marks_added: usize,
    /// The merged book's resume page — the further of the two, per
    /// [`Policy::Furthest`].
    pub resume_page: u32,
    /// True when the resume point came from the dissolving row: the reader
    /// got further in the copy that is about to disappear, and the survivor
    /// carries that reading forward.
    pub resume_from_incoming: bool,
}

/// Fold two rows of the same book into the survivor.
///
/// `existing` is the row that stays — the merged book carries its id, and the
/// caller transfers whatever the `incoming` row held that the id cannot: shelf
/// memberships, and the side data keyed by an address the survivor does not
/// read from. `incoming` is what the fold consumes; the caller drops the row
/// afterwards.
///
/// The field rules are [`POLICIES`]'s, and every one of them is total: any
/// two rows produce a merged row, in any order, without the caller deciding
/// anything. Not symmetric on purpose — `merge_books(a, b)` and
/// `merge_books(b, a)` agree on every fact about the FILE and the READER's
/// progress, and differ only in whose identity survives, which is the
/// caller's choice to make once (the row already on the shelf is the
/// survivor) rather than a coin toss per field.
///
/// The [`MergeNotes`] that come back report the resume decision; the mark
/// counts in them are the app layer's half of the fold to fill in.
pub fn merge_books(existing: &Book, incoming: &Book) -> (Book, MergeNotes) {
    // The resume point is decided as one unit — page, count and fraction
    // travel together, because a page without its count is a position the
    // progress bar cannot draw and a fraction without its page is half a
    // stream reading.
    let mine = read_point(existing);
    let theirs = read_point(incoming);
    let point = further_point(mine, theirs);
    let notes = MergeNotes {
        resume_page: point.page,
        // A full tie keeps the survivor's own point, so "from incoming" is
        // the point travelling AND the two actually differing — a fold of
        // two untouched copies reports nothing travelling.
        resume_from_incoming: point == theirs && mine != theirs,
        ..MergeNotes::default()
    };
    let book = Book {
        id: existing.id.clone(),
        fp: measured_fp(existing, incoming),
        title: fill_gap(existing.title.clone(), incoming.title.clone()),
        author: fill_gap(existing.author.clone(), incoming.author.clone()),
        format: existing.format,
        origin: live_origin(existing, incoming),
        added_ms: earliest_known(existing.added_ms, incoming.added_ms),
        last_read_ms: existing.last_read_ms.max(incoming.last_read_ms),
        page: point.page,
        // The count is a fact about the file, so the best measurement either
        // row ever saw wins even when the resume point came from the other —
        // a merged book that knew 300 pages must not go back to not knowing.
        num_pages: point.num_pages.max(existing.num_pages).max(incoming.num_pages),
        fraction: point.fraction,
        missing: existing.missing && incoming.missing,
        fp_pending: existing.fp_pending && incoming.fp_pending,
    };
    (book, notes)
}

/// The resume point of a row, as the reader's own value.
fn read_point(book: &Book) -> ReadPoint {
    ReadPoint {
        page: book.page,
        num_pages: book.num_pages,
        fraction: book.fraction,
    }
}

/// The point that shows the reader got further: the higher page, and on a
/// page tie the deeper stream fraction. A full tie keeps `a` — the existing
/// row's — so a merge of two untouched copies moves nothing.
///
/// Public because the rule is the answer to a question the app asks on its
/// own ("which of these two rows is the reader further in?") wherever two
/// rows of one book are on screen, and one definition is one fewer place for
/// the furthest-read rule to drift.
pub fn further_point(a: ReadPoint, b: ReadPoint) -> ReadPoint {
    match a.page.cmp(&b.page) {
        std::cmp::Ordering::Greater => a,
        std::cmp::Ordering::Less => b,
        std::cmp::Ordering::Equal => match (a.fraction, b.fraction) {
            (Some(x), Some(y)) => if y > x { b } else { a },
            // A fraction is a position a page is not: the stream reader got
            // somewhere the page reader cannot name.
            (None, Some(_)) => b,
            _ => a,
        },
    }
}

/// [`Policy::Measured`]: the fingerprint the row that actually weighed the
/// file carries. A pending placeholder loses to any measurement, and when
/// neither row was ever measured the survivor's placeholder stays — there is
/// nothing better to take, and the check that follows replaces it.
fn measured_fp(existing: &Book, incoming: &Book) -> Fingerprint {
    if existing.fp_pending && !incoming.fp_pending {
        incoming.fp
    } else {
        existing.fp
    }
}

/// [`Policy::FillGap`]: the survivor's value unless it is empty, and then the
/// incoming one — for the two fields where an empty is a gap and not an
/// answer.
fn fill_gap(existing: Option<String>, incoming: Option<String>) -> Option<String> {
    let blank = |s: &Option<String>| s.as_deref().map(str::trim).unwrap_or("").is_empty();
    match (blank(&existing), blank(&incoming)) {
        (false, _) => existing,
        (true, false) => incoming,
        (true, true) => None,
    }
}

/// [`Policy::Existing`] for the address, with the one exception a merge
/// exists for: a survivor whose address died takes the incoming row's living
/// one. The merged book must open — a merge that kept a dead address beside a
/// live one merged two rows into one missing book.
fn live_origin(existing: &Book, incoming: &Book) -> Origin {
    if existing.missing && !incoming.missing {
        incoming.origin.clone()
    } else {
        existing.origin.clone()
    }
}

/// [`Policy::Earliest`] with `0` as the gap it is: a migrated row carries no
/// join stamp, and "joined at the epoch" would sort it above every real date.
fn earliest_known(a: u64, b: u64) -> u64 {
    match (a, b) {
        (0, _) => b,
        (_, 0) => a,
        _ => a.min(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Origin;
    use reader_core::format::Format;

    fn fp(size: u64, mtime: u64, head: u32) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    fn book(id: &str, path: &str) -> Book {
        Book::new(
            id.to_string(),
            fp(100, 5, 9),
            Format::Pdf,
            Origin::Linked { src: path.to_string() },
            10,
        )
    }

    #[test]
    fn the_survivor_keeps_its_identity() {
        let (merged, _) = merge_books(&book("keep", "/a.pdf"), &book("gone", "/a.pdf"));
        assert_eq!(merged.id, "keep");
        assert_eq!(merged.format, Format::Pdf);
        assert_eq!(merged.path(), "/a.pdf");
    }

    #[test]
    fn the_reader_who_got_further_decides_the_resume() {
        let mut existing = book("a", "/books/dune.pdf");
        existing.page = 12;
        existing.num_pages = 300;
        let mut incoming = book("b", "/books/dune.pdf");
        incoming.page = 240;
        incoming.num_pages = 300;
        // Either order agrees on the point: the furthest read is a fact about
        // the reader, not about which row the caller named first. The notes
        // say which side it travelled from.
        let (merged, notes) = merge_books(&existing, &incoming);
        assert_eq!(merged.page, 240, "a merge never sends a reader backwards");
        assert_eq!(merged.num_pages, 300);
        assert!(notes.resume_from_incoming, "the point came from the arrival");
        assert_eq!(notes.resume_page, 240);
        let (merged, notes) = merge_books(&incoming, &existing);
        assert_eq!(merged.page, 240);
        assert!(
            !notes.resume_from_incoming,
            "the further point was already the survivor's"
        );
        // A full tie keeps the survivor's own point, and reports nothing
        // travelling.
        let (_, notes) = merge_books(&existing, &existing);
        assert!(!notes.resume_from_incoming);
        assert_eq!(notes.resume_page, 12);
        // The page count survives from whichever row knew it, even when the
        // resume point came from the row that did not.
        incoming.num_pages = 0;
        let (merged, _) = merge_books(&existing, &incoming);
        assert_eq!(merged.page, 240);
        assert_eq!(merged.num_pages, 300);
    }

    #[test]
    fn a_page_tie_goes_to_the_deeper_stream_fraction() {
        let a = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.4) };
        let b = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.7) };
        assert_eq!(further_point(a, b), b);
        assert_eq!(further_point(b, a), b);
        // A fraction beats no fraction on the same page — a stream reader got
        // somewhere a page count of zero cannot name.
        let plain = ReadPoint { page: 10, num_pages: 0, fraction: None };
        assert_eq!(further_point(plain, a), a);
        // A full tie keeps the first: the existing row's own point.
        assert_eq!(further_point(plain, plain), plain);
        assert_eq!(further_point(a, a), a);
    }

    #[test]
    fn names_fill_gaps_and_never_overwrite() {
        let mut existing = book("a", "/one.pdf");
        existing.title = Some("Dune".into());
        let mut incoming = book("b", "/one.pdf");
        incoming.author = Some("Frank Herbert".into());
        let (merged, _) = merge_books(&existing, &incoming);
        assert_eq!(merged.title.as_deref(), Some("Dune"));
        assert_eq!(merged.author.as_deref(), Some("Frank Herbert"));
        // The incoming name only ever fills: a survivor that has one keeps it.
        incoming.title = Some("Dune_1".into());
        assert_eq!(merge_books(&existing, &incoming).0.title.as_deref(), Some("Dune"));
        // A blank is a gap, not a value.
        existing.title = Some("   ".into());
        assert_eq!(merge_books(&existing, &incoming).0.title.as_deref(), Some("Dune_1"));
    }

    #[test]
    fn a_measurement_beats_a_placeholder() {
        let mut pending = book("a", "/one.pdf");
        pending.fp = Fingerprint::placeholder("/one.pdf");
        pending.fp_pending = true;
        let measured = book("b", "/one.pdf");
        let (merged, _) = merge_books(&pending, &measured);
        assert_eq!(merged.fp, measured.fp);
        assert!(!merged.fp_pending, "the merged row has been weighed");
        // And the other way round changes nothing: the measured row leads.
        let (merged, _) = merge_books(&measured, &pending);
        assert_eq!(merged.fp, measured.fp);
        assert!(!merged.fp_pending);
        // Two placeholders stay one: nothing has been measured, so the
        // survivor's stands until the check that follows.
        let (both, _) = merge_books(&pending, &pending);
        assert!(both.fp_pending);
        assert_eq!(both.fp, pending.fp);
    }

    #[test]
    fn a_dead_address_yields_to_a_living_one() {
        let mut dead = book("a", "/gone/dune.pdf");
        dead.missing = true;
        let alive = book("b", "/books/dune.pdf");
        let (merged, _) = merge_books(&dead, &alive);
        assert_eq!(merged.path(), "/books/dune.pdf", "the merged book opens");
        assert!(!merged.missing);
        // A living survivor keeps its address whatever the incoming row is.
        assert_eq!(merge_books(&alive, &dead).0.path(), "/books/dune.pdf");
        // Only missing when both are.
        let mut also_dead = book("c", "/gone/two.pdf");
        also_dead.missing = true;
        assert!(merge_books(&dead, &also_dead).0.missing);
    }

    #[test]
    fn stamps_keep_the_first_join_and_the_last_read() {
        let mut existing = book("a", "/one.pdf");
        existing.added_ms = 500;
        existing.last_read_ms = 900;
        let mut incoming = book("b", "/one.pdf");
        incoming.added_ms = 300;
        incoming.last_read_ms = 700;
        let (merged, _) = merge_books(&existing, &incoming);
        assert_eq!(merged.added_ms, 300, "the book joined when it first joined");
        assert_eq!(merged.last_read_ms, 900, "and was read as recently as it was");
        // Zero is "never", not the epoch: a migrated row's blank stamp does
        // not outdate a real one.
        incoming.added_ms = 0;
        assert_eq!(merge_books(&existing, &incoming).0.added_ms, 500);
        existing.added_ms = 0;
        assert_eq!(merge_books(&existing, &incoming).0.added_ms, 0, "neither knows, so neither invents");
    }

    #[test]
    fn the_policy_table_is_the_merge_in_data() {
        // The rows a reviewer (or a future UI) reads instead of the function.
        // A Book field added without a table row and without a merge_books
        // line fails to compile there and is caught by the count here.
        for (field, policy) in [
            ("id", Policy::Existing),
            ("fp", Policy::Measured),
            ("title", Policy::FillGap),
            ("author", Policy::FillGap),
            ("added_ms", Policy::Earliest),
            ("last_read_ms", Policy::Latest),
            ("page", Policy::Furthest),
            ("gloss marks", Policy::Union),
        ] {
            assert!(
                POLICIES.iter().any(|(f, p)| *f == field && *p == policy),
                "{field} is not merged by {policy:?} in the table"
            );
        }
        // Every Book field, plus the two rows of app-side gloss storage.
        assert_eq!(POLICIES.len(), 15, "a new field means a new row");
    }
}
