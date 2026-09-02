//! Auto-hide surfaces: the whole hover machine, once.
//!
//! [`use_hover_visibility`] is the timer primitive — `show` reveals and
//! cancels a pending hide, `hide_later` schedules one unless a hold says
//! otherwise. Every real surface then needs the same four lines on top of
//! it, and the title bar and the bottom bar had written them out verbatim:
//!
//!   - one `hovered` truth shared by the N elements that make up the
//!     surface (a hover band plus the row it reveals, a bottom strip plus
//!     the bar), so an enter on either counts and a leave on either counts;
//!   - an effect that rechecks the moment a hold releases, because a hold
//!     that ends while the pointer is already gone never produces another
//!     `mouseleave` to hide on;
//!   - `visible = pinned || hovered`, where a pin exists.
//!
//! This module owns that composite so a new auto-hide surface is one call
//! instead of another twenty copied lines (and another chance to re-invent
//! the bottom-edge-exit and pointer-capture bugs).
//!
//! Contract, same as the rest of `hooks`: nothing here knows what holds a
//! surface open. A hold is a `Signal<bool>` — the title bar's open popovers
//! and the bottom bar's scrubber drag are the callers' business.

use std::rc::Rc;
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use crate::hooks::use_timeout::use_hover_visibility;

/// The grace period every chrome surface hides after. Shared so the title
/// bar and the bottom bar cannot drift apart by a hundred milliseconds.
pub const DEFAULT_HOVER_DELAY: Duration = Duration::from_millis(400);

/// How a surface reveals: the grace period, what holds it open, what pins
/// it. `..Default::default()` covers the two optional halves.
#[derive(Clone, Copy)]
pub struct HoverConfig {
    /// Pointer must be off the surface this long before it hides.
    pub delay: Duration,
    /// While true the surface never hides (an open popover, a drag).
    pub hold: Option<Signal<bool>>,
    /// While true the surface is always visible, hover or not.
    pub pin: Option<Signal<bool>>,
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self { delay: DEFAULT_HOVER_DELAY, hold: None, pin: None }
    }
}

/// A reveal controller: the visibility to render from, plus the two pointer
/// edges to bind on every element that belongs to the surface.
#[derive(Clone)]
pub struct HoverReveal {
    /// What to render from: `pin || hovered_visible`.
    pub visible: Signal<bool>,
    /// The raw hover truth, without the pin — the title bar publishes this
    /// one to its descendants.
    pub hovered_visible: RwSignal<bool>,
    enter: Rc<dyn Fn()>,
    leave: Rc<dyn Fn()>,
}

impl HoverReveal {
    /// Pointer entered one of the surface's elements.
    pub fn enter(&self) {
        (self.enter)();
    }

    /// Pointer left one of the surface's elements.
    pub fn leave(&self) {
        (self.leave)();
    }

    /// A fresh `(enter, leave)` pair to move into one element's handlers.
    /// Call it once per element — a surface made of a band and a row binds
    /// twice, and both edges feed the same `hovered` truth.
    pub fn bind(&self) -> (Rc<dyn Fn()>, Rc<dyn Fn()>) {
        (Rc::clone(&self.enter), Rc::clone(&self.leave))
    }
}

/// Build a reveal controller owned by the current reactive owner.
///
/// Call it from a component body: the hide timer and its cleanup belong to
/// the component's owner, not to an effect scope that is disposed per run.
pub fn use_hover_reveal(config: HoverConfig) -> HoverReveal {
    let hold = config.hold;
    let held: Rc<dyn Fn() -> bool> = Rc::new(move || hold.is_some_and(|h| h.get()));
    build(config.delay, held, config.pin)
}

/// [`use_hover_reveal`] for a hold that is a closure rather than a signal,
/// with the shared delay and no pin — the common case.
pub fn use_hover_reveal_with(delay: Duration, hold: impl Fn() -> bool + 'static) -> HoverReveal {
    build(delay, Rc::new(hold), None)
}

fn build(delay: Duration, hold: Rc<dyn Fn() -> bool>, pin: Option<Signal<bool>>) -> HoverReveal {
    let postpone = Rc::clone(&hold);
    let hover = use_hover_visibility(delay, move || postpone());

    // `StoredValue`, not a signal: the flag is read inside the recheck
    // effect (a tracked read there would re-run it on every hover) and
    // cloned into as many element handlers as the surface has.
    let hovered = StoredValue::new_local(false);
    let enter: Rc<dyn Fn()> = Rc::new({
        let show = hover.show.clone();
        move || {
            hovered.set_value(true);
            show();
        }
    });
    let leave: Rc<dyn Fn()> = Rc::new({
        let hide = hover.hide_later.clone();
        move || {
            hovered.set_value(false);
            hide();
        }
    });

    // The non-obvious edge: a hold released while the pointer is already
    // elsewhere produces no `mouseleave`, so nothing would ever schedule
    // the hide. This effect tracks the hold and settles it.
    let recheck = hover.hide_later.clone();
    let watch = Rc::clone(&hold);
    Effect::new(move |_| {
        let held_now = watch();
        if !held_now && !hovered.get_value() {
            recheck(); // the hold is gone and so is the pointer → hide
        }
    });

    let hovered_visible = hover.visible;
    let visible = match pin {
        Some(pin) => Signal::derive(move || pin.get() || hovered_visible.get()),
        None => Signal::derive(move || hovered_visible.get()),
    };

    HoverReveal { visible, hovered_visible, enter, leave }
}

/// Whether the point still lands on the surface. Pointer capture keeps a
/// drag's events glued to the captured element — including releases that
/// land outside — so after a drag the release coordinates are the only
/// trustworthy answer to "is the pointer still over us?".
/// `element_from_point` skips `pointer-events: none` decorations, so a
/// release over an inert overlay still counts as on-surface.
fn released_on(surface: &web_sys::Element, x: f32, y: f32) -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.element_from_point(x, y))
        .is_some_and(|el| surface.contains(Some(&el)))
}

/// The pointer-capture half, opt-in: a drag inside the surface holds it
/// open, and the release re-synchronises the hover truth.
///
/// Returns the `pointerup` / `pointercancel` handler to bind alongside a
/// `on:pointerdown=move |_| dragging.set(true)`. `dragging` is the caller's
/// so it can also be the [`HoverConfig::hold`] the reveal was built with —
/// which is why it is passed in rather than created here. The reveal is
/// taken by value (it is `Clone`): the returned handler outlives this call,
/// and a borrow would tie it to the caller's stack frame.
pub fn use_drag_hold(
    surface: NodeRef<html::Div>,
    dragging: RwSignal<bool>,
    reveal: HoverReveal,
) -> impl Fn(leptos::ev::PointerEvent) + Clone + 'static {
    let (_, leave) = reveal.bind();
    move |ev: leptos::ev::PointerEvent| {
        if !dragging.get_untracked() {
            return;
        }
        dragging.set(false);
        // Capture swallowed the surface's `mouseleave` for the whole drag,
        // so this is where it learns the pointer's real position again.
        let over = surface
            .get()
            .is_some_and(|el| released_on(&el, ev.client_x() as f32, ev.client_y() as f32));
        if !over {
            leave();
        }
    }
}

/// The ergonomic wrapper: any single element that should reveal on hover
/// and hide after the grace period, without the caller touching the two
/// pointer edges. Surfaces built from several elements (a band plus a row)
/// bind [`HoverReveal::bind`] themselves instead.
#[component]
pub fn HoverRevealSurface(
    /// The controller — build it with [`use_hover_reveal`] so the caller
    /// can read `visible` for its own layout too.
    reveal: HoverReveal,
    /// Classes for the wrapper element, applied on top of the hidden-state
    /// toggles below.
    #[prop(optional, into)]
    class: String,
    children: Children,
) -> impl IntoView {
    let (enter, leave) = reveal.bind();
    let visible = reveal.visible;
    view! {
        <div
            class=class
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| enter()
            on:mouseleave=move |_| leave()
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            {children()}
        </div>
    }
}
