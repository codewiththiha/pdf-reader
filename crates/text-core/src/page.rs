//! The text page's geometry.
//!
//! Text documents are cut into fixed-size pages — A4 at 96dpi — so the
//! paginated modes (single, spread, horizontal strip) can reuse the
//! reader's whole page machinery: fit, zoom, navigation, progress. The
//! page size is the one fixed point; everything inside it (type, margins)
//! is the typography settings' job.
//!
//! Book layout swaps the symmetric margins for a gutter: the facing edge
//! carries extra air, and which side that is alternates with the page's
//! parity, exactly like a bound book.

/// Page width in CSS px at scale 1: A4 at 96dpi.
pub const PAGE_WIDTH: f64 = 794.0;
/// Page height in CSS px at scale 1: A4 at 96dpi.
pub const PAGE_HEIGHT: f64 = 1123.0;

/// Symmetric margin around the text area, in CSS px at scale 1.
const PAD: f64 = 72.0;
/// Book layout: the spine-side margin.
const GUTTER: f64 = 92.0;
/// Book layout: the outer margin.
const EDGE: f64 = 56.0;

/// One page's geometry at scale 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub width: f64,
    pub height: f64,
    /// Block (top/bottom) padding.
    pub pad_block: f64,
    /// Inline padding of a LEFT page (or a symmetric page): the near side.
    pub pad_inline_left: f64,
    /// Inline padding of a LEFT page (or a symmetric page): the far side.
    pub pad_inline_right: f64,
    /// Width the text actually flows in.
    pub content_width: f64,
    /// Height the paginator packs blocks into.
    pub content_height: f64,
}

impl PageGeometry {
    /// The inline paddings of page `page` (0-based). With a book layout the
    /// gutter faces the spine: page 0 is a recto (gutter on the LEFT), and
    /// it alternates from there. Without one, both sides are symmetric.
    pub fn inline_pads(&self, book_layout: bool, page: usize) -> (f64, f64) {
        if !book_layout {
            return (self.pad_inline_left, self.pad_inline_right);
        }
        if page.is_multiple_of(2) {
            (GUTTER, EDGE)
        } else {
            (EDGE, GUTTER)
        }
    }

    /// The inline paddings of a page FIXED to one side of a spread: a
    /// left-hand page reads as a verso (gutter on the RIGHT), a right-hand
    /// page as a recto (gutter on the LEFT) — the spine sits between the
    /// two hosts, so the page's own parity is irrelevant there. Without a
    /// book layout both sides stay symmetric.
    pub fn spread_pads(&self, book_layout: bool, right_page: bool) -> (f64, f64) {
        if !book_layout {
            return (self.pad_inline_left, self.pad_inline_right);
        }
        if right_page {
            (GUTTER, EDGE)
        } else {
            (EDGE, GUTTER)
        }
    }
}

impl Default for PageGeometry {
    /// The symmetric (non-book) geometry — what a fresh reader starts from.
    fn default() -> Self {
        geometry(false)
    }
}

/// The geometry for a layout choice.
pub fn geometry(book_layout: bool) -> PageGeometry {
    let (left, right) = if book_layout { (GUTTER, EDGE) } else { (PAD, PAD) };
    PageGeometry {
        width: PAGE_WIDTH,
        height: PAGE_HEIGHT,
        pad_block: PAD,
        pad_inline_left: left,
        pad_inline_right: right,
        content_width: PAGE_WIDTH - left - right,
        content_height: PAGE_HEIGHT - 2.0 * PAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_pages_center_the_column() {
        let g = geometry(false);
        assert_eq!(g.width, PAGE_WIDTH);
        assert_eq!(g.height, PAGE_HEIGHT);
        assert_eq!(g.pad_inline_left, g.pad_inline_right);
        assert!((g.content_width - (PAGE_WIDTH - 2.0 * PAD)).abs() < 1e-9);
        assert!((g.content_height - (PAGE_HEIGHT - 2.0 * PAD)).abs() < 1e-9);
    }

    #[test]
    fn book_layout_alternates_the_gutter() {
        let g = geometry(true);
        // The gutter always faces the spine: left on recto pages, right
        // on verso ones.
        for page in 0..4usize {
            let (l, r) = g.inline_pads(true, page);
            if page % 2 == 0 {
                assert_eq!((l, r), (GUTTER, EDGE));
            } else {
                assert_eq!((l, r), (EDGE, GUTTER));
            }
        }
        // Content width is the same whichever side the gutter sits on.
        assert!((g.content_width - (PAGE_WIDTH - GUTTER - EDGE)).abs() < 1e-9);
        // Without a book layout the pads are the stored symmetric pair.
        assert_eq!(g.inline_pads(false, 1), (g.pad_inline_left, g.pad_inline_right));
    }
}
