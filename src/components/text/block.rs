//! One block of a text document, rendered.
//!
//! Plain-text blocks render verbatim with their hard line breaks preserved
//! (`pre-wrap`); Markdown blocks go through `leptos-md`, one top-level
//! construct per block, so a heading never shares a layout atom with the
//! paragraph under it — which is what lets the paginator treat each block
//! as one unpackable unit.
//!
//! Typography does not live here on purpose: every style the settings
//! control (font, size, spacing, justification) is inherited from the page
//! host's inline style, so a block renders identically on a page and in the
//! measure column, at any scale, with no props for it.

use leptos::prelude::*;
use leptos_md::{render_markdown_with_options, MarkdownOptions};

use text_core::blocks::{BlockKind, TextBlock};

/// The options every Markdown block renders with.
///
/// GFM is on (tables and task lists are everyday Markdown), links open in a
/// new tab, and raw HTML is REFUSED: these are the reader's own files, but
/// a Markdown document is still untrusted input, and there is nothing a
/// reading view needs from inline `<script>` or `<style>`. Code blocks get
/// no theme classes — the reader's stylesheet owns the look, and keeps it
/// in every tint.
fn markdown_options() -> MarkdownOptions {
    MarkdownOptions::new()
        .with_gfm(true)
        .with_language_classes(true)
        .with_new_tab_links(true)
        .with_allow_raw_html(false)
        .without_code_theme()
}

#[component]
pub fn TextBlockView(block: TextBlock) -> impl IntoView {
    let content: AnyView = match block.kind {
        BlockKind::Text => view! {
            <div class="tx-plain">{block.text}</div>
        }
        .into_any(),
        BlockKind::Markdown => {
            match render_markdown_with_options(&block.text, markdown_options()) {
                // The `tx-md` wrapper is the stylesheet's handle on the
                // rendered construct: one block, one wrapper, so the rules
                // can never leak past their construct.
                Ok(rendered) => view! { <div class="tx-md">{rendered}</div> }.into_any(),
                // A block the renderer refuses still deserves its words:
                // fall back to the source as plain text.
                Err(_) => view! {
                    <div class="tx-plain">{block.text}</div>
                }
                .into_any(),
            }
        }
    };
    // The tail of a split paragraph drops its paragraph space (see
    // `text_core::blocks::subdivide`); the class does exactly that.
    let class = if block.continuation { "tx-block tx-cont" } else { "tx-block" };
    view! {
        <div class=class>{content}</div>
    }
}
