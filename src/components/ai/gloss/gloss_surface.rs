//! The morphing surface component — a direct port of the Gloss reference's
//! `gloss-surface`, composed on the generic [`FloatingCard`] primitive so the
//! box morph mechanics, the no-reflow content wrapper and the drag-handle
//! slot are shared with any future floating card.
//!
//! One fixed `div` whose `left/top/width/height/border-radius` come from the
//! sprung box; the primitive owns that shell. This module keeps ONLY the
//! gloss policy: phase fills/shadows/opacity (the `GlossBox` → `FloatBox`
//! conversions happen here, at the primitive boundary), the drag-handle
//! contents, and the header + scroll column.
//!
//! Dismiss is not a button on the card: Escape / outside-tap / origin-exit are
//! owned by the popover's window listeners.

use leptos::prelude::*;

use pdf_core::gloss::{smoothstep, GlossBox};

use crate::components::ai::types::GlossPhase;
use crate::components::primitives::floating::floating_card::FloatingCard;
use crate::components::primitives::floating::types::FloatBox;

/// Surface background by phase.
///
/// Processing and compact are deliberately SURFACE-heavy rather than a bare
/// accent tint: both sit directly on top of page text. Expanded cross-fades
/// from that fill to the opaque card as the morph completes — and the same
/// mix run backwards is what makes the outro read as the card dissolving into
/// the stroke it is landing on.
fn fill_for(phase: GlossPhase, progress: f64) -> String {
    match phase {
        GlossPhase::Processing => concat!(
            "color-mix(in oklab, var(--color-surface) 78%, ",
            "color-mix(in oklab, var(--color-accent) 34%, transparent))",
        )
        .into(),
        GlossPhase::Compact => concat!(
            "color-mix(in oklab, var(--color-surface) 70%, ",
            "color-mix(in oklab, var(--color-accent) 30%, transparent))",
        )
        .into(),
        GlossPhase::Expanded => {
            let p = (progress.clamp(0.0, 1.0) * 100.0).round() as u32;
            format!(
                "color-mix(in oklab, var(--color-surface) {p}%, \
                 color-mix(in oklab, var(--color-accent) 36%, transparent))"
            )
        }
    }
}

/// Surface shadow by phase — an accent halo while processing/thin, the card
/// elevation once expanded.
fn shadow_for(phase: GlossPhase) -> String {
    match phase {
        GlossPhase::Processing => concat!(
            "0 0 0 1px color-mix(in oklab, var(--color-accent) 50%, transparent),",
            " 0 0 20px 5px color-mix(in oklab, var(--color-accent) 38%, transparent)"
        )
        .into(),
        GlossPhase::Compact => {
            "0 0 0 1px color-mix(in oklab, var(--color-accent) 22%, transparent)".into()
        }
        GlossPhase::Expanded => "var(--gloss-shadow-card)".into(),
    }
}

#[component]
pub fn GlossSurface(
    /// The card's geometry phase (independent of the AI data phase).
    phase: Signal<GlossPhase>,
    /// The current sprung box — written per-frame by the spring.
    box_: Signal<GlossBox>,
    /// The expanded target — the content wrapper is sized to THIS, not to the
    /// current box, so the text never reflows as the box morphs.
    expanded: Signal<GlossBox>,
    /// 0..1 morph progress; drives the content fade-in.
    progress: Signal<f64>,
    /// The selected word, for the card header.
    word: Signal<String>,
    /// Begin dragging the expanded card.
    on_drag_start: Callback<(f64, f64, GlossBox)>,
    /// Card body: shimmer / word sections / error.
    children: Children,
) -> impl IntoView {
    // The primitive speaks FloatBox; the domain speaks GlossBox. Convert at
    // the seam — one place, one From impl, math shared in pdf_core.
    let box_f = Signal::derive(move || FloatBox::from(box_.get()));
    let expanded_f = Signal::derive(move || FloatBox::from(expanded.get()));

    // Gloss policy: fills, shadows, surface opacity/pointer-events. This is
    // the `surface_style` extra the primitive appends after the geometry.
    let surface_style = Signal::derive(move || {
        let p = phase.get();
        let pr = progress.get();
        // Fade IN as the morph leaves the stroke, fade OUT as it returns onto
        // it. The mark stroke underneath owns the fully-collapsed look, so
        // the outro reads as "card shrinks AND dissolves back into the
        // highlight".
        let opacity = smoothstep(pr, 0.05, 0.5);
        let pe = if p == GlossPhase::Expanded && pr > 0.4 {
            "auto"
        } else {
            "none"
        };
        format!(
            "box-shadow:{};background:{};opacity:{};pointer-events:{};",
            shadow_for(p),
            fill_for(p, pr),
            opacity,
            pe
        )
    });

    let content_opacity = Signal::derive(move || {        if phase.get() == GlossPhase::Processing {
            0.0
        } else {
            smoothstep(progress.get(), 0.18, 0.7)
        }
    });
    let content_interactive = Signal::derive(move || progress.get() > 0.55);

    let phase_str = Signal::derive(move || match phase.get() {
        GlossPhase::Processing => "processing",
        GlossPhase::Expanded => "expanded",
        GlossPhase::Compact => "compact",
    });
    let role = Signal::derive(move || match phase.get() {
        GlossPhase::Expanded => "dialog",
        _ => "",
    });
    let aria_label = Signal::derive(move || match phase.get() {
        GlossPhase::Expanded => format!("Gloss for {}", word.get()),
        _ => String::new(),
    });

    let word_h = word.clone();
    let drag_handle: leptos::children::Children = Box::new(move || {
        // Drag handle — only live when expanded (guarded in the callback).
        view! {
            <div
                class="absolute left-0 right-0 top-2.5 z-10 flex cursor-grab \
                       justify-center active:cursor-grabbing"
                style:opacity=move || format!("{}", smoothstep(progress.get(), 0.5, 0.95))
                on:pointerdown=move |ev| {
                    if phase.get_untracked() != GlossPhase::Expanded {
                        return;
                    }
                    ev.prevent_default();
                    ev.stop_propagation();
                    on_drag_start.run((ev.client_x() as f64, ev.client_y() as f64, box_.get_untracked()));
                }
            >
                <span class="block h-1 w-8 rounded-full bg-ink/15"></span>
            </div>
        }
        .into_any()
    });

    view! {
        <FloatingCard
            box_=box_f
            expanded=expanded_f
            surface_style=surface_style
            content_opacity=content_opacity
            content_interactive=content_interactive
            drag_handle=drag_handle
            data_phase=phase_str
            role=role
            aria_label=aria_label
            class="gloss-surface"
        >
            <GlossBody word=word_h>
                {children()}
            </GlossBody>
        </FloatingCard>
    }
}

/// The card body: padded header + separator + content column. ONE definition
/// shared by the visible surface and the hidden measure twin in
/// [`gloss_ai_popover`](super::gloss_ai_popover), so the twin's measured
/// height can never drift from the real layout — that is what makes
/// `content_height` correct. Block-flow container: the flex-squeeze
/// protection lives on this root (`shrink-0`), so inner sections need none.
#[component]
pub(crate) fn GlossBody(
    /// The word being explained (header title).
    #[prop(into)]
    word: Signal<String>,
    /// Body content (word sections / shimmer / error row).
    children: Children,
) -> impl IntoView {
    view! {
        <div class="shrink-0 px-5 pb-4 pt-6">
            <header class="mb-4">
                <h2 class="text-lg font-semibold leading-tight text-balance text-ink">
                    {move || word.get()}
                </h2>
            </header>
            <div class="mb-4 h-px bg-line"></div>
            <div>{children()}</div>
        </div>
    }
}
