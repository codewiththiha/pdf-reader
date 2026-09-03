//! The reflowable formats (TXT, Markdown) rendered as real DOM text.
//!
//! The shape mirrors the PDF side one-for-one — a measure column that finds
//! the truth of block heights, pages that host an A4 box of type, and an
//! axis-generic virtualized strip — with the one structural difference that
//! carries all the others: a text page's size is known (A4), so there is no
//! engine, no raster, and no geometry feedback loop. Zoom re-lays the same
//! text at `base × scale`; pagination never recomputes for it.
//!
//! Every mode renders pages EXCEPT vertical scroll: single and spread show
//! one (or a facing pair), the horizontal strip streams the cut as a
//! virtualized strip of A4 cards, and the vertical mode — the one place
//! reading is not paging — hands the document to the continuous
//! [`stream`](stream), which virtualizes the BLOCKS themselves with no
//! pages at all. The cut still backs the paged modes, the page bookkeeping
//! and the resume flow; it simply is not what scrolls.

pub mod block;
pub mod measure;
pub mod page;
pub mod stream;
pub mod strip;

pub(crate) use measure::TextMeasureColumn;
pub(crate) use page::{SpineSide, TextPage, TypographySignal};
pub(crate) use stream::TextStreamLayout;
pub(crate) use strip::TextPageStrip;
