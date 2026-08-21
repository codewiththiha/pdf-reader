//! Components whose purpose is displaying/reading PDF documents: the page
//! canvas, the single/continuous layouts, page navigation, floating search,
//! the outline and thumbnails panels, and the DOM plumbing behind them.
//! Pure PDF math stays in `pdf-core`; the pdf.js bridge in `pdf-engine`;
//! generic virtualization math in `virtual-list`.

pub mod continuous;
pub mod dom;
pub mod outline;
pub mod page_canvas;
pub mod page_list;
pub mod single_page;
pub mod thumbnails;

pub(crate) use continuous::ContinuousView;
pub(crate) use outline::OutlinePanel;
pub(crate) use page_canvas::PageCanvas;
pub(crate) use page_list::PageList;
pub(crate) use single_page::SinglePageView;
pub(crate) use thumbnails::ThumbnailsPanel;
