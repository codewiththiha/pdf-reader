//! Which pages a fixed-mode scan samples: `1..=min(total, cap)`.
//!
//! A plain range, named. The scan cap is the "adjustable page number" of the
//! paper pipeline — callers shrink it for a quicker scan or grow it for a
//! more representative one, and short books simply stop at their last page.

/// The pages a fixed-mode scan samples for a book of `total_pages` pages
/// under a `cap`-page budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanPlan {
    first: u32,
    last: u32,
}

impl ScanPlan {
    pub fn new(total_pages: u32, cap: u32) -> Self {
        Self {
            first: 1,
            last: total_pages.min(cap.max(1)),
        }
    }

    /// The pages to sample, in order.
    pub fn pages(&self) -> std::ops::RangeInclusive<u32> {
        self.first..=self.last
    }

    pub fn contains(&self, page: u32) -> bool {
        self.pages().contains(&page)
    }

    /// How many pages the plan samples.
    pub fn len(&self) -> u32 {
        // A book with no pages (or a degenerate total) leaves `last` below
        // `first`; the subtraction must not assume otherwise.
        if self.last < self.first {
            0
        } else {
            self.last - self.first + 1
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_book_scans_every_page() {
        let plan = ScanPlan::new(5, 100);
        assert_eq!(plan.len(), 5);
        assert_eq!(plan.pages().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
        assert!(plan.contains(5));
        assert!(!plan.contains(6));
    }

    #[test]
    fn a_long_book_stops_at_the_cap() {
        let plan = ScanPlan::new(1000, 100);
        assert_eq!(plan.len(), 100);
        assert_eq!(plan.pages().last(), Some(100));
    }

    #[test]
    fn a_zero_cap_scans_one_page_rather_than_none() {
        // A degenerate cap must not produce an empty scan: one page is the
        // smallest honest fixed colour.
        let plan = ScanPlan::new(50, 0);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.pages().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn a_book_without_pages_has_an_empty_plan() {
        assert!(ScanPlan::new(0, 100).is_empty());
        assert_eq!(ScanPlan::new(0, 100).len(), 0);
    }
}
