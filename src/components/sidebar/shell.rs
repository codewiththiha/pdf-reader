//! Left sidebar with panels (outline / thumbnails). The coordinator owns the
//! open/close state machine and composes the chrome row, the book-identity
//! row, the two stacked panels, and the bottom rail (see the sibling modules).
//!
//! The `<aside>` is ALWAYS mounted and slides its `width` between 18rem and 0
//! (single-phase, no two-phase unmount). The inner content stays fixed at
//! `w-72` so it never collapses — `overflow-hidden` on the aside clips it
//! while closed. When collapsed the content is made `inert` so the clipped
//! rail can't be tab-focused / activated.
//!
//! THE TOGGLE SLIDES, unless told not to. The aside tweens its width over
//! `SIDEBAR_SLIDE_MS`, and `.sidebar-aside { contain: layout style }` keeps the
//! reflow from escaping the aside. The page follows the rail on every frame of
//! that slide — that is `follow_watcher`'s container follow, not a refit, and
//! the burst costs one raster pass because a follow holds its commit until the
//! container goes quiet.
//!
//! Settings → Animations owns two of the motions here. "Sidebar Slide" freezes
//! the width tween itself (`no_slide`, which adds the `no-slide` class and
//! releases the close hold in the same frame, because nothing is left waiting
//! on a slide that never runs); "Canvas Rides Sidebar" switches the per-frame
//! follow off, which leaves the page at its old scale for the length of the
//! slide and lands the new one in one step after it.
//!
//! CLOSE / OPEN. The last-open panel stays painted for the whole width slide
//! (`collapsing`), then the grid unmounts, which `cancelThumb`s every live
//! canvas and drops the backing stores. A reopen inside that window never
//! unmounts, never re-renders, never reallocates.
//!
//! OPEN mounts cells immediately so warm thumbnail bitmaps can paint while
//! the aside is moving. `intro` is paint-only: a two-frame toggle starts the
//! panel opacity transition without delaying the cell DOM. Cold cells keep
//! their skeleton until their own render completes. Rendering concurrency is
//! capped in the engine, so immediate mounting does not turn rapid toggles
//! into an unbounded raster backlog.
//!
//!
//! Tab switches (Thumbs ↔ Outline) still use `invisible` on the inactive
//! panel so the virtualization window stays engine-bound and a switch back
//! is instant. `hidden` (`display:none`) is still forbidden: its height
//! collapse would re-evict the window and re-render every thumb.
//!
//! Gotcha: each reactive `class=("name", cond)` toggle becomes one
//! `classList.add("name")` call — a space-separated token throws a swallowed
//! SyntaxError and the class is silently never applied. Keep every conditional
//! class to a SINGLE token (hence `w-0` and `border-r-0` as separate toggles).


use std::time::Duration;

use leptos::prelude::*;

use leptos::children::ViewFn;

use crate::components::primitives::hooks::use_timeout::use_debounce;
use crate::state::SidebarMode;

/// How long the aside takes to change width. The panel paint and the deferred
/// canvas release key off this so they land with the end of the slide rather
/// than trailing it, and the aside's own CSS transition is declared with the
/// matching `duration-300` — keep the two in step.
///
/// The reader can freeze the tween, which does not shorten this number: with
/// the switch off the aside jumps to its end width and the close hold is
/// released on the spot, so the timer this constant feeds never runs.
pub(crate) const SIDEBAR_SLIDE_MS: u64 = 300;

/// Selector for the sliding aside itself. The toolbar's title measurement
/// observes this element (its width changes every frame of the slide, unlike
/// the row, which only changes inset at the end of the close hold) — the
/// selector lives next to the class it targets so they cannot drift apart.
pub(crate) const SIDEBAR_ASIDE_SELECTOR: &str = "aside.sidebar-aside";

/// Ask the visible panel to scroll to wherever the reader currently is.
///
/// A CustomEvent rather than a signal in `AppState`: this is a one-shot
/// gesture with no state to hold, and a counter signal would need to be read
/// by both panels and could not distinguish "asked twice" from "asked once"
/// without extra bookkeeping. Same mechanism the PDF link layer uses.
pub(crate) fn request_reveal_active() {
    let Some(win) = web_sys::window() else { return };
    let Ok(event) = web_sys::CustomEvent::new("pdfreader:reveal-active") else {
        return;
    };
    _ = win.dispatch_event(&event);
}

/// Whether `panel` should stay painted this frame.
///
/// Open: only the active panel. Closing: the panel that was showing, for
/// the whole slide, so it can fade and clip with the rail labels instead
/// of popping off on frame one.
pub(crate) fn panel_is_shown(
    panel: SidebarMode,
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    mode == panel || (mode == SidebarMode::None && collapsing && last == panel)
}

/// Whether the rail is still painted and therefore still owns title-bar
/// chrome space. This stays true through the close slide after raw `mode`
/// has changed to `None`.
pub(crate) fn sidebar_is_present(mode: SidebarMode, collapsing: bool) -> bool {
    mode != SidebarMode::None || collapsing
}

/// Chrome-facing view of the open/close slide. The page that runs
/// [`sidebar_paint`] provides this; the title bar above the sidebar and the
/// floating label read it, so chrome follows the paint rather than the raw
/// mode (the mode flips to `None` on the close click, three hundred milliseconds
/// before the rail is out of the way).
#[derive(Clone, Copy)]
pub struct SidebarChromeCtx {
    /// The rail is on screen: open, or its close slide still running — in
    /// DOCKED or in overlay mode. The floating label and the native traffic
    /// lights key off this directly: a rail of either kind covers the window's
    /// top-left corner, so the label gets out of the way and the lights are
    /// hosted by the rail's own header gutter. Only the title bar's left inset
    /// is docked-only — an overlay rail floats above the bar and takes the
    /// corner from it instead of the bar yielding (see
    /// `app_shell/app_title_bar.rs`).
    pub present: Signal<bool>,
}

/// Paint flags derived from the open/close slide. The page composes
/// the panel hosts with these; the shell itself only owns the aside.
///
/// `Copy` because the flags are read from both rail mount points (see
/// `features/reader/rail.rs`) and from the page's own effects.
#[derive(Clone, Copy)]
pub struct SidebarPaint {
    pub show_outline: Signal<bool>,
    pub show_thumbs: Signal<bool>,
    pub thumbs_live: Signal<bool>,
    pub is_closed: Signal<bool>,
    pub outline_active: Signal<bool>,
    pub thumbs_active: Signal<bool>,
    /// Paint-only fade-in marker. It starts the panel opacity transition but
    /// never delays thumbnail-cell mounting.
    pub intro: Signal<bool>,
    /// The sidebar still occupies chrome space: open OR mid-close-slide.
    /// Whatever yields to the rail derives from this so it releases when the
    /// slide lands, not on the first frame of the close.
    pub present: Signal<bool>,
}

/// Drive the open/close slide bookkeeping and return the paint flags.
///
/// Opening mounts thumbnail cells immediately. Closing is the only timer-gated
/// direction: it keeps the last panel painted through the rail's width slide,
/// then releases DOM canvases at the same instant the slide lands.
pub fn sidebar_paint(mode: RwSignal<SidebarMode>, no_slide: Signal<bool>) -> SidebarPaint {
    let last_mode = RwSignal::new(SidebarMode::Thumbs);
    let collapsing = RwSignal::new(false);
    let intro = RwSignal::new(false);
    let cells_mounted = RwSignal::new(false);
    // Whether the previous mode was closed. Tab changes do not re-run the
    // open path, while a real None → panel transition does.
    let was_closed = StoredValue::new_local(true);
    // The end of the outro: hold the panel and its canvases for one slide,
    // then release. A debounce rather than a hand-rolled handle, because
    // `on_cleanup` then clears a fire that is still pending — which a stored
    // handle only did if the NEXT close arrived first, leaving a reader that was
    // gone writing to signals that were. Re-arming postpones the release instead
    // of queueing a second one, which is what a burst of toggles should do.
    let outro = use_debounce(Duration::from_millis(SIDEBAR_SLIDE_MS), move || {
        collapsing.set(false);
        // The engine cache remains; only live DOM canvases are released, so a
        // later open can synchronously blit.
        cells_mounted.set(false);
    });

    Effect::new(move |_| {
        let now = mode.get();
        let was = was_closed.get_value();

        if now != SidebarMode::None {
            last_mode.set(now);
            collapsing.set(false);
            outro.cancel();
            if was {
                // Let cached thumbnails ride the width slide. Cold cells keep
                // their own skeleton until renderThumb completes.
                cells_mounted.set(true);
                intro.set(true);
                // Keep the marker through one COMMITTED frame, then remove
                // it so the CSS opacity transition runs alongside the aside.
                // Two rAFs, not one: the first callback fires BEFORE the
                // frame with `intro` painted has been composited, so clearing
                // there would change the class in the same paint the marker
                // appeared in — no transition. The second callback runs
                // strictly after that frame is on screen, which is the
                // earliest point the fade can actually animate from.
                request_animation_frame(move || {
                    request_animation_frame(move || intro.set(false));
                });
            }
            was_closed.set_value(false);
        } else {
            was_closed.set_value(true);
            intro.set(false);
            // The initial closed state has no outro. Every panel → None
            // transition holds cells and chrome for the actual width slide —
            // and with the tween frozen there IS no slide to wait out, so
            // holding them would leave the title bar's inset released a
            // timer late by a rail that is already gone.
            if was || no_slide.get_untracked() {
                collapsing.set(false);
                cells_mounted.set(false);
            } else {
                collapsing.set(true);
                outro.trigger();
            }
        }
    });

    SidebarPaint {
        show_outline: Signal::derive(move || {
            panel_is_shown(SidebarMode::Outline, mode.get(), collapsing.get(), last_mode.get())
        }),
        show_thumbs: Signal::derive(move || {
            panel_is_shown(SidebarMode::Thumbs, mode.get(), collapsing.get(), last_mode.get())
        }),
        thumbs_live: Signal::derive(move || {
            thumbnail_cells_are_live(
                cells_mounted.get(),
                mode.get(),
                collapsing.get(),
                last_mode.get(),
            )
        }),
        is_closed: Signal::derive(move || mode.get() == SidebarMode::None),
        outline_active: Signal::derive(move || mode.get() == SidebarMode::Outline),
        thumbs_active: Signal::derive(move || mode.get() == SidebarMode::Thumbs),
        intro: intro.into(),
        present: Signal::derive(move || sidebar_is_present(mode.get(), collapsing.get())),
    }
}

/// Final mount gate for thumbnail cells. Even if the panel state would
/// normally preserve cells through an outro, there is nothing to preserve
/// when a close arrives before the opening delay created any cells.
pub(crate) fn thumbnail_cells_are_live(
    cells_mounted: bool,
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    cells_mounted && thumbs_should_stay_mounted(mode, collapsing, last)
}

/// Whether the thumbnail grid should keep its cells mounted.
///
/// Mounted while Thumbs is showing, while Outline is showing (so a tab
/// switch does not re-render every thumb), and while the Thumbs panel is
/// mid-outro. Dropped only once a close from Thumbs has finished — that
/// is what releases the live canvases without a quick-reopen spike.
pub(crate) fn thumbs_should_stay_mounted(
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    match mode {
        SidebarMode::Thumbs | SidebarMode::Outline => true,
        SidebarMode::None => collapsing && last == SidebarMode::Thumbs,
    }
}

#[component]
pub fn Sidebar(
    mode: RwSignal<SidebarMode>,
    // `no_slide` freezes the width tween (Settings → Animations). Read TRACKED
    // here, unlike the effects that consult the same flag: the class has to
    // move in the frame the switch does.
    #[prop(into)] no_slide: Signal<bool>,
    #[prop(into)] header: ViewFn,
    #[prop(optional, into)] info_row: Option<ViewFn>,
    #[prop(into)] panels: ViewFn,
    #[prop(optional, into)] footer: Option<ViewFn>,
) -> impl IntoView {
    view! {
        <aside
            class="sidebar-aside flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || matches!(mode.get(), SidebarMode::Thumbs | SidebarMode::Outline))
            class=("w-0", move || mode.get() == SidebarMode::None)
            class=("border-r-0", move || mode.get() == SidebarMode::None)
            class=("no-slide", move || no_slide.get())
        >
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || mode.get() == SidebarMode::None
            >
                {header.run()}
                {info_row.map(|row| row.run())}
                <div class="relative min-h-0 flex-1">
                    {panels.run()}
                </div>
                {footer.map(|row| row.run())}
            </div>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SIDEBAR_SLIDE_MS, panel_is_shown, sidebar_is_present, thumbnail_cells_are_live,
        thumbs_should_stay_mounted,
    };
    use crate::state::SidebarMode;

    #[test]
    fn the_slide_matches_the_css_duration() {
        // The aside carries a `duration-300` width transition, and the panel
        // paint plus the deferred canvas release both key off this constant,
        // so the outros land with the end of the slide rather than trailing
        // it. Rename either side and this test is the tripwire.
        assert_eq!(SIDEBAR_SLIDE_MS, 300);
    }

    #[test]
    fn a_close_keeps_the_open_panel_painted_until_the_slide_ends() {
        // Frame one of a Thumbs close: still painted, so it can fade/clip.
        assert!(panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        assert!(!panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // After the slide: both hidden.
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
    }

    #[test]
    fn chrome_space_is_held_until_the_close_slide_lands() {
        assert!(sidebar_is_present(SidebarMode::Thumbs, false));
        // Frame one of a close: raw mode is None, but the aside is still
        // sliding and title-bar chrome must remain aligned with it.
        assert!(sidebar_is_present(SidebarMode::None, true));
        assert!(!sidebar_is_present(SidebarMode::None, false));
    }

    #[test]
    fn a_tab_switch_shows_only_the_active_panel() {
        assert!(panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
    }

    #[test]
    fn thumbnail_cells_require_a_real_mount() {
        // The helper remains defensive: an outro state alone never creates
        // cells; the open transition is what mounts them.
        assert!(!thumbnail_cells_are_live(
            false,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
        // Once cells genuinely exist, the ordinary open and outro paths keep
        // working as before.
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs,
        ));
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
    }

    #[test]
    fn thumbs_stay_mounted_across_a_tab_switch_but_not_a_finished_close() {
        // Instant Thumbs ↔ Outline: keep the canvases.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs
        ));
        // Mid-outro from Thumbs: keep them so a quick reopen is free.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // Slide finished, or we closed from Outline: drop the live canvases.
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Outline
        ));
    }
}
