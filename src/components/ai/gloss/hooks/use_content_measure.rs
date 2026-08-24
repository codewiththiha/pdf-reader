//! Measures the real content height through an invisible twin so the card
//! height fits the answer (no minimum: a two-line answer gets a two-line
//! card, and a later chunk growing the twin morphs the card open a little
//! further). The twin itself is rendered by the popover — it must be a
//! pixel-exact replica of the surface's scroll column.

use leptos::{html, prelude::*};

use crate::components::ai::types::WordInfo;

/// Returns the node ref for the invisible measure twin plus the live height
/// signal it feeds.
pub fn use_content_measure(
    word: RwSignal<String>,
    word_info: RwSignal<Option<WordInfo>>,
) -> (NodeRef<html::Div>, RwSignal<f64>) {
    let measure_ref: NodeRef<html::Div> = NodeRef::new();
    let content_height = RwSignal::new(0.0_f64);

    Effect::new(move |_| {
        let _ = word_info.get();
        let _ = word.get(); // title wrap can change height independently of body
        if let Some(el) = measure_ref.get() {
            // Defer to the next frame so the twin reflects the new content.
            request_animation_frame(move || {
                let next = el.scroll_height() as f64;
                content_height.update(|h| {
                    if (*h - next).abs() > 2.0 {
                        *h = next;
                    }
                });
            });
        }
    });

    (measure_ref, content_height)
}
