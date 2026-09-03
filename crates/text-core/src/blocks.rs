//! Cutting a raw file into blocks — the unit everything below the parser
//! works in.
//!
//! A block is one layout atom: a paragraph of plain text, or one top-level
//! Markdown construct (heading, paragraph, list, code fence, table, quote,
//! rule). Blocks are what the paginator packs into pages and what the
//! vertical reader streams, so both formats share one shape.
//!
//! The Markdown split is fence-aware but otherwise deliberately simple: it
//! cuts on blank lines outside code fences. That is exactly CommonMark's
//! top-level block boundary for the constructs a reader cares about, and it
//! keeps the parser free of a second Markdown dependency — the RENDER still
//! goes through the real parser, block by block.

/// Which renderer a block belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Plain text: rendered verbatim, hard line breaks preserved.
    Text,
    /// Markdown source: one top-level construct, rendered as Markdown.
    Markdown,
}

/// One layout atom of a text document.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub kind: BlockKind,
    /// The block's source text. Text blocks keep their internal newlines;
    /// Markdown blocks keep exactly the lines of their construct.
    pub text: String,
}

/// Normalise a raw file for parsing: drop the UTF-8 BOM, fold CRLF/CR to
/// LF, and trim trailing whitespace off every line (a trailing double space
/// is a Markdown hard break the renderer still gets from the line's own
/// content — the trim only removes the invisible kind that makes empty
/// lines look non-empty).
pub fn normalize(raw: &str) -> String {
    let stripped = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    // Fold CRLF first, then lone CRs (old macOS): every line ending becomes
    // exactly one LF, so the split below never yields phantom blank lines.
    let folded = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(folded.len());
    for (i, line) in folded.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

/// A plain-text file into blocks: runs of non-empty lines, separated by
/// blank lines. Internal single newlines are KEPT — the renderer preserves
/// them (pre-wrap), which is what makes fixed-line prose and code-ish notes
/// read as authored.
pub fn parse_text(raw: &str) -> Vec<TextBlock> {
    let text = normalize(raw);
    split_blocks(&text, BlockKind::Text)
}

/// A Markdown file into its top-level blocks. Blank lines separate blocks
/// EXCEPT inside a fenced code block, where a blank line is content.
pub fn parse_markdown(raw: &str) -> Vec<TextBlock> {
    let text = normalize(raw);
    split_blocks(&text, BlockKind::Markdown)
}

/// The shared blank-line splitter; Markdown mode keeps fence state so a
/// fenced block survives interior blank lines.
fn split_blocks(text: &str, kind: BlockKind) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut fence_marker = "";

    let flush = |blocks: &mut Vec<TextBlock>, current: &mut Vec<&str>| {
        if current.is_empty() {
            return;
        }
        let joined = current.join("\n");
        current.clear();
        if !joined.trim().is_empty() {
            blocks.push(TextBlock { kind, text: joined });
        }
    };

    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if kind == BlockKind::Markdown && !in_fence && is_fence_open(trimmed) {
            in_fence = true;
            fence_marker = fence_marker_of(trimmed);
            current.push(line);
            continue;
        }
        if kind == BlockKind::Markdown && in_fence {
            current.push(line);
            // A closing fence: the same marker char, nothing on the line
            // but the fence itself (it may be longer than the opener).
            let marker_char = fence_marker.chars().next().unwrap_or('`');
            if trimmed.starts_with(fence_marker)
                && trimmed.trim_end_matches(marker_char).trim().is_empty()
            {
                in_fence = false;
            }
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut blocks, &mut current);
        } else {
            current.push(line);
        }
    }
    flush(&mut blocks, &mut current);
    blocks
}

/// Whether a trimmed line opens a fenced code block (``` or ~~~, optionally
/// followed by an info string).
fn is_fence_open(trimmed: &str) -> bool {
    !fence_marker_of(trimmed).is_empty()
}

/// The fence marker a line opens with (``` or ~~~), or "" for none.
fn fence_marker_of(trimmed: &str) -> &'static str {
    if trimmed.starts_with("```") {
        "```"
    } else if trimmed.starts_with("~~~") {
        "~~~"
    } else {
        ""
    }
}

/// The document's title, when the Markdown opens with an ATX heading:
/// `# Title` (levels 1–3 count; deeper headings are sectioning, not a
/// title). Returns the heading text without markers or emphasis noise.
pub fn markdown_title(raw: &str) -> Option<String> {
    let text = normalize(raw);
    for line in text.split('\n') {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=3).contains(&hashes) {
            let rest = trimmed[hashes..].trim();
            if rest.is_empty() {
                return None;
            }
            return Some(rest.trim_matches('*').trim().to_string());
        }
        // The first non-blank line is not a heading: the document has no
        // title to claim.
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_line_endings_and_strips_the_bom() {
        assert_eq!(normalize("\u{feff}a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(normalize("x  \ny\t"), "x\ny");
    }

    #[test]
    fn text_paragraphs_split_on_blank_lines_only() {
        let blocks = parse_text("line one\nline two\n\nsecond para\n\n\nthird");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "line one\nline two");
        assert_eq!(blocks[1].text, "second para");
        assert_eq!(blocks[2].text, "third");
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Text));
    }

    #[test]
    fn an_empty_or_blank_file_has_no_blocks() {
        assert!(parse_text("").is_empty());
        assert!(parse_text("  \n\n  \n").is_empty());
        assert!(parse_markdown("").is_empty());
    }

    #[test]
    fn markdown_splits_on_blank_lines() {
        let blocks = parse_markdown("# Title\n\nSome prose.\n\n- a\n- b\n");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "# Title");
        assert_eq!(blocks[1].text, "Some prose.");
        assert_eq!(blocks[2].text, "- a\n- b");
    }

    #[test]
    fn a_code_fence_survives_interior_blank_lines() {
        let md = "before\n\n```rust\nfn main() {\n\n    println!(\"hi\");\n}\n```\n\nafter";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert!(blocks[1].text.starts_with("```rust"));
        assert!(blocks[1].text.ends_with("```"));
        assert!(blocks[1].text.contains("\n\n"));
        assert_eq!(blocks[2].text, "after");
    }

    #[test]
    fn an_unclosed_fence_still_yields_one_block() {
        let blocks = parse_markdown("text\n\n```\ncode line\n\nmore code");
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].text.contains("more code"));
    }

    #[test]
    fn markdown_title_takes_the_first_heading() {
        assert_eq!(markdown_title("# Dune\n\nby Frank Herbert"), Some("Dune".into()));
        assert_eq!(markdown_title("## Part One"), Some("Part One".into()));
        assert_eq!(markdown_title("#### too deep"), None);
        assert_eq!(markdown_title("plain prose first\n\n# later"), None);
        assert_eq!(markdown_title("#"), None);
        assert_eq!(markdown_title(""), None);
    }
}
