//! Components whose purpose is displaying PDF documents: the page canvas
//! and the single/continuous/dual/horizontal layouts. Pure PDF math stays in
//! `pdf-core`; the pdf.js bridge in `pdf-engine`; generic virtualization math
//! in `virtual-list`. Shared DOM lookups live in
//! [`crate::components::primitives::hooks::dom`].

pub mod continuous_view;
pub mod dual_page_view;
pub mod horizontal_view;
pub mod page_canvas;
pub mod page_list;
pub mod single_page_view;

pub(crate) use continuous_view::ContinuousView;
pub(crate) use dual_page_view::DualPageView;
pub(crate) use horizontal_view::HorizontalView;
pub(crate) use page_canvas::PageCanvas;
pub(crate) use page_list::PageList;
pub(crate) use single_page_view::SinglePageView;
