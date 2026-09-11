//! What a book is called, and what to call the second one.
//!
//! Two rules the shelf's surfaces all needed and each used to spell for itself: a
//! title that is a filename is not a title, and a name already on the shelf gets a
//! number rather than a collision.

/// The human-readable stem of an address, or the address when it has none. Never
/// empty, because it is the last fallback a search index and a shelf card both
/// rely on.
pub fn stem_of(path: &str) -> String {
    reader_core::filename::file_stem_from_path(path).unwrap_or_else(|| path.to_string())
}

/// The next free duplicate of `base`: `base_1`, `base_2`, and so on — the
/// counter a file manager appends when a second file of one name has to live
/// beside the first, and the name the library's conflict sheet gives a
/// duplicate the reader chose to keep.
///
/// `in_use` is every name the library already shows. The counter starts at the
/// first free number, and a trailing `_N` on `base` is stripped before
/// counting, so duplicating a duplicate steps instead of stacking: "Dune_1"
/// becomes "Dune_2" rather than "Dune_1_1" — the same reading a file manager
/// gives it, where the counter is not part of the name.
///
/// The result is a title the shelf can keep: never empty (a blank `base`
/// falls back to a word), and the trailing counter is exempt from the
/// filename-shaped rule [`sanitize`] applies to document-supplied titles (the
/// exemption is `reader_core::filename::is_usable_title`'s own — a name this
/// function minted survives the load that reads it back).
pub fn duplicate_title(base: &str, in_use: &std::collections::HashSet<String>) -> String {
    let base = base.trim();
    let root = if base.is_empty() { "Book" } else { base };
    // The counter rule is the filename policy's and not this crate's: the same
    // predicate decides whether a document-supplied title is download debris
    // wearing a copy counter, and a second spelling of it here would be a second
    // convention the moment either half was edited.
    let root = reader_core::filename::strip_copy_counter(root);
    (1u32..)
        .map(|n| format!("{root}_{n}"))
        .find(|candidate| !in_use.contains(candidate))
        .expect("an unbounded counter always finds a free name")
}
