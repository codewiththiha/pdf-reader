//! The morphing surface component — a direct port of the Gloss reference's
//! `gloss-surface`. One fixed `div` whose `left/top/width/height/border-radius/
//! box-shadow/background` all come from the sprung box, with an inner content
//! wrapper sized to the *expanded* card (not the current box) whose opacity and
//! pointer-events are gated by morph progress.

use leptos::prelude::*;

use pdf_core::gloss::{smoothstep, GlossBox};

use crate::components::ai::types::GlossPhase;

/// Surface background by phase.
///
/// Processing and compact are deliberately SURFACE-heavy rather than a bare
/// accent tint: both sit directly on top of page text. Expanded cross-fades
/// from that fill to the opaque card as the morph completes — and the same
/// mix run backwards is what makes the outro read as the card dissolving into
/// the pill it is landing on.
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
    /// Full dismiss (the Close button).
    on_dismiss: Callback<()>,
    /// Begin dragging the expanded card.
    on_drag_start: Callback<(f64, f64, GlossBox)>,
    /// Card body: shimmer / word sections / error.
    children: Children,
) -> impl IntoView {
    let surface_style = Signal::derive(move || {
        let b = box_.get();
        let p = phase.get();
        let pr = progress.get();
        // Fade IN as the morph leaves the pill, fade OUT as it returns onto it.
        // The mark pill underneath owns the fully-collapsed look, so the outro
        // reads as "card shrinks AND dissolves back into the highlight".
        let opacity = smoothstep(pr, 0.05, 0.5);
        let pe = if p == GlossPhase::Expanded && pr > 0.4 {
            "auto"
        } else {
            "none"
        };
        format!(
            "left:{}px;top:{}px;width:{}px;height:{}px;border-radius:{}px;\
             box-shadow:{};background:{};opacity:{};pointer-events:{};",
            b.x,
            b.y,
            b.w,
            b.h,
            b.r,
            shadow_for(p),
            fill_for(p, pr),
            opacity,
            pe
        )
    });

    let content_style = Signal::derive(move || {
        let e = expanded.get();
        let ph = phase.get();
        let pr = progress.get();
        let opacity = if ph == GlossPhase::Processing {
            0.0
        } else {
            smoothstep(pr, 0.18, 0.7)
        };
        let interactive = pr > 0.55;
        format!(
            "width:{}px;height:{}px;opacity:{};pointer-events:{};",
            e.w,
            e.h,
            opacity,
            if interactive { "auto" } else { "none" }
        )
    });

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

    view! {
        <div
            class="gloss-surface"
            data-phase=move || phase_str.get()
            role=move || role.get()
            aria-label=move || aria_label.get()
            style=move || surface_style.get()
        >
            // Content wrapper: sized to the EXPANDED card, faded in by progress.
            <div
                class="absolute left-0 top-0 overflow-hidden text-ink"
                style=move || content_style.get()
            >
                // Drag handle — only live when expanded (guarded in the callback).
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

                <div
                    data-gloss-scroll=""
                    class="flex h-full min-h-0 flex-col overflow-y-auto \
                           overscroll-contain px-5 pb-3 pt-6"
                >
                    <header class="mb-4">
                        <h2 class="text-lg font-semibold leading-tight text-balance text-ink">
                            {move || word.get()}
                        </h2>
                    </header>
                    <div class="mb-4 h-px bg-line"></div>

                    {children()}

                    <div class="mt-auto flex justify-end pt-1">
                        <button
                            type="button"
                            class="min-h-11 px-1 text-sm text-muted transition-colors \
                                   hover:text-ink active:scale-[0.96]"
                            on:click=move |_| on_dismiss.run(())
                        >
                            "Close"
                        </button>
                    </div>
                </div>
            </div>
        </div>
    }
}
