//! What a folder scan found, and whether the folder's own options admit it.
//!
//! The split is deliberate: the shell walks the filesystem and produces
//! [`FoundFile`] rows (it is the only layer that can), and this module decides
//! which of them the folder the user configured actually wants. The decision
//! is pure, so the two things that make it awkward — the include/exclude flip
//! and the size threshold's strictness — are host-testable rather than
//! something you discover by pointing the app at a real folder.

use serde::{Deserialize, Serialize};

use reader_core::format::{Format, SUPPORTED, format_from_ext};

use crate::book::Fingerprint;
use crate::folder::FolderOpts;

/// One file a walk turned up, with the measurements its fingerprint needs.
///
/// Serialized because it crosses the IPC boundary: the shell's blocking walk
/// produces these and the frontend's ledger consumes them, and a 2 000-file
/// folder is a few hundred kilobytes of JSON either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundFile {
    /// The absolute address. What a [`crate::book::Origin::Linked`] book holds.
    pub path: String,
    /// The path relative to the watched root, with `/` separators on every
    /// platform, and empty for a file at the root itself. The subfolder a
    /// shelf is cut from.
    pub rel: String,
    /// Lower-case extension without its dot, empty when the name has none.
    pub ext: String,
    pub size: u64,
    pub fp: Fingerprint,
}

impl FoundFile {
    /// The pipeline this file would open through. `None` for a name the format
    /// registry does not know, which is also what [`admits`] refuses.
    pub fn format(&self) -> Option<Format> {
        format_from_ext(&self.ext)
    }

    /// The subfolder this file sits in, relative to the watched root — `""` at
    /// the root. The key [`crate::folder::WatchedFolder::shelf_key`] hands to
    /// the shelf chain a file is placed on.
    pub fn subfolder(&self) -> &str {
        subfolder_of(&self.rel)
    }
}

/// The subfolder a path relative to a watched root stands in: everything before
/// its last `/`, and the empty string for a file at the root itself.
///
/// Free rather than a method on [`FoundFile`] because a book already in the
/// library has an address and no finding, and the two have to agree about which
/// rung an address belongs to — see
/// [`crate::folder::WatchedFolder::rungs_for`].
pub fn subfolder_of(rel: &str) -> &str {
    match rel.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// Whether a folder's options admit one found file.
///
/// Two knobs, and each has a direction that is easy to get the wrong way
/// round:
///
/// * `include_selected` flips the format set from a whitelist into a
///   blacklist, so "everything but PDF" is expressible without a second list;
/// * `min_size` is a STRICT lower bound — "larger than 30 KB" rejects a file
///   of exactly 30 KB, which is what the import sheet's wording promises.
///
/// A name the format registry does not know is refused outright rather than
/// admitted as a PDF: [`format_from_ext`] is the registry's own answer, so a
/// fourth kind added to `reader_core::format::SUPPORTED` is admitted by every
/// folder that selects it, with no edit here.
pub fn admits(opts: &FolderOpts, ext: &str, size: u64) -> bool {
    let Some(fmt) = format_from_ext(ext) else {
        return false;
    };
    let selected = opts.formats.contains(&fmt);
    let wanted = if opts.include_selected { selected } else { !selected };
    wanted && size > opts.min_size
}

/// The formats a folder may select, straight out of the registry and in its
/// order. The import sheet's checkboxes are built from this, so they are the
/// same rows the open dialog's filter and the drag-drop admission already
/// read: a fourth format appears in all of them at once, with no list here to
/// forget.
///
/// The registry's own column rather than a walk of its extensions deduped back
/// into pipelines: one row is one format, and recovering that from an extension
/// list is a second answer to a question the table already settled.
pub fn selectable_formats() -> Vec<Format> {
    SUPPORTED.iter().map(|kind| kind.format).collect()
}

/// The store sub-directory a format's copies go into: the pipeline's own name
/// for its directory, so the directories read `pdf`, `text` and `markdown` and a
/// fourth kind arrives with its own. `"other"` is only reachable for an
/// extension the registry refuses — which [`admits`] has already turned away, so
/// it is a belt-and-braces answer rather than a case the store can land in.
///
/// Here rather than in the shell because it is a question about the format
/// registry, and the shell does not name `reader-core` directly: the registry is
/// the frontend's, and `tools/check-formats.ts` keeps the shell's own copy of
/// the extension list honest.
pub fn store_dir(ext: &str) -> &'static str {
    format_from_ext(ext)
        .map_or("other", |fmt| fmt.store_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn opts(formats: &[Format], include: bool, min: u64) -> FolderOpts {
        FolderOpts {
            formats: formats.iter().copied().collect::<BTreeSet<_>>(),
            include_selected: include,
            min_size: min,
            ..FolderOpts::default()
        }
    }

    fn found(path: &str, rel: &str, size: u64) -> FoundFile {
        FoundFile {
            path: path.to_string(),
            rel: rel.to_string(),
            ext: path.rsplit('.').next().unwrap_or("").to_string(),
            size,
            fp: Fingerprint {
                size,
                mtime_ms: 1,
                head_hash: 1,
            },
        }
    }

    #[test]
    fn a_selected_format_is_admitted_and_an_unselected_one_is_not() {
        let o = opts(&[Format::Pdf], true, 0);
        assert!(admits(&o, "pdf", 1));
        assert!(!admits(&o, "md", 1));
        assert!(!admits(&o, "txt", 1));
    }

    #[test]
    fn excluding_the_selection_admits_everything_else() {
        let o = opts(&[Format::Pdf], false, 0);
        assert!(!admits(&o, "pdf", 1));
        assert!(admits(&o, "md", 1));
        assert!(admits(&o, "txt", 1));
        // The flip is about the registry, not about a fourth kind sneaking in.
        assert!(!admits(&o, "epub", 1));
    }

    #[test]
    fn the_size_threshold_is_strict() {
        let o = opts(&[Format::Pdf], true, 30 * 1024);
        assert!(!admits(&o, "pdf", 30 * 1024), "exactly 30 KB is not larger than 30 KB");
        assert!(admits(&o, "pdf", 30 * 1024 + 1));
        assert!(!admits(&o, "pdf", 1));
        // Zero admits every byte the format set does — but "larger than zero"
        // still refuses an empty file, because the comparison is strict at every
        // threshold including this one.
        assert!(admits(&opts(&[Format::Pdf], true, 0), "pdf", 1));
        assert!(!admits(&opts(&[Format::Pdf], true, 0), "pdf", 0));
    }

    #[test]
    fn an_unknown_extension_is_refused_whatever_the_options() {
        for include in [true, false] {
            let o = opts(&[Format::Pdf, Format::Text, Format::Markdown], include, 0);
            assert!(!admits(&o, "epub", 1 << 20));
            assert!(!admits(&o, "", 1 << 20));
            assert!(!admits(&o, "png", 1 << 20));
        }
    }

    #[test]
    fn extensions_are_matched_case_blindly_and_with_their_dot() {
        let o = opts(&[Format::Markdown], true, 0);
        assert!(admits(&o, "MD", 1));
        assert!(admits(&o, ".md", 1));
        assert!(admits(&o, "markdown", 1));
        assert!(admits(&o, "mdown", 1));
    }

    #[test]
    fn the_store_directories_are_the_pipelines_own() {
        assert_eq!(store_dir("pdf"), "pdf");
        assert_eq!(store_dir("TXT"), "text");
        assert_eq!(store_dir("markdown"), "markdown");
        assert_eq!(store_dir("mdown"), "markdown");
        assert_eq!(store_dir("epub"), "other", "an unknown kind gets no directory of its own");
    }

    #[test]
    fn the_selectable_set_is_the_registry_and_nothing_more() {
        let formats = selectable_formats();
        assert_eq!(formats.len(), SUPPORTED.len());
        assert_eq!(formats, vec![Format::Pdf, Format::Text, Format::Markdown]);
    }

    #[test]
    fn the_subfolder_is_the_relative_path_minus_its_name() {
        assert_eq!(found("/r/a.pdf", "a.pdf", 1).subfolder(), "");
        assert_eq!(found("/r/x/a.pdf", "x/a.pdf", 1).subfolder(), "x");
        assert_eq!(found("/r/x/y/a.pdf", "x/y/a.pdf", 1).subfolder(), "x/y");
        // A Windows walk normalises its separators to `/` before it gets
        // here, so a `rel` that still carries one is a single segment.
        assert_eq!(found("C:\\r\\x\\a.pdf", "x\\a.pdf", 1).subfolder(), "");
    }

    #[test]
    fn a_found_file_crosses_the_wire_with_its_fingerprint() {
        let f = found("/r/a.pdf", "a.pdf", 1234);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"mtimeMs\""), "{json}");
        assert!(json.contains("\"headHash\""), "{json}");
        let back: FoundFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }
}
