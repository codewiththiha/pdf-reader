//! Opening a reflowable document — plain text, Markdown, and whatever joins
//! them later.
//!
//! The shape mirrors the PDF open (claim the session, read, seed, flip the
//! status), but the content never touches the pdf.js engine: the file is read
//! through the shell's `read_file_text` command and handed to its format's
//! parser, which returns the blocks the shared machinery lays out. From here on
//! a text document and a PDF are the same object — pages of the same A4 sheet,
//! the same scale pipeline, the same strip, the same virtualizer — and the one
//! difference is what paints inside a page (see `components::formats::reflow`).
//!
//! Two things make this the whole format-specific surface of the open flow:
//!
//! * the PARSER is a match on the format, not a second pipeline. A third
//!   reflowable format adds one arm here (and a block view, and a renderer);
//!   it does not copy this file.
//! * pagination starts from the pure estimate — character counts against the
//!   column width — so the reader is up the instant the file is read. The measure
//!   column then replaces the estimate with the DOM's real heights and re-cuts
//!   once (see `components::formats::reflow::measure`).
//!
//! Markdown also gets an outline, and it is seeded rather than resolved: the
//! headings are already in the text, so this file hands the reader the block
//! indices and `effects::reader::text_outline` turns them into pages against the
//! live cut. There is no `outline::resolve` tail to race, which is why
//! `outline_pending` goes false here.

use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use md_core::MarkdownHeading;
use pdf_engine::types::{DocStatus, PageSize};
use reader_core::filename::display_name;
use reader_core::format::Format;
use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;
use reflow_core::block::TextBlock;
use reflow_core::geometry::{geometry, PAGE_HEIGHT, PAGE_WIDTH};
use reflow_core::pager::estimate_heights;

use crate::state::AppState;
use crate::state::reader::document::reflow::estimate_metrics;
use crate::viewer::zoom::target::FitDims;

use super::session;

/// What a format contributes to an open: its parsed blocks, what to call the
/// document, and the headings its outline will be built from.
struct Parsed {
    blocks: Vec<TextBlock>,
    title: Option<String>,
    /// The author line, when the format has one (front matter).
    author: Option<String>,
    headings: Vec<MarkdownHeading>,
}

/// The document's bytes as text, through the shell's gated read command.
///
/// Outside the desktop shell there is no filesystem to read from — the
/// plain-browser build answers with the same "desktop only" error the open
/// dialog gives, rather than a platform failure.
async fn read_file_text(path: &str) -> Result<String, String> {
    if !tauri_bridge::has_tauri() {
        return Err(
            "Opening files is only available in the desktop app. Drag and drop runs through \
             the shell too."
                .to_string(),
        );
    }
    let args = js_sys::Object::new();
    _ = js_sys::Reflect::set(&args, &"path".into(), &JsValue::from_str(path));
    let value = tauri_bridge::invoke("read_file_text", args.into())
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))?;
    value
        .as_string()
        .ok_or_else(|| "read_file_text returned no text".to_string())
}

/// Shared open flow for the reflowable formats: read the file, parse it with
/// the format's own parser, and populate the whole app state. Mirrors
/// [`super::open_pdf`]'s tail, session stamp and all.
pub(super) fn open_reflowable(
    state: AppState,
    path: String,
    format: Format,
    saved_page: u32,
    saved_fraction: Option<f64>,
    stamp: u64,
) {
    spawn_local(async move {
        let raw = match read_file_text(&path).await {
            Ok(raw) => raw,
            Err(message) => {
                if session::owns(stamp) {
                    super::fail(state, message);
                }
                return;
            }
        };
        // The read finished — but a second open (or a close) may have taken
        // the document state over while it was working.
        if !session::owns(stamp) {
            return;
        }
        let parsed = parse(format, &raw);
        if parsed.blocks.is_empty() {
            super::fail(state, "This file has no readable text.".to_string());
            return;
        }
        // Everything below is synchronous, and the session was just checked,
        // so no tail of this flow can outlive its stamp.
        ready(state, path, format, parsed, saved_page, saved_fraction);
    });
}

/// The format's own step, and nothing else: normalise, parse, then cut oversized
/// blocks so a page can be packed tightly.
///
/// The subdivision runs BEFORE anything downstream sees the blocks, so block
/// identities are stable for the whole session and every consumer (pages,
/// stream, search, outline) works in the same atoms. Plain text cuts at 40 lines
/// — its hard breaks are natural boundaries and a fixed-line paragraph (ASCII
/// tables, poetry) must not be chopped; Markdown cuts prose only, at
/// [`reflow_core::block::SPLIT_MAX_LINES`], because a split inside a list, a
/// fence, a table or a quote re-opens that construct mid-page.
fn parse(format: Format, raw: &str) -> Parsed {
    match format {
        Format::Markdown => {
            let blocks = md_core::subdivide_prose(md_core::parse_markdown(raw));
            // Keyed on the SUBDIVIDED blocks: a split shifts every index after
            // it, and an outline entry that points at the wrong block is worse
            // than no outline at all.
            let headings = md_core::headings_of_blocks(&blocks);
            Parsed {
                blocks,
                title: md_core::document_title(raw),
                author: md_core::document_author(raw),
                headings,
            }
        }
        // Anything else reflowable is read as text, the same answer
        // `format_of` gave at the door.
        _ => Parsed {
            blocks: txt_core::subdivide_paragraphs(txt_core::parse_plain_text(raw)),
            title: None,
            author: None,
            headings: Vec::new(),
        },
    }
}

/// The document read and parsed: seed the state, flip the route, and let the
/// measure column refine the cut.
fn ready(
    state: AppState,
    path: String,
    format: Format,
    parsed: Parsed,
    saved_page: u32,
    saved_fraction: Option<f64>,
) {
    let settings = state.settings.get_untracked();
    let geo = geometry(settings.text.book_layout);

    // A text document opening over a PDF: release the engine's book and the
    // paper session that tracked it — neither has any part in what follows.
    spawn_local(async move {
        _ = pdf_engine::api::destroy().await;
    });
    pdf_engine::paper::document_close();

    // Document identity.
    let name = display_name(parsed.title.as_deref(), Some(&path));
    let Parsed { blocks, title, author, headings } = parsed;
    state.reader.document.format.set(format);
    state.reader.document.path.set(Some(path.clone()));
    state.reader.document.title.set(title);
    state.reader.document.author.set(author);
    state.reader.document.outline.set(Arc::new(Vec::new()));
    state.reader.document.outline_pending.set(false);
    state
        .reader
        .document
        .content
        .pdf
        .page1_size
        .set(Some(PageSize { width: PAGE_WIDTH, height: PAGE_HEIGHT }));

    // The reflowable content: blocks in, estimate cut out. `apply_heights`
    // carries the page count and the per-page sizes across to the machinery the
    // PDF shares. The other format's pages are released at the same moment.
    state.reader.document.content.reflow.reset();
    state.reader.gloss.reset();
    // The document's gloss highlights, loaded exactly where the PDF open loads
    // them: before anything mounts, so the first page (or the first stream
    // window) already paints them. A reflowable mark is a block and a
    // character range rather than a rect, so it is the `apply_heights` below —
    // which publishes the block→page map — that makes it projectable; loading
    // first and paginating second is what puts it on the right page at first
    // paint instead of a frame later.
    state.reader.gloss.marks.set(
        crate::storage::load_gloss()
            .remove(&path)
            .unwrap_or_default(),
    );
    let metrics = estimate_metrics(&settings.text, &geo);
    let heights = estimate_heights(&blocks, &metrics);
    state.reader.document.content.reflow.blocks.set(Arc::new(blocks));
    state.reader.document.content.reflow.headings.set(Arc::new(headings));

    // The seed scale, resolved exactly the way the first live refit will
    // (a text page is always A4, so the fit inputs are known up front) —
    // except in the continuous stream, where there is no page to fit: the
    // window is the page, type size belongs to the typography settings,
    // and the zoom starts at 1.
    let streaming = state.reader.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    let startup_fit = if streaming { FitMode::None } else { settings.layout.default_fit };
    let scale = if streaming {
        1.0
    } else {
        FitDims::from_geometry(
            state.reader.viewer.mode.get_untracked(),
            state.reader.viewer.container_size.get_untracked(),
            state.reader.viewer.page_margin.get_untracked(),
            (PAGE_WIDTH, PAGE_HEIGHT),
        )
        .map_or(1.0, |dims| dims.fit(startup_fit, 1.0))
    };

    // Reading position + zoom, seeded in the same order the PDF seed uses:
    // anchor guard up BEFORE the page is written, zoom initialised BEFORE
    // the heights are published at that scale. The stream's fractional
    // resume rides along only when the document opens INTO the stream — a
    // fraction saved by an earlier streamed session means nothing to a paged
    // read, and letting it linger would hijack the anchor when the reader
    // later flips the mode.
    state.reader.document.content.reflow.resume_fraction.set(if streaming { saved_fraction } else { None });
    state.reader.viewer.awaiting_anchor.set(true);
    state.reader.viewer.fit.set(startup_fit);
    state.reader.viewer.zoom.initialize(scale);
    state.reader.viewer.scroll_top.set(0.0);

    state.reader.document.content.reflow.apply_heights(state, heights, geo);
    let n = state.reader.document.num_pages.get_untracked();
    state.reader.viewer.page.set(saved_page.clamp(1, n.max(1)));

    // Ready: flip the route LAST, after every signal the fresh mount reads
    // is seeded. A successful open dismisses any stale error toast.
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    state.ui.toast.set(None);
    state.reader.search.reset();

    // The shelf record is the last step, exactly as it is for a PDF.
    super::shelf::record(state, &path, name, saved_page.clamp(1, n.max(1)), n);
}
