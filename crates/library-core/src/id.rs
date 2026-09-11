//! Book and shelf ids.
//!
//! An id is the drag payload, the shelf member and the row a relink writes
//! back to, so it has to be stable across sessions and unique across a library
//! — but it never has to be unpredictable, sortable across machines, or
//! anything else a ULID crate would sell. A timestamp plus a counter is all
//! the guarantee the library needs, and it keeps the wasm bundle free of a
//! dependency whose only job here is to look random.
//!
//! The counter is the crate's own ([`next_id`] and its siblings) rather than
//! the caller's, and that is a correctness rule rather than a convenience:
//! two folder imports run concurrently (every watched folder rescans on one
//! focus), and a seq derived from the length of a list each task snapshotted
//! BEFORE its own walk is the same number minted twice in the same
//! millisecond — two different books wearing one id, which the next load's
//! sanitize resolves by dropping one of them.

use std::sync::atomic::{AtomicU32, Ordering};

/// The process's own minting counter. Relaxed ordering: the webview is
/// single-threaded, so this only has to hand out distinct numbers, never
/// synchronise anything.
static SEQ: AtomicU32 = AtomicU32::new(0);

fn next_seq() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// A fresh book id, minted from the crate's own counter. This is the id every
/// joining book gets — a scan, a drop, a restore, a hand-open.
pub fn next_id(now_ms: u64) -> String {
    new_id(now_ms, next_seq())
}

/// A fresh shelf id, off the same counter as [`next_id`]: one sequence across
/// the three kinds is one less thing two mints can disagree about.
pub fn next_shelf_id(now_ms: u64) -> String {
    new_shelf_id(now_ms, next_seq())
}

/// A fresh watched-folder id, off the same counter.
pub fn next_folder_id(now_ms: u64) -> String {
    new_folder_id(now_ms, next_seq())
}

/// Whether a token is a SHELF's id rather than a book's — the letter prefix
/// is what makes the two kinds disjoint, so a link row's target can name
/// either kind and a reader of the row list alone can tell them apart.
pub fn is_shelf(id: &str) -> bool {
    id.starts_with('s')
}

/// A fresh id: the millisecond it was minted at, plus a per-millisecond
/// counter so two books imported in the same tick differ.
///
/// The explicit-seq form is public for the one mint that is deliberately
/// deterministic: the `v1` migration ([`crate::blob::migrate::migrate_v1`]), which
/// numbers a list it is handed in one pass and must produce the same ids if it
/// ever runs twice over the same legacy blob. Everything else mints through
/// [`next_id`]. Rendered in lower-case hex, which is 11 + 4 characters and reads
/// as an opaque token in a shelf member list.
pub fn new_id(now_ms: u64, seq: u32) -> String {
    format!("b{now_ms:011x}{seq:04x}")
}

/// The shelf's explicit-seq form. Crate-private, and the reason is the rule the
/// module doc gives: an id is minted off THIS counter or it is not an id the
/// next concurrent mint can be sure differs from. The migration is the one
/// caller that has a sequence of its own to hand over, and it mints books.
fn new_shelf_id(now_ms: u64, seq: u32) -> String {
    format!("s{now_ms:011x}{seq:04x}")
}

/// A watched folder's id, explicit-seq form. See [`new_shelf_id`].
fn new_folder_id(now_ms: u64, seq: u32) -> String {
    format!("f{now_ms:011x}{seq:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_mints_in_one_tick_never_share_an_id() {
        // Two import tasks finishing their walks in the same millisecond: the
        // counter is the crate's, so the second mint differs from the first
        // and no caller has to know what the other one minted.
        let now = 1_700_000_000_000;
        let mut all = vec![next_id(now), next_id(now), next_shelf_id(now), next_folder_id(now)];
        all.sort();
        all.dedup();
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn an_id_is_one_token_a_shelf_can_hold() {
        let id = new_id(1_700_000_000_000, 12);
        assert!(id.starts_with('b'));
        assert!(!id.contains(char::is_whitespace));
        assert_eq!(id.len(), 16);
    }

    #[test]
    fn the_prefix_says_which_kind_a_token_is() {
        let (now, seq) = (1_700_000_000_000, 3);
        assert!(is_shelf(&new_shelf_id(now, seq)));
        assert!(!is_shelf(&new_id(now, seq)));
        assert!(!is_shelf(&new_folder_id(now, seq)));
        assert!(!is_shelf(""));
    }

    #[test]
    fn the_three_kinds_never_collide() {
        // A shelf member list and a folder id can land in the same blob; the
        // letter prefix is what keeps a book from being read as either.
        let (now, seq) = (1_700_000_000_000, 3);
        let ids = [new_id(now, seq), new_shelf_id(now, seq), new_folder_id(now, seq)];
        let mut distinct = ids.to_vec();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), 3);
    }

    #[test]
    fn minting_in_order_is_stable_across_a_session() {
        // Two scans of the same folder in the same millisecond must not hand
        // out the same id twice, which is what the sequence half is for.
        let first: Vec<String> = (0..4096).map(|i| new_id(7, i)).collect();
        let mut sorted = first.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), first.len(), "4096 ids in one tick, all distinct");
    }
}
