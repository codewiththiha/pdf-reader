//! How the library is looking at the moment: the layout, the cover treatment,
//! the column count and the sort.
//!
//! Deliberately NOT part of `reader_core::settings::Settings`. That blob is the
//! appearance and reader-preference store, and every write to it re-runs the
//! theme projection and re-serialises the whole settings JSON — which is why
//! `src/state/library.rs` keeps the library out of it already. A column-count
//! nudge is a library concern with the same write frequency as a page turn, so
//! it persists with the library, in the library's own key.

use serde::{Deserialize, Serialize};

use crate::sort::SortKey;

/// The narrowest and widest fixed column counts the view menu offers. Between
/// them the grid is legible on a 640 px window and on a maximised display;
/// outside them a card is either a postage stamp or a poster.
pub const COLUMNS_MIN: u8 = 2;
pub const COLUMNS_MAX: u8 = 10;

/// Grid of book covers, or a dense list of rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryLayout {
    /// Covers on a shelf — the layout the library has always had.
    #[default]
    Grid,
    /// One row per book: cover thumbnail, title, author, progress.
    List,
}

/// How a cover fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverFit {
    /// The whole cover, at the document's own aspect ratio (clamped, so a
    /// pathological page cannot break the grid). The look the shelf has today.
    #[default]
    Fit,
    /// Every cover cropped to A4 portrait, so a shelf of mixed scans and
    /// exports reads as one row of identical spines.
    Crop,
}

impl CoverFit {
    /// The label the view menu shows.
    pub fn label(self) -> &'static str {
        match self {
            CoverFit::Fit => "Fit",
            CoverFit::Crop => "Crop",
        }
    }
}

/// The library's view, persisted with the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    #[serde(default)]
    pub layout: LibraryLayout,
    /// Fixed column count, or `None` for Auto (as many as fit). Ignored — and
    /// shown disabled — in [`LibraryLayout::List`], where a row is a row; the
    /// count survives the visit, so switching back to the grid returns the
    /// columns the reader last picked.
    #[serde(default)]
    pub columns: Option<u8>,
    /// The count the auto flow currently produces. The shell writes it on every
    /// resize while `columns` is `None`, so the menu can show Auto's live count
    /// and the stepper's first `+` pins the count the reader is looking at
    /// (5 → 6) rather than stepping from an idea nobody can see. Stale is
    /// harmless: a resize — or a return to Auto — refreshes it before the next
    /// click.
    #[serde(default = "default_auto_fit")]
    pub auto_fit: u8,
    #[serde(default)]
    pub cover: CoverFit,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default = "default_asc")]
    pub sort_asc: bool,
}

fn default_asc() -> bool {
    true
}

/// What `auto_fit` is in a blob written before the grid reported the flow's
/// count: a mid-range guess, so a restored Auto steps inside the range rather
/// than at its edge until the first measurement lands.
fn default_auto_fit() -> u8 {
    5
}

impl Default for LibraryView {
    fn default() -> Self {
        Self {
            layout: LibraryLayout::default(),
            columns: None,
            auto_fit: default_auto_fit(),
            cover: CoverFit::default(),
            sort: SortKey::default(),
            sort_asc: true,
        }
    }
}

impl LibraryView {
    /// The value the grid's `--lib-cols` custom property takes: a count, or
    /// `auto-fill` when the reader left it to the window width. One function
    /// owns the spelling so the CSS and the control cannot drift. `auto_fit`
    /// deliberately never reaches this token: a measurement that moved the
    /// thing it measured would be a layout loop.
    pub fn columns_token(&self) -> String {
        match self.columns {
            Some(n) => n.to_string(),
            None => "auto-fill".to_string(),
        }
    }

    /// Whether the +/− stepper may act. Auto is a real target: the first `+`
    /// pins the count the auto flow is producing right now and steps from
    /// there, so the stepper is live wherever a grid is showing columns and
    /// only the list layout — which has none — kills it.
    pub fn columns_enabled(&self) -> bool {
        !self.is_list()
    }

    /// Move the column count by one press, staying inside
    /// [`COLUMNS_MIN`]..=[`COLUMNS_MAX`]. From Auto the first press pins what
    /// the flow was showing (`auto_fit`) and steps from THAT; in the list it is
    /// a no-op, so a control that renders disabled cannot be driven by a stray
    /// key.
    pub fn step_columns(&mut self, delta: i32) {
        if self.is_list() {
            return;
        }
        let base = u16::from(self.columns.unwrap_or(self.auto_fit));
        let next =
            (i32::from(base) + delta).clamp(i32::from(COLUMNS_MIN), i32::from(COLUMNS_MAX));
        self.columns = Some(next as u8);
    }

    /// Leave Auto: the columns become the count the window fits right now, so
    /// the reader's first press of −/+ has something to step from. `fit` is the
    /// caller's measurement, clamped here rather than trusted. A count the
    /// reader has pinned already is not overridden by a report — it is only
    /// clamped, so a stray call can move the range but never the choice.
    pub fn pin_columns(&mut self, fit: u8) {
        self.auto_fit = fit.clamp(COLUMNS_MIN, COLUMNS_MAX);
        self.columns = Some(
            self.columns
                .map_or(self.auto_fit, |n| n.clamp(COLUMNS_MIN, COLUMNS_MAX)),
        );
    }

    /// Back to Auto. The flow's count keeps being reported into `auto_fit`, so
    /// the menu still shows a live number and the stepper's next press steps
    /// from what the shelf is showing.
    pub fn auto_columns(&mut self) {
        self.columns = None;
    }

    /// Whether a drag may re-order what the reader is looking at. A sorted
    /// shelf re-sorts on the next render, so a drop would be undone before the
    /// reader saw it land.
    pub fn drag_reorders(&self) -> bool {
        self.sort.is_manual()
    }

    /// True when the shelf renders as rows rather than as covers.
    pub fn is_list(&self) -> bool {
        self.layout == LibraryLayout::List
    }
}

/// Make a persisted view internally valid: the pinned count and the reported
/// auto count both inside the range the menu offers. Idempotent. A list keeps
/// the count it cannot show — it returns with the grid.
pub fn sanitize(view: &mut LibraryView) {
    view.columns = view.columns.map(|n| n.clamp(COLUMNS_MIN, COLUMNS_MAX));
    view.auto_fit = view.auto_fit.clamp(COLUMNS_MIN, COLUMNS_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_look_the_library_has_today() {
        let v = LibraryView::default();
        assert_eq!(v.layout, LibraryLayout::Grid);
        assert_eq!(v.columns, None, "Auto until the reader says otherwise");
        assert_eq!(v.auto_fit, 5, "a mid-range guess until the grid reports");
        assert_eq!(v.cover, CoverFit::Fit);
        assert_eq!(v.sort, SortKey::Manual);
        assert!(v.sort_asc);
        assert_eq!(v.columns_token(), "auto-fill");
        assert!(
            v.columns_enabled(),
            "Auto is a count the stepper can step from"
        );
        assert!(v.drag_reorders(), "a manual grid is the one a drag writes to");
    }

    #[test]
    fn a_blob_without_a_view_loads_the_default() {
        let v: LibraryView = serde_json::from_str("{}").unwrap();
        assert_eq!(v, LibraryView::default());
    }

    #[test]
    fn a_blob_from_before_the_grid_reported_loads_a_mid_range_auto_fit() {
        let v: LibraryView = serde_json::from_str(r#"{"columns":null}"#).unwrap();
        assert_eq!(v.auto_fit, 5);
        assert_eq!(v.columns, None);
    }

    #[test]
    fn the_view_persists_under_its_camel_case_names() {
        let v = LibraryView {
            layout: LibraryLayout::List,
            columns: Some(4),
            auto_fit: 7,
            cover: CoverFit::Crop,
            sort: SortKey::LastRead,
            sort_asc: false,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"layout\":\"list\""), "{json}");
        assert!(json.contains("\"autoFit\":7"), "{json}");
        assert!(json.contains("\"cover\":\"crop\""), "{json}");
        assert!(json.contains("\"sort\":\"lastRead\""), "{json}");
        let back: LibraryView = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn pinning_auto_gives_the_stepper_something_to_step_from() {
        let mut v = LibraryView::default();
        v.pin_columns(6);
        assert_eq!(v.columns, Some(6));
        assert_eq!(v.columns_token(), "6");
        assert!(v.columns_enabled());
        v.step_columns(1);
        assert_eq!(v.columns, Some(7));
        v.step_columns(-20);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        v.step_columns(99);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
        v.auto_columns();
        assert_eq!(v.columns, None);
        assert_eq!(v.columns_token(), "auto-fill");
    }

    #[test]
    fn stepping_from_auto_pins_what_auto_was_showing() {
        let mut v = LibraryView::default();
        // The grid has been reporting the flow's count on every resize.
        v.auto_fit = 7;
        assert_eq!(v.columns, None, "Auto still owns the layout");
        v.step_columns(1);
        assert_eq!(
            v.columns,
            Some(8),
            "the first + pins what Auto was showing and steps from there"
        );
        assert_eq!(v.columns_token(), "8");
        v.step_columns(-2);
        assert_eq!(v.columns, Some(6), "and from then on it steps the pin");
        v.step_columns(-99);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        v.step_columns(99);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
    }

    #[test]
    fn the_stepper_is_live_in_grid_and_dead_in_list() {
        let mut v = LibraryView::default();
        assert!(v.columns_enabled(), "live in a grid under Auto…");
        v.columns = Some(4);
        assert!(v.columns_enabled(), "…and with a pinned count");
        v.layout = LibraryLayout::List;
        assert!(!v.columns_enabled(), "dead only where there are no columns");
        v.step_columns(1);
        assert_eq!(v.columns, Some(4), "and a stray key cannot drive it");
        v.layout = LibraryLayout::Grid;
        assert!(
            v.columns_enabled(),
            "the pin survived the list, so the grid returns as it left"
        );
    }

    #[test]
    fn a_pin_outside_the_range_is_clamped_not_trusted() {
        let mut v = LibraryView::default();
        v.pin_columns(0);
        assert_eq!(v.auto_fit, COLUMNS_MIN);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        v.auto_columns();
        v.pin_columns(200);
        assert_eq!(v.auto_fit, COLUMNS_MAX);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
    }

    #[test]
    fn a_report_refreshes_auto_without_overriding_a_pin() {
        let mut v = LibraryView {
            columns: Some(4),
            ..LibraryView::default()
        };
        v.pin_columns(9);
        assert_eq!(v.auto_fit, 9, "the flow's count is recorded…");
        assert_eq!(v.columns, Some(4), "…and the reader's pin is left alone");
    }

    #[test]
    fn the_list_layout_has_no_columns_to_step() {
        let mut v = LibraryView {
            layout: LibraryLayout::List,
            columns: Some(5),
            ..LibraryView::default()
        };
        sanitize(&mut v);
        assert_eq!(
            v.columns,
            Some(5),
            "the count survives the list, to return with the grid"
        );
        assert!(!v.columns_enabled());
        assert!(v.is_list());
        v.step_columns(1);
        assert_eq!(v.columns, Some(5), "a list ignores a step");
    }

    #[test]
    fn a_stored_column_count_is_clamped_on_load() {
        let mut v: LibraryView = serde_json::from_str(r#"{"columns":99}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
        let mut v: LibraryView = serde_json::from_str(r#"{"columns":0}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        let mut v: LibraryView = serde_json::from_str(r#"{"autoFit":99}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.auto_fit, COLUMNS_MAX);
    }

    #[test]
    fn sorting_stands_a_drag_down() {
        let mut v = LibraryView::default();
        assert!(v.drag_reorders());
        v.sort = SortKey::Title;
        assert!(!v.drag_reorders());
        v.sort = SortKey::Manual;
        assert!(v.drag_reorders());
    }

    #[test]
    fn every_cover_and_layout_has_a_label() {
        assert_eq!(CoverFit::Fit.label(), "Fit");
        assert_eq!(CoverFit::Crop.label(), "Crop");
    }
}
