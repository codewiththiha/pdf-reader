//! A single column of variably-sized items: the [`Strip`] backend behind the
//! [`Layout`] contract, plus sticky-header support and per-item estimates.

use alloc::vec::Vec;

use crate::{Budget, Strip, Viewport, Window};

use super::Layout;

/// A column (or row, for horizontal axes) of variably-sized items with a
/// fixed gap between them, optionally with sticky items.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListLayout {
    pub(crate) strip: Strip,
    pub(crate) gap: f64,
    sticky: Vec<usize>,
}

impl ListLayout {
    /// Build from explicit item sizes.
    pub fn new(sizes: impl IntoIterator<Item = f64>, gap: f64) -> Self {
        Self {
            strip: Strip::new(sizes, gap),
            gap,
            sticky: Vec::new(),
        }
    }

    /// Build a layout of `count` identically-sized items.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Build from a **per-item** estimate, to be refined later with
    /// [`set_size`](Self::set_size) as real sizes are measured.
    ///
    /// Every item is seeded from its OWN estimate — never from one global
    /// fallback. In a mixed-size document, seeding every unknown item from
    /// item 0's size mislocates everything below the first odd one, and the
    /// corrections then shift the content under the scroll anchor (the
    /// "landed on a different page after zoom-out" bug this API exists to
    /// prevent).
    pub fn estimated(count: usize, estimate: impl Fn(usize) -> f64, gap: f64) -> Self {
        Self::new((0..count).map(estimate), gap)
    }

    /// Mark items as sticky (`position: sticky` semantics): once their
    /// natural start scrolls past the viewport top, the most recent one
    /// pins to the top and is always included in the window.
    pub fn with_sticky(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.sticky = indices.into_iter().collect();
        self
    }

    /// The gap between adjacent items.
    #[inline]
    pub fn gap(&self) -> f64 {
        self.gap
    }

    /// Direct access to the backing strip (backends, benches, tests).
    #[inline]
    pub fn strip(&self) -> &Strip {
        &self.strip
    }
}

impl Layout for ListLayout {
    #[inline]
    fn item_count(&self) -> usize {
        self.strip.len()
    }

    #[inline]
    fn total(&self) -> f64 {
        self.strip.total()
    }

    #[inline]
    fn offset(&self, index: usize) -> f64 {
        self.strip.offset(index)
    }

    #[inline]
    fn size(&self, index: usize) -> f64 {
        self.strip.size(index)
    }

    #[inline]
    fn cross_offset(&self, _index: usize) -> f64 {
        0.0
    }

    #[inline]
    fn cross_size(&self, _index: usize) -> f64 {
        0.0
    }

    #[inline]
    fn index_at(&self, pos: f64) -> usize {
        self.strip.index_at(pos)
    }

    #[inline]
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        self.strip.index_at_hinted(pos, hint)
    }

    #[inline]
    fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        self.strip.overlapping(top, extent)
    }

    fn window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window> {
        self.strip
            .window_with_sticky(scroll, viewport.main, budget, &self.sticky)
    }

    fn window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        if self.sticky.is_empty() {
            self.strip
                .window_hinted(scroll, viewport.main, budget, hint)
        } else {
            self.strip
                .window_with_sticky(scroll, viewport.main, budget, &self.sticky)
        }
    }

    #[inline]
    fn dominant(&self, scroll: f64, extent: f64) -> usize {
        self.strip.dominant(scroll, extent)
    }

    #[inline]
    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        self.strip.set_size(index, new_size)
    }

    #[inline]
    fn item_size_hint(&self) -> f64 {
        self.strip.mean_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Overscan;

    #[test]
    fn estimated_seeds_each_item_from_its_own_size() {
        let l = ListLayout::estimated(4, |i| [100.0, 300.0, 200.0, 100.0][i], 0.0);
        assert_eq!(l.offset(0), 0.0);
        assert_eq!(l.offset(1), 100.0);
        assert_eq!(l.offset(2), 400.0);
        assert_eq!(l.offset(3), 600.0);
        assert_eq!(l.total(), 700.0);
    }

    #[test]
    fn sticky_matches_strip_window_with_sticky() {
        let sizes = [50.0, 100.0, 100.0, 100.0, 50.0, 100.0, 100.0, 100.0];
        let l = ListLayout::new(sizes, 0.0).with_sticky([0, 4]);
        let budget = Budget::screenfuls(0.0, 50);
        let win = l
            .window(l.offset(1), Viewport::main_only(300.0), budget)
            .unwrap();
        assert!(win.contains(0) && win.contains(1));
    }

    #[test]
    fn overscan_items_uses_the_mean_item_size() {
        let l = ListLayout::uniform(40, 100.0, 0.0);
        let a = l
            .window(2_000.0, Viewport::main_only(200.0), Budget::items(2, 100))
            .unwrap();
        let b = l
            .window(
                2_000.0,
                Viewport::main_only(200.0),
                Budget {
                    overscan: Overscan::Px(200.0),
                    max_items: 100,
                },
            )
            .unwrap();
        assert_eq!(a, b);
    }
}
