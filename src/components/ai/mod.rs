//! AI-assisted reading: the floating pill anchored to the reader's text
//! selection and the explanation card it opens.
//!
//!   * `selection_pill` — the pill a selection produces, and its anchor maths.
//!   * `anchor` / `reflow_anchor` — where a selection or a mark is on screen,
//!     per format family.
//!   * `gloss` — the explanation card, its highlight marks and the commands
//!     that drive them.
//!   * `settings` — the AI's appearance knobs, hosted by the settings modal's
//!     Theme tab.

pub mod anchor;
pub mod gloss;
pub mod reflow_anchor;
pub mod selection_pill;
pub mod settings;
