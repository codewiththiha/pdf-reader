//! Live content-box size of a `NodeRef` element as a signal. The resize
//! observer is installed once per element; replacements (remounts) re-arm.

use leptos::{html, prelude::*};

use super::use_resize_observer::use_resize_observer;

/// Live `(content_width, content_height)` of the target element.
///
/// The initial value reflects the layout the element mounted with; every
/// subsequent resize updates the signal. The observer and its closure are
/// disconnected and dropped (in that order) on owner cleanup.
#[allow(dead_code)] // consumed by the thumbnail-grid / sidebar adoptions (plan phase 5+)
pub fn use_element_size(target: NodeRef<html::Div>) -> RwSignal<(f64, f64)> {
    let size = RwSignal::new((0.0, 0.0));

    // Seed with the live layout once the node exists — the observer only
    // fires on CHANGES, not on install.
    {
        Effect::new(move |_| {
            let Some(el) = target.get() else {
                return;
            };
            let rect = el.get_bounding_client_rect();
            size.set((rect.width(), rect.height()));
        });
    }

    use_resize_observer(target, move |entry| {
        let rect = entry.content_rect();
        size.set((rect.width(), rect.height()));
    });

    size
}
