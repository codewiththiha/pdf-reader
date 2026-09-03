//! Pure text-document domain logic: no wasm, no DOM, no leptos.
//!
//! The reflowable formats (plain text and Markdown) share one pipeline:
//! a raw file is cut into [`blocks`](blocks) (paragraphs and top-level
//! Markdown constructs), the blocks are packed into fixed-size
//! [`pages`](pager) — the page cutter — and typography
//! ([`typography`]) turns the reader's settings into the CSS that paints
//! them. [`search`] is the in-document substring index.
//!
//! Everything here is unit-testable on the host via
//! `cargo test -p text-core`.

#![forbid(unsafe_code)]

pub mod blocks;
pub mod page;
pub mod pager;
pub mod search;
pub mod typography;

pub use blocks::{parse_markdown, parse_text, BlockKind, TextBlock};
pub use page::{geometry, PageGeometry, PAGE_HEIGHT, PAGE_WIDTH};
pub use pager::{
    block_page_index, estimate_block_height, estimate_heights, first_block_of_page, paginate,
    BlockMetrics, PageCut,
};
pub use search::{find_matches, TextHit};
pub use typography::{sanitize, TextSettings};
