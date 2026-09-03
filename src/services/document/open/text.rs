//! Opening a reflowable document (plain text, Markdown).
//!
//! The shape mirrors the PDF open — claim the session, read, seed, flip the
//! status — but the content never touches the pdf.js engine. The file is
//! read through the shell's `read_file_text` command, parsed into blocks,
//! and seeded into the SAME page machinery the PDF uses: text documents are
//! cut into A4 pages, so fit, zoom, navigation and the virtualized strips
//! all serve them unchanged. The one difference is the page content — real
//! text in the DOM instead of a rasterised canvas (see `components::text`).
//!
//! Pagination starts from the pure estimate (character counts against the
//! column width) so the reader is up the instant the file is read; the
//! measure column then replaces the estimate with the DOM's real heights
//! and re-cuts once (see `components::text::measure`).

use std::sync::Arc;

use leptos::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;

use pdf_core::documents::Format;
use pdf_core::filename::display_name;
use pdf_engine::types::{DocStatus, PageSize};
use text_core::blocks::{markdown_title, parse_markdown, parse_text};
use text_core::page::{geometry, PAGE_HEIGHT, PAGE_WIDTH};
use text_core::pager::estimate_heights;

use crate::state::reader::text::{estimate_metrics, TextDocument};
use crate::state::AppState;
use crate::viewer::zoom::target::FitDims;

use super::session;

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

/// Shared open-flow for the reflowable formats: read the file, cut it into
/// blocks, and populate the whole app state. Mirrors [`super::open_path`]'s
/// PDF tail, session stamp and all.
pub(super) fn open_text(state: AppState, path: String, format: Format, saved_page: u32, stamp: u64) {
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
        let title = if format == Format::Markdown {
            markdown_title(&raw)
        } else {
            None
        };
        let blocks = match format {
            Format::Markdown => parse_markdown(&raw),
            _ => parse_text(&raw),
        };
        if blocks.is_empty() {
            super::fail(state, "This file has no readable text.".to_string());
            return;
        }
        // Everything below is synchronous, and the session was just checked,
        // so no tail of this flow can outlive its stamp.
        ready_text(state, path, format, title, blocks, saved_page);
    });
}

/// The document read and parsed: seed the state, flip the route, and let
/// the measure column refine the cut.
fn ready_text(
    state: AppState,
    path: String,
    format: Format,
    title: Option<String>,
    blocks: Vec<text_core::blocks::TextBlock>,
    saved_page: u32,
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
    let name = display_name(title.as_deref(), Some(&path));
    state.reader.document.format.set(format);
    state.reader.document.path.set(Some(path.clone()));
    state.reader.document.title.set(title.clone());
    state.reader.document.author.set(None);
    state.reader.document.outline.set(Arc::new(Vec::new()));
    state.reader.document.outline_pending.set(false);
    state
        .reader
        .document
        .page1_size
        .set(Some(PageSize { width: PAGE_WIDTH, height: PAGE_HEIGHT }));

    // The text model: blocks in, estimate cut out. `apply_heights` carries
    // the page count and per-page sizes across to the shared machinery.
    state.reader.text.reset();
    state.reader.gloss.reset();
    let metrics = estimate_metrics(&settings.text, &geo);
    let heights = estimate_heights(&blocks, &metrics);
    state
        .reader
        .text
        .doc
        .set(Some(Arc::new(TextDocument {
            blocks: Arc::new(blocks),
        })));

    // The seed scale, resolved exactly the way the first live refit will
    // (a text page is always A4, so the fit inputs are known up front).
    let startup_fit = settings.layout.default_fit;
    let scale = FitDims::from_geometry(
        state.reader.viewer.mode.get_untracked(),
        state.reader.viewer.container_size.get_untracked(),
        state.reader.viewer.page_margin.get_untracked(),
        (PAGE_WIDTH, PAGE_HEIGHT),
    )
    .map_or(1.0, |dims| dims.fit(startup_fit, 1.0));

    // Reading position + zoom, seeded in the same order the PDF seed uses:
    // anchor guard up BEFORE the page is written, zoom initialised BEFORE
    // the heights are published at that scale.
    state.reader.viewer.awaiting_anchor.set(true);
    state.reader.viewer.fit.set(startup_fit);
    state.reader.viewer.zoom.initialize(scale);
    state.reader.viewer.scroll_top.set(0.0);

    state.reader.text.apply_heights(state, heights, geo);
    let n = state.reader.document.num_pages.get_untracked();
    state
        .reader
        .viewer
        .page
        .set(saved_page.clamp(1, n.max(1)));

    // Ready: flip the route LAST, after every signal the fresh mount reads
    // is seeded. A successful open dismisses any stale error toast.
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    state.ui.toast.set(None);
    state.reader.search.reset();

    // The shelf record is the last step, exactly as it is for a PDF.
    super::shelf::record(state, &path, name, saved_page.clamp(1, n.max(1)), n);
}
