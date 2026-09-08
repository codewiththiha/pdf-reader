//! Book and shelf ids.
//!
//! An id is the drag payload, the shelf member and the row a relink writes
//! back to, so it has to be stable across sessions and unique across a library
//! — but it never has to be unpredictable, sortable across machines, or
//! anything else a ULID crate would sell. A timestamp plus the position the
//! book was minted at is all the guarantee the library needs, and it keeps the
//! wasm bundle free of a dependency whose only job here is to look random.

/// A fresh id: the millisecond it was minted at, plus a per-millisecond
/// counter so two books imported in the same tick differ.
///
/// `seq` is the caller's own counter — the length of the list the book is
/// joining is enough, since a scan mints in order and a millisecond holds at
/// most a few thousand files. Rendered in lower-case hex, which is 11 + 4
/// characters and reads as an opaque token in a shelf member list.
pub fn new_id(now_ms: u64, seq: u32) -> String {
    format!("b{now_ms:011x}{seq:04x}")
}

/// The letter an id kind is prefixed with, so a shelf member can be told from
/// a folder id at a glance in a persisted blob. Both are minted here.
pub fn new_shelf_id(now_ms: u64, seq: u32) -> String {
    format!("s{now_ms:011x}{seq:04x}")
}

/// A watched folder's id.
pub fn new_folder_id(now_ms: u64, seq: u32) -> String {
    format!("f{now_ms:011x}{seq:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_one_tick_still_differ() {
        assert_ne!(new_id(1, 0), new_id(1, 1));
        assert_ne!(new_id(1, 0), new_id(2, 0));
    }

    #[test]
    fn an_id_is_one_token_a_shelf_can_hold() {
        let id = new_id(1_700_000_000_000, 12);
        assert!(id.starts_with('b'));
        assert!(!id.contains(char::is_whitespace));
        assert_eq!(id.len(), 16);
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
