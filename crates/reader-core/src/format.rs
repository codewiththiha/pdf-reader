//! What the reader can open — the one registry every entry point consults.
//!
//! The open dialog's filter, the drop target's "is this worth showing feedback
//! for" and the OS handoff all answer the same question, so the answer lives
//! here once. Adding a format is adding a row to [`SUPPORTED`].

/// One openable document kind: its file extensions (lower-case, no dot) and
/// the MIME types a drag may advertise it under before its name is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentKind {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub mimes: &'static [&'static str],
}

/// Every kind the reader opens.
pub const SUPPORTED: &[DocumentKind] = &[
    DocumentKind {
        name: "PDF",
        extensions: &["pdf"],
        mimes: &["application/pdf", "application/x-pdf"],
    },
    DocumentKind {
        name: "Text",
        extensions: &["txt", "text"],
        mimes: &["text/plain"],
    },
    DocumentKind {
        name: "Markdown",
        extensions: &["md", "markdown", "mdown"],
        mimes: &["text/markdown", "text/x-markdown"],
    },
];

/// The pipeline a path opens through.
///
/// PDF renders through the pdf.js engine; the two reflowable formats share
/// `reflow-core`'s block, pagination and typography maths and differ only in
/// how their source becomes blocks (`txt-core` and `md-core`). The page,
/// zoom and navigation machinery above that is the same for all three, which
/// is why this enum names pipelines rather than file types: adding a format is
/// adding a row to [`SUPPORTED`] and a handler, not a branch in the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Pdf,
    Text,
    Markdown,
}

impl Format {
    /// True for the reflowable formats — the ones with typography settings,
    /// measurement-driven pagination and no pdf.js involvement.
    ///
    /// Named for what they share rather than for not being a PDF, because
    /// that is the answer the callers need: a reflowable document has a column
    /// that re-measures, a stream that is not pages, and a raster it never
    /// paints. A format that is neither (an epub) must not inherit "text".
    pub fn is_reflowable(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Text => "Text",
            Format::Markdown => "Markdown",
        }
    }
}

/// The format of a path, by extension. Callers check
/// [`is_supported_path`] first; anything that reaches here resolves, and a
/// name the registry does not know answers PDF (the historical default —
/// and the format an extension-less handoff can only be).
pub fn format_of(path: &str) -> Format {
    let name = path.trim_end().rsplit(['/', '\\']).next().unwrap_or("");
    let Some((_, ext)) = name.rsplit_once('.') else {
        return Format::Pdf;
    };
    let ext = ext.to_ascii_lowercase();
    for kind in SUPPORTED {
        if kind.extensions.contains(&ext.as_str()) {
            return match kind.name {
                "Text" => Format::Text,
                "Markdown" => Format::Markdown,
                _ => Format::Pdf,
            };
        }
    }
    Format::Pdf
}

/// Every supported extension, flattened (for dialog filters).
pub fn extensions() -> impl Iterator<Item = &'static str> {
    SUPPORTED.iter().flat_map(|kind| kind.extensions.iter().copied())
}

/// Whether `path` names a file the reader can open, by extension. Case- and
/// trailing-whitespace-insensitive; a name without an extension is not.
pub fn is_supported_path(path: &str) -> bool {
    let name = path.trim_end().rsplit(['/', '\\']).next().unwrap_or("");
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    extensions().any(|known| known == ext)
}

/// Whether a drag advertising `mime` may be carrying a supported document.
/// An EMPTY type is accepted: browsers omit it for files whose kind they do
/// not know at drag time, and the drop itself is still checked by path.
pub fn is_supported_mime(mime: &str) -> bool {
    mime.is_empty()
        || SUPPORTED
            .iter()
            .any(|kind| kind.mimes.iter().any(|m| m.eq_ignore_ascii_case(mime)))
}

/// The first openable path among `paths`, if any.
pub fn first_supported<'a, I: IntoIterator<Item = &'a str>>(paths: I) -> Option<&'a str> {
    paths.into_iter().find(|path| is_supported_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_matched_by_extension_only() {
        assert!(is_supported_path("/Users/me/Books/Dune.pdf"));
        assert!(is_supported_path("C:\\books\\DUNE.PDF"));
        assert!(is_supported_path("weird.name.with.dots.Pdf "));
        assert!(is_supported_path("/Users/me/notes.txt"));
        assert!(is_supported_path("/Users/me/notes.MD"));
        assert!(is_supported_path("C:\\docs\\README.markdown"));
        assert!(!is_supported_path("/Users/me/Books/Dune.epub"));
        assert!(!is_supported_path("/Users/me/pdf"));
        assert!(!is_supported_path("notes.pdf.epub"));
        assert!(!is_supported_path(""));
    }

    #[test]
    fn mimes_accept_the_known_kinds_and_the_unknown_blank() {
        assert!(is_supported_mime("application/pdf"));
        assert!(is_supported_mime("Application/PDF"));
        assert!(is_supported_mime("text/plain"));
        assert!(is_supported_mime("text/markdown"));
        assert!(is_supported_mime(""));
        assert!(!is_supported_mime("image/png"));
        assert!(!is_supported_mime("application/epub+zip"));
    }

    #[test]
    fn the_first_openable_path_wins_over_earlier_junk() {
        let paths = ["cover.png", "book.pdf", "other.pdf"];
        assert_eq!(first_supported(paths), Some("book.pdf"));
        assert_eq!(first_supported(["a.png", "b.epub"]), None);
        // A text document is a first-class citizen now too.
        assert_eq!(first_supported(["a.png", "notes.txt"]), Some("notes.txt"));
    }

    #[test]
    fn the_format_follows_the_extension() {
        assert_eq!(format_of("/books/dune.pdf"), Format::Pdf);
        assert_eq!(format_of("/books/notes.TXT"), Format::Text);
        assert_eq!(format_of("/books/notes.text"), Format::Text);
        assert_eq!(format_of("/books/README.md"), Format::Markdown);
        assert_eq!(format_of("/books/notes.mdown"), Format::Markdown);
        // Unknown or missing extensions fall back to the default pipeline.
        assert_eq!(format_of("/books/notes.epub"), Format::Pdf);
        assert_eq!(format_of("/books/Makefile"), Format::Pdf);
        assert_eq!(format_of("C:\\books\\chapter.MARKDOWN"), Format::Markdown);
    }
}
