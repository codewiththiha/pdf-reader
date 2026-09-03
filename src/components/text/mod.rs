//! The reflowable formats (TXT, Markdown) rendered as real DOM text.
//!
//! The shape mirrors the PDF side one-for-one — a measure column that finds
//! the truth of block heights, pages that host an A4 box of type, and an
//! axis-generic virtualized strip — with the one structural difference that
//! carries all the others: a text page's size is known (A4), so there is no
//! engine, no raster, and no geometry feedback loop. Zoom re-lays the same
//! text at `base × scale`; pagination never recomputes for it.
//!
//! Every mode renders pages: single and spread show one (or a facing pair),
//! the two scroll modes stream the cut as virtualized strips. The vertical
//! strip styles itself as one continuous column (gap-less, no page boxes),
//! but its virtual items ARE the page cut — which is what lets the shared
//! zoom engine, search reveal, navigation sync and auto-scroll drive text
//! documents without a second code path.

pub mod block;
pub mod measure;
pub mod page;
pub mod strip;

pub(crate) use measure::TextMeasureColumn;
pub(crate) use page::{SpineSide, TextPage, TypographySignal};
pub(crate) use strip::TextPageStrip;
