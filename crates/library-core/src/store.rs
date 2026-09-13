//! Where a book's bytes live on disk: one folder per book, keyed by an id that
//! never changes.
//!
//! The store used to be flat and name-derived — every PDF in one `Library/pdf/`
//! bucket, each file called `<stem>_<id>.<ext>` after the filename it was copied
//! from. That gave a book no directory of its own, so its cover and its
//! highlights had to live somewhere else entirely (a cover cache and a gloss
//! store keyed by strings that are not the book's id), and a rename had nothing
//! stable to rename: the id was a suffix on a name that came from the source
//! file, not the primary key of anything on disk.
//!
//! One folder per book ends that. The id is minted once and never changes, so it
//! is the folder's name, and everything the app owns about a book — its bytes
//! when it copied them, its cover, its marks — lives inside that folder:
//!
//! ```text
//! <store_root>/items/<book_id>/
//!     source.<ext>    the bytes, present only for a stored book
//!     cover.webp      the rendered cover
//!     marks.json      the reader's highlights
//!     meta.json       an optional mirror of the book, for recovery
//! ```
//!
//! A linked book gets an item folder too — just without `source.*` — so its cover
//! and marks are colocated with it exactly as a stored book's are, and "where
//! does this book's stuff live" has one answer whatever its origin. Renaming the
//! title never touches the filesystem, because nothing on disk is named after the
//! title. Merging two books becomes "move the loser's `marks.json` and
//! `cover.webp` into the survivor's folder, delete the loser's" — the on-disk
//! twin of [`crate::book::fold_books`].
//!
//! This module is pure: it turns ids and extensions into path STRINGS and does no
//! I/O. The filesystem calls stay in the shell (`src-tauri`'s `commands`
//! module), which asks here what path to compute and then writes it — the same
//! split the rest of the crate keeps between a decision and the machine. Paths
//! are joined with `/` on every platform, the convention [`crate::folder::rel_under`]
//! and the shell's own walk already normalise to, and one every host's `Path`
//! reads back.

/// The directory under the store root that holds every book's item folder.
pub const ITEMS_DIR: &str = "items";

/// The file name a stored book's bytes wear inside its item folder, before its
/// extension: a PDF is `source.pdf`, a markdown file `source.md`. One predictable
/// stem, the format carried by the suffix, so nothing on disk is named after a
/// title that can be renamed.
const SOURCE_STEM: &str = "source";

/// The cover image's file name inside an item folder.
pub const COVER_FILE: &str = "cover.webp";

/// The highlights' file name inside an item folder.
pub const MARKS_FILE: &str = "marks.json";

/// The optional book-mirror's file name inside an item folder.
pub const META_FILE: &str = "meta.json";

/// The item-folder root under a store root: `<store_root>/items`.
///
/// The shell computes `<app_data_dir>/Library` and hands it here; everything
/// below lives under the one directory this returns, which is what keeps a
/// book's bytes, cover and marks inside the store the delete command's
/// containment check already guards.
pub fn items_root(store_root: &str) -> String {
    join(trim_sep(store_root), ITEMS_DIR)
}

/// The folder one book owns end to end: `<items_root>/<id>`.
///
/// The id is sanitised into a single component rather than trusted, even though
/// the crate mints it as an opaque alphanumeric token: a hand-edited blob or a
/// future id scheme must not be able to turn a folder name into a traversal.
pub fn item_dir(items_root: &str, book_id: &str) -> String {
    join(trim_sep(items_root), &component(book_id))
}

/// Where a stored book's bytes live: `<items_root>/<id>/source.<ext>`.
///
/// `ext` is the lower-case extension without its dot, the spelling
/// [`crate::scan::FoundFile::ext`] and the shell's `extension_of` both produce.
/// An empty extension yields a bare `source` with no suffix — which no admitted
/// format ever asks for, since the registry refuses an extension-less name, so
/// it is a belt-and-braces answer rather than a case the store lands in.
pub fn source_path(items_root: &str, book_id: &str, ext: &str) -> String {
    // The emptiness check is on the RAW extension: `component` maps an empty
    // string to its `item` fallback, which is right for a folder name and wrong
    // for "this source has no suffix". A non-empty extension is sanitised like
    // any other component, so a separator in it cannot climb out of the folder.
    let file = if ext.trim().is_empty() {
        SOURCE_STEM.to_string()
    } else {
        format!("{SOURCE_STEM}.{}", component(ext))
    };
    join(&item_dir(items_root, book_id), &file)
}

/// Where a book's rendered cover lives: `<items_root>/<id>/cover.webp`.
pub fn cover_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), COVER_FILE)
}

/// Where a book's highlights live: `<items_root>/<id>/marks.json`.
pub fn marks_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), MARKS_FILE)
}

/// Where a book's optional recovery mirror lives: `<items_root>/<id>/meta.json`.
pub fn meta_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), META_FILE)
}

/// The stem a migrated source keeps in its new name, which is every supported
/// extension lower-cased. The store's own batch used to name a copy after the
/// FILE it came from, so an old copy can wear any extension the registry
/// admits; the new layout names it after its format's pipeline instead, which is
/// one name per book rather than one per source filename.
///
/// `None` for an extension the registry does not know, which leaves the old name
/// alone: a migration that guessed at a format it could not name would be
/// renaming a file to something no reader can open.
pub fn migrated_ext(ext: &str) -> Option<&'static str> {
    match crate::scan::store_dir(ext) {
        "other" => None,
        dir => Some(dir),
    }
}

/// The id a copy in the OLD flat store was named with: the token after its last
/// underscore.
///
/// The old name was `<stem>_<id>.<ext>`, and an id is hex with a letter prefix,
/// so it never carries an underscore — the last one in the stem is the seam
/// however many underscores the source filename had. `None` for a name with no
/// seam, which is a file this app did not write and a migration leaves where it
/// is.
pub fn flat_store_id(file_name: &str) -> Option<String> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
    stem.rsplit_once('_')
        .map(|(_, id)| id.to_string())
        .filter(|id| !id.is_empty())
}

/// Whether a recorded store address still sits in the OLD flat bucket, and so is
/// a candidate for the one-time migration: directly inside one of the three
/// format directories under the store root, as `<root>/pdf/dune_ab12.pdf`.
///
/// Two directories are deliberately not candidates. The item layout puts a
/// folder between the bucket and the file, so anything under `items/` has already
/// moved; and `other/` never held a copy, because the format registry refuses an
/// extension it does not know and a store batch only ever copied a file it
/// admitted. A linked book's address is the reader's own file and is never a
/// candidate whatever it is called — the caller asks this of stored books only.
pub fn is_flat_store_path(store_root: &str, path: &str) -> bool {
    // A directory edge rather than a string prefix, so `/Library-old/x` is not
    // under `/Library` — the rule `crate::folder::rel_under` gives a watched
    // folder, applied to the one directory the app owns.
    let Some(rest) = crate::folder::rel_under(path, store_root) else {
        return false;
    };
    // One directory and one file, no deeper.
    let mut parts = rest.split('/');
    let (Some(dir), Some(_file), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    matches!(dir, "pdf" | "text" | "markdown")
}

/// Drop a trailing separator so a join never produces `root//child`. Both
/// separators are trimmed: a store root arrives from the host's own path API,
/// which on Windows ends in `\`.
fn trim_sep(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
}

/// Join one component onto a parent with `/`, the separator every path in the
/// ledger wears. An empty parent is the child alone, so a relative items root
/// stays relative rather than gaining a leading slash.
fn join(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

/// One path component, made safe to write: separators, the characters Windows
/// reserves, and control characters become `_`; the result is trimmed of the
/// dots and spaces that would make it a relative path, capped, and replaced with
/// a fallback when nothing is left. The shell's own sanitizer for a stored name,
/// moved into the crate that now owns the layout.
fn component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let capped: String = cleaned.trim_matches(['.', ' ']).chars().take(64).collect();
    match capped.as_str() {
        "" | "." | ".." => "item".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A book id of the shape the crate mints: an opaque alphanumeric token.
    const ID: &str = "b018c4f9e2a00001";

    #[test]
    fn the_items_root_is_one_directory_under_the_store() {
        assert_eq!(items_root("/app/Library"), "/app/Library/items");
        // A trailing separator on either host is trimmed, not doubled.
        assert_eq!(items_root("/app/Library/"), "/app/Library/items");
        assert_eq!(items_root("C:\\AppData\\Library\\"), "C:\\AppData\\Library/items");
    }

    #[test]
    fn a_book_owns_one_folder_named_by_its_id() {
        assert_eq!(
            item_dir("/app/Library/items", ID),
            format!("/app/Library/items/{ID}")
        );
        // A trailing separator on the items root is trimmed before the join.
        assert_eq!(
            item_dir("/app/Library/items/", ID),
            format!("/app/Library/items/{ID}")
        );
    }

    #[test]
    fn a_stored_book_s_bytes_are_source_under_its_extension() {
        assert_eq!(
            source_path("/app/Library/items", ID, "pdf"),
            format!("/app/Library/items/{ID}/source.pdf")
        );
        assert_eq!(
            source_path("/app/Library/items", ID, "md"),
            format!("/app/Library/items/{ID}/source.md")
        );
        // An extension no format produces still yields a usable, suffix-less
        // name rather than a trailing dot.
        assert_eq!(
            source_path("/app/Library/items", ID, ""),
            format!("/app/Library/items/{ID}/source")
        );
    }

    #[test]
    fn the_cover_marks_and_meta_live_beside_the_source() {
        let dir = format!("/app/Library/items/{ID}");
        assert_eq!(cover_path("/app/Library/items", ID), format!("{dir}/{COVER_FILE}"));
        assert_eq!(marks_path("/app/Library/items", ID), format!("{dir}/{MARKS_FILE}"));
        assert_eq!(meta_path("/app/Library/items", ID), format!("{dir}/{META_FILE}"));
        // Every file a book owns is inside the one folder its id names, which
        // is the whole of the colocation: a merge or a delete is one directory.
        for path in [
            source_path("/app/Library/items", ID, "pdf"),
            cover_path("/app/Library/items", ID),
            marks_path("/app/Library/items", ID),
            meta_path("/app/Library/items", ID),
        ] {
            assert!(path.starts_with(&format!("{dir}/")), "{path} escapes {dir}");
        }
    }

    #[test]
    fn a_linked_and_a_stored_book_share_one_folder_shape() {
        // The point of giving a linked book an item folder too: its cover and
        // marks land in the same place a stored book's do, so "where does this
        // book's stuff live" does not branch on the origin. Only `source.*` is
        // the stored book's alone.
        let stored = item_dir("/app/Library/items", ID);
        assert_eq!(cover_path("/app/Library/items", ID), format!("{stored}/{COVER_FILE}"));
        assert_eq!(marks_path("/app/Library/items", ID), format!("{stored}/{MARKS_FILE}"));
    }

    #[test]
    fn an_id_cannot_escape_its_folder() {
        // Defence in depth: the crate mints an id as an alphanumeric token, but
        // a hand-edited blob must not turn a folder name into a traversal. The
        // separators become `_` and the leading dots are trimmed, so what is left
        // is one component that cannot climb — the dots inside it are inert.
        let root = "/app/Library/items";
        assert_eq!(item_dir(root, "../../etc"), format!("{root}/_.._etc"));
        assert_eq!(item_dir(root, "a/b\\c:d"), format!("{root}/a_b_c_d"));
        assert_eq!(item_dir(root, "../../x"), format!("{root}/_.._x"));
        // Nothing left after the trim is a named folder, not an empty component
        // that would collapse onto the items root itself.
        assert_eq!(item_dir(root, "..."), format!("{root}/item"));
        assert_eq!(item_dir(root, ""), format!("{root}/item"));
        assert_eq!(item_dir(root, "  "), format!("{root}/item"));
    }

    #[test]
    fn a_control_character_never_reaches_a_folder_name() {
        assert_eq!(
            item_dir("/r", "a\u{0}b\u{1f}c"),
            "/r/a_b_c"
        );
    }

    #[test]
    fn a_long_id_is_capped_not_truncated_into_nothing() {
        let long = "a".repeat(400);
        assert_eq!(component(&long).chars().count(), 64);
        assert_eq!(item_dir("/r", &long), format!("/r/{}", "a".repeat(64)));
    }

    #[test]
    fn an_extension_is_sanitised_like_any_other_component() {
        // The extension comes off a filename on disk, so it gets the same
        // treatment: a separator in it cannot climb out of the item folder.
        assert_eq!(
            source_path("/r", ID, "pdf/../../x"),
            format!("/r/{ID}/source.pdf_.._.._x")
        );
    }

    // -------------------------------------------------------------------
    // The old flat layout, recognised so a migration can move out of it.
    // -------------------------------------------------------------------

    #[test]
    fn a_migrated_copy_is_named_after_its_pipeline_not_its_source() {
        // The old name kept whatever the source file was called; the new one is
        // the format's own directory name, so every alias of a format agrees.
        assert_eq!(migrated_ext("pdf"), Some("pdf"));
        assert_eq!(migrated_ext("TXT"), Some("text"));
        assert_eq!(migrated_ext("markdown"), Some("markdown"));
        assert_eq!(migrated_ext("mdown"), Some("markdown"));
        // A format the registry does not know keeps its old name rather than
        // being renamed into something no reader can open.
        assert_eq!(migrated_ext("epub"), None);
        assert_eq!(migrated_ext(""), None);
    }

    #[test]
    fn the_id_an_old_copy_was_named_with_is_the_token_after_its_last_underscore() {
        assert_eq!(flat_store_id("dune_ab12cd.pdf").as_deref(), Some("ab12cd"));
        // An id is hex with a letter prefix, so it never carries an underscore:
        // a source filename full of them still seams at the last one.
        assert_eq!(
            flat_store_id("my_big_book_b018c4f9e2a0.pdf").as_deref(),
            Some("b018c4f9e2a0")
        );
        // A name with no seam is a file this app did not write, and a migration
        // leaves it where it is rather than guessing at an id.
        assert_eq!(flat_store_id("dune.pdf"), None);
        assert_eq!(flat_store_id("dune_.pdf"), None, "an empty id is no id");
        assert_eq!(flat_store_id(""), None);
        // A name with no extension still seams.
        assert_eq!(flat_store_id("dune_ab12").as_deref(), Some("ab12"));
    }

    #[test]
    fn only_the_three_format_buckets_are_the_old_layout() {
        let root = "/app/Library";
        assert!(is_flat_store_path(root, "/app/Library/pdf/dune_ab12.pdf"));
        assert!(is_flat_store_path(root, "/app/Library/text/notes_ab12.txt"));
        assert!(is_flat_store_path(root, "/app/Library/markdown/a_ab12.md"));
        // Already migrated: the item layout puts a folder between bucket and file.
        assert!(!is_flat_store_path(root, &source_path("/app/Library/items", ID, "pdf")));
        assert!(!is_flat_store_path(root, "/app/Library/items/x/y.pdf"));
        // `other` never held a copy — the registry refuses what it does not know.
        assert!(!is_flat_store_path(root, "/app/Library/other/a_ab12.epub"));
        // Not in the store at all: a linked book's own file, and a path that
        // merely starts with the root's characters.
        assert!(!is_flat_store_path(root, "/books/dune.pdf"));
        assert!(!is_flat_store_path(root, "/app/Library-old/pdf/dune_ab12.pdf"));
        // A trailing separator on the root is the same directory.
        assert!(is_flat_store_path("/app/Library/", "/app/Library/pdf/dune_ab12.pdf"));
        // Deeper than one bucket is not a copy this app wrote.
        assert!(!is_flat_store_path(root, "/app/Library/pdf/2024/dune_ab12.pdf"));
    }

    #[test]
    fn a_migrated_address_is_the_item_path_under_its_new_name() {
        // The whole migration in one assertion: recognise the old bucket, read
        // the id out of the old name, and ask this module where that book's copy
        // belongs now.
        let root = "/app/Library";
        let old = format!("/app/Library/pdf/my_big_book_{ID}.pdf");
        assert!(is_flat_store_path(root, &old));
        let id = flat_store_id(&format!("my_big_book_{ID}.pdf")).expect("a seam");
        assert_eq!(id, ID, "the seam is the id the copy was named with");
        let ext = migrated_ext("pdf").expect("a known format");
        assert_eq!(
            source_path(&items_root(root), &id, ext),
            format!("/app/Library/items/{ID}/source.pdf")
        );
    }
}
