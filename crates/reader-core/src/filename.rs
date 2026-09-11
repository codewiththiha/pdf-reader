//! Display-name derivation for the open document. Pure — no wasm deps.
//! A PDF's `/Title` is free-form and frequently garbage (source paths,
//! percent-encoded URLs, placeholders): use it only when it looks like a
//! real title, else fall back to the file name.

const MAX_TITLE_LEN: usize = 200;

/// The `Err` both halves of the file dialog report for "the reader said no".
///
/// Both of them (`crates/pdf-engine/src/api/dialog.rs` and `src/services/library`,
/// the library's own picker) call the same `__TAURI__.dialog.open`, and
/// a caller has to be able to tell a cancel — a silent no-op, an answer — from
/// a failure worth a toast. Two spellings of the word meant a caller could only
/// hard-code one; the predicate is the other half of the same agreement.
pub const CANCELLED: &str = "Open cancelled";

/// Whether a dialog's `Err` is that marker and not a real failure.
pub fn is_cancelled(message: &str) -> bool {
    message == CANCELLED
}

/// The display name for the open document: a trustworthy `/Title`, else the
/// file name derived from `path`, else `None`.
pub fn display_name(title: Option<&str>, path: Option<&str>) -> Option<String> {
    if let Some(t) = title.map(str::trim).filter(|t| is_usable_title(t)) {
        return Some(t.to_string());
    }
    path.and_then(file_stem_from_path).filter(|s| !s.is_empty())
}

/// True when a title is worth showing instead of the file name: short enough,
/// not URL- or path-shaped, not a known placeholder, not a filename wearing a
/// title's clothes, and not all punctuation.
pub fn is_usable_title(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > MAX_TITLE_LEN {
        return false;
    }
    let lower = t.to_lowercase();
    const PLACEHOLDERS: [&str; 5] = ["untitled", "unknown", "document", "no title", "pdf document"];
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return false;
    }
    if t.contains("://") || t.contains('\\') || t.contains('%') {
        return false;
    }
    if looks_like_file_name(t) {
        return false;
    }
    t.chars().any(|c| c.is_alphanumeric())
}

/// The shapes a downloader or a scanner leaves in `/Title`: the name the file
/// arrived with, still wearing its extension ("0321894073.pdf"), a bare
/// ISBN/UPC digit run, or a snake-case mangling with underscores where a
/// person would have typed spaces. In every case the file on disk has usually
/// been renamed to something a human can read since, so the stem of the
/// address is the honest name and the metadata is the stale one.
fn looks_like_file_name(t: &str) -> bool {
    // A title does not carry its own extension.
    if strip_doc_extension(t) != t {
        return true;
    }
    // ISBN-10/13 and UPC digit runs, separators aside. Nine or more, so a
    // real numeric title like "1984" stays a title; the trailing check-digit
    // X of an ISBN-10 counts as a digit.
    let run: String = t.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if run.len() >= 9
        && run
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')
        && run.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    // Snake-case: a title typed by a person has spaces. The exception is a
    // trailing `_N` copy counter — the name file managers give the second of
    // two files that would collide ("dune_1"), and the convention the
    // library's own duplicate naming follows
    // (`library_core::book::duplicate_title`). A name that convention minted
    // has to survive the rule that hunts download debris.
    strip_copy_counter(t).contains('_') && !t.contains(' ')
}

/// Drop a trailing `_N` copy counter ("dune_1" → "dune"), when there is one.
/// One level, on purpose: a counter is appended to a name that had none, so
/// "dune_1_2" is not a counter on "dune_1" but a snake-case name in its own
/// right — and the duplicate namer never mints one (it strips the old counter
/// before appending the next).
fn strip_copy_counter(t: &str) -> &str {
    match t.rsplit_once('_') {
        Some((base, counter))
            if !base.is_empty()
                && !counter.is_empty()
                && counter.chars().all(|c| c.is_ascii_digit()) =>
        {
            base
        }
        _ => t,
    }
}

/// Human-readable file name for `path`: last segment (splitting on both `/`
/// and `\\`), with a document extension removed.
pub fn file_stem_from_path(path: &str) -> Option<String> {
    let p = path.trim().trim_end_matches(['/', '\\']);
    if p.is_empty() {
        return None;
    }
    let last = p.rsplit(['/', '\\']).next().unwrap_or(p);
    let stem = strip_doc_extension(last.trim());
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

/// Extensions this app never admits but a name may still carry: a file the
/// reader was handed before the format gate learned to refuse it, or a title
/// typed by hand. Everything the library can actually hold comes from the
/// registry below.
const OFFICE_AND_PRINT: [&str; 7] = ["doc", "docx", "ps", "dvi", "tex", "ppt", "pptx"];

/// Remove a trailing document extension (case-insensitive).
///
/// Read from the format registry rather than typed out here, so a fourth kind
/// strips its own extension with no edit to this list: a shelf that shows
/// "notes.md" under a Markdown book is a shelf telling the reader the file
/// system's business, and the extension was never part of the name — the card
/// next to it says what KIND of document this is without any of it.
///
/// A title like "Rust 1.75" keeps its ".75": the part after the last dot has to
/// BE a known extension, and "75" is not one. A leading dot is the whole of a
/// hidden file's name (".markdown"), not an extension on an empty stem.
fn strip_doc_extension(s: &str) -> &str {
    let lower = s.to_lowercase();
    let Some(dot) = lower.rfind('.') else {
        return s;
    };
    if dot == 0 {
        return s;
    }
    let ext = &lower[dot + 1..];
    let known = crate::format::extensions().any(|kind| kind == ext)
        || OFFICE_AND_PRINT.contains(&ext);
    if known {
        &s[..dot]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Title-vs-path arbitration: a usable /Title wins, an unusable one falls
    /// back to the file name.
    #[test]
    fn picks_the_best_available_name() {
        assert_eq!(
            display_name(Some("The Rust Programming Language"), Some("/tmp/trpl.pdf")).as_deref(),
            Some("The Rust Programming Language")
        );
        // The canonical offender: Distiller wrote a truncated source path.
        assert_eq!(
            display_name(
                Some("file:///F|/Mis%20docum"),
                Some("/Users/me/Books/Programming Pearls (2nd Edition) - Jon Bentley.pdf"),
            )
            .as_deref(),
            Some("Programming Pearls (2nd Edition) - Jon Bentley")
        );
        // Placeholders and path-shaped titles fall back too.
        assert_eq!(display_name(Some("untitled"), Some("/x/y.pdf")).as_deref(), Some("y"));
        assert_eq!(display_name(None, None), None);
    }

    /// Extracting a display name from a path: separators (both kinds), and
    /// extension stripping.
    #[test]
    fn a_download_name_is_not_a_title() {
        // The /Title a downloader leaves behind — an ISBN wearing its
        // extension, a bare ISBN, a snake-case mangling — loses to the stem of
        // the address, which is the name the file has on disk now.
        assert_eq!(
            display_name(Some("0321894073.pdf"), Some("/d/mathematical-proofs.pdf")).as_deref(),
            Some("mathematical-proofs")
        );
        assert_eq!(
            display_name(Some("032190026X"), Some("/d/graphical-approach.pdf")).as_deref(),
            Some("graphical-approach")
        );
        assert_eq!(
            display_name(
                Some("A_Graphical_Approach_to_Algebra_and_Trigonometry"),
                Some("/d/approach.pdf")
            )
            .as_deref(),
            Some("approach")
        );
        // Real titles stay titles, short numeric ones included, and a dot in
        // the middle of a name is not an extension.
        assert_eq!(display_name(Some("1984"), Some("/d/1984.pdf")).as_deref(), Some("1984"));
        assert_eq!(
            display_name(Some("Mathematical Proofs"), Some("/d/mp.pdf")).as_deref(),
            Some("Mathematical Proofs")
        );
        assert_eq!(
            display_name(Some("Mr. Smith Goes West"), Some("/d/msgw.pdf")).as_deref(),
            Some("Mr. Smith Goes West")
        );
        assert!(!super::looks_like_file_name("Discrete Mathematics"));
        assert!(super::looks_like_file_name("978-0-321-89407-3"));
        // A trailing copy counter is the one snake-case shape that IS a name
        // a person (or the library's duplicate namer) chose.
        assert!(!super::looks_like_file_name("dune_1"));
        assert!(!super::looks_like_file_name("Dune_12"));
        // A counter on a mangled name does not launder the mangling.
        assert!(super::looks_like_file_name("harry_potter_goblet_1"));
    }

    #[test]
    fn file_stem_extraction() {
        for (path, want) in [
            ("/b/Programming Pearls (2nd Edition) - Jon Bentley.pdf",
             Some("Programming Pearls (2nd Edition) - Jon Bentley")),
            (r"C:\Users\me\Docs\Deep Work.pdf", Some("Deep Work")),
            (r"\\server\share\Annual Report.pdf", Some("Annual Report")),
            ("/a/b/", Some("b")),
            ("book.pdf", Some("book")),
            // The formats this app opens, not just the ones pdf.js reads: a
            // Markdown book is named "notes" on the shelf, never "notes.md".
            ("/books/notes.md", Some("notes")),
            ("/books/notes.markdown", Some("notes")),
            ("/books/notes.MDOWN", Some("notes")),
            ("/books/log.txt", Some("log")),
            ("/books/log.text", Some("log")),
            // And the office and print kinds, which the gate refuses but a name
            // may still wear.
            ("/books/slides.pptx", Some("slides")),
            // A version is not an extension, a hidden file's dot is not one
            // either, and an unknown one stays because nothing claims it.
            ("/books/Rust 1.75", Some("Rust 1.75")),
            ("/books/.markdown", Some(".markdown")),
            ("/books/archive.epub", Some("archive.epub")),
            ("/", None),
        ] {
            assert_eq!(file_stem_from_path(path).as_deref(), want, "path {path:?}");
        }
    }
}
