//! Gloss card geometry: box math, viewport placement, spring stepping.
//!
//! Pure domain math for the AI word-card morph (the Gloss reference's
//! `src/lib/geometry.ts`, ported). No wasm, no DOM, no leptos — unit-testable
//! on the host via `cargo test -p pdf-core gloss`.
//!
//! The card is one rectangle whose `left/top/width/height` and corner
//! `radius` are all driven by a single critically-damped spring, so the chip's
//! pill radius morphs into the card radius in the same motion that grows the
//! box. Named `GlossBox` to avoid colliding with `std::boxed::Box`.

/// A positioned, sized, rounded rectangle — the five fields the spring drives.
///
/// Serializable because [`GlossMark`] persists one of these to localStorage
/// (and ships one through a `CustomEvent` detail when a mark is clicked).
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct GlossBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Corner radius — animated alongside the box so the chip's pill radius
    /// morphs into the card radius.
    pub r: f64,
}

/// A persisted gloss highlight: the word that was explained, plus WHERE it is
/// on the page in a form that survives everything the viewport does to it.
///
/// The rect is deliberately in **page space** — unscaled CSS px measured from
/// the `.pdf-page` host's origin — not in viewport space. A native
/// `Selection` cannot be persisted (it is cleared when the card opens, it dies
/// with the text-layer spans the virtualizer unmounts, and there is only ever
/// one of it), so the mark is re-projected onto the screen as
/// `host_rect.origin + rect * display_scale` every time the page mounts. That
/// is what makes the highlight survive scroll, zoom, remounts and sessions.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlossMark {
    pub id: String,
    pub page: u32,
    pub word: String,
    pub context: String,
    /// Page space (unscaled CSS px from the page origin). Screen = rect * display_scale.
    pub rect: GlossBox,
}

/// A point of interest in *page* space (unscaled page coordinates). Unlike a
/// screen rect it survives scroll, zoom and view-mode flips: the live screen
/// box is re-derived from the page host element whenever anything moves.
///
/// Shared by the selection Info pill and the gloss card so both glue to the
/// page without each inventing its own coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PageAnchor {
    pub page: u32,
    pub rect: GlossBox,
}

impl PageAnchor {
    pub fn from_mark(m: &GlossMark) -> Self {
        Self {
            page: m.page,
            rect: m.rect,
        }
    }
}

/// `f64::clamp` lifted to a free fn so call sites read as the reference.
pub fn clamp(n: f64, min: f64, max: f64) -> f64 {
    n.clamp(min, max)
}

/// Hermite smoothstep: 0 below `edge0`, 1 above `edge1`, smooth between.
/// Drives the card content's opacity/interactivity fade as the morph progresses.
pub fn smoothstep(t: f64, edge0: f64, edge1: f64) -> f64 {
    let x = clamp((t - edge0) / (edge1 - edge0).max(0.0001), 0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Pad a box outward by `(pad_x, pad_y)`, rounding the corners to at most half
/// the new height so it stays a pill when it is short (the chip case).
pub fn pad_box(b: GlossBox, pad_x: f64, pad_y: f64) -> GlossBox {
    GlossBox {
        x: b.x - pad_x,
        y: b.y - pad_y,
        w: b.w + pad_x * 2.0,
        h: b.h + pad_y * 2.0,
        r: (18.0_f64).min((b.h + pad_y * 2.0) / 2.0),
    }
}

/// Whether two boxes are equal to within `epsilon` on all five fields — the
/// spring's "settled" and "already snapped" tests.
pub fn boxes_close(a: GlossBox, b: GlossBox, epsilon: f64) -> bool {
    (a.x - b.x).abs() < epsilon
        && (a.y - b.y).abs() < epsilon
        && (a.w - b.w).abs() < epsilon
        && (a.h - b.h).abs() < epsilon
        && (a.r - b.r).abs() < epsilon
}

/// Clamp an expanded card of intrinsic `size_w` x `size_h` into a viewport of
/// `view_w` x `view_h`, hugging the anchor edge and keeping a 16px margin. The
/// card never overflows the window or escapes its padding ring.
pub fn place_expanded(
    anchor: GlossBox,
    size_w: f64,
    size_h: f64,
    view_w: f64,
    view_h: f64,
    radius: f64,
) -> GlossBox {
    let pad = 16.0;
    let w = size_w.min((view_w - pad * 2.0).max(240.0));
    let h = size_h.min((view_h - pad * 2.0).max(200.0));
    let mut x = anchor.x;
    let mut y = anchor.y;
    if x + w > view_w - pad {
        x = view_w - pad - w;
    }
    if x < pad {
        x = pad;
    }
    if y + h > view_h - pad {
        y = view_h - pad - h;
    }
    if y < pad {
        y = pad;
    }
    GlossBox { x, y, w, h, r: radius }
}

/// Spring stiffness and damping for the morph. Stiffness 210 / damping 26 is
/// mildly underdamped (critical ≈ 29 at mass 1): a confident pop with one small
/// settle, matching the reference's feel.
const STIFFNESS: f64 = 210.0;
const DAMPING: f64 = 26.0;

/// One explicit-Euler step of a 1-D spring toward `t` from `c` at velocity `v`.
/// Returns `(position, velocity)`. The dt is clamped by the caller so long
/// frames never blow the integrator past its stability bound (2 / sqrt(k) ≈ 0.138s).
fn spring_axis(c: f64, v: f64, t: f64, dt: f64) -> (f64, f64) {
    let force = STIFFNESS * (t - c) - DAMPING * v;
    let nv = v + force * dt;
    (c + nv * dt, nv)
}

/// One spring step over all five box fields. Returns `(next_box, next_velocity)`.
pub fn step_spring(cur: GlossBox, vel: GlossBox, target: GlossBox, dt: f64) -> (GlossBox, GlossBox) {
    let (x, vx) = spring_axis(cur.x, vel.x, target.x, dt);
    let (y, vy) = spring_axis(cur.y, vel.y, target.y, dt);
    let (w, vw) = spring_axis(cur.w, vel.w, target.w, dt);
    let (h, vh) = spring_axis(cur.h, vel.h, target.h, dt);
    let (r, vr) = spring_axis(cur.r, vel.r, target.r, dt);
    (
        GlossBox { x, y, w, h, r },
        GlossBox {
            x: vx,
            y: vy,
            w: vw,
            h: vh,
            r: vr,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothstep_is_clamped_at_both_edges_and_smooth_between() {
        assert!((smoothstep(-1.0, 0.0, 1.0)).abs() < 1e-9);
        assert!((smoothstep(0.0, 0.0, 1.0)).abs() < 1e-9);
        assert!((smoothstep(1.0, 0.0, 1.0) - 1.0).abs() < 1e-9);
        assert!((smoothstep(2.0, 0.0, 1.0) - 1.0).abs() < 1e-9);
        // The midpoint of a smoothstep is exactly 0.5.
        assert!((smoothstep(0.5, 0.0, 1.0) - 0.5).abs() < 1e-9);
        // Monotonic across the ramp.
        let mut last = -1.0;
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let s = smoothstep(t, 0.0, 1.0);
            assert!(s >= last, "non-monotonic at {t}: {s} < {last}");
            last = s;
        }
    }

    #[test]
    fn pad_box_grows_outward_and_caps_radius_at_half_height() {
        let b = GlossBox { x: 100.0, y: 50.0, w: 20.0, h: 14.0, r: 0.0 };
        let p = pad_box(b, 5.0, 3.0);
        // Outward by the exact padding on every side.
        assert!((p.x - 95.0).abs() < 1e-9);
        assert!((p.y - 47.0).abs() < 1e-9);
        assert!((p.w - 30.0).abs() < 1e-9);
        assert!((p.h - 20.0).abs() < 1e-9);
        // Radius is half the padded height (pill) since that is < 18.
        assert!((p.r - 10.0).abs() < 1e-9);
    }

    #[test]
    fn place_expanded_keeps_the_card_inside_the_viewport_margin() {
        // Anchor pushed into the far corner: the card must slide inboard so its
        // right/bottom edges never cross the 16px margin.
        let anchor = GlossBox { x: 1900.0, y: 1000.0, w: 40.0, h: 20.0, r: 0.0 };
        let card = place_expanded(anchor, 320.0, 420.0, 1920.0, 1080.0, 24.0);
        assert!(card.x >= 16.0);
        assert!(card.y >= 16.0);
        assert!(card.x + card.w <= 1920.0 - 16.0 + 1e-6);
        assert!(card.y + card.h <= 1080.0 - 16.0 + 1e-6);
        assert!((card.r - 24.0).abs() < 1e-9);
    }

    #[test]
    fn place_expanded_shrinks_a_card_that_does_not_fit() {
        // A card taller than the viewport is clamped to (vh - 2*pad), never cropped.
        let anchor = GlossBox { x: 100.0, y: 100.0, w: 40.0, h: 20.0, r: 0.0 };
        let card = place_expanded(anchor, 320.0, 2000.0, 800.0, 600.0, 24.0);
        assert!(card.h <= 600.0 - 32.0 + 1e-6);
        assert!(card.w <= 800.0 - 32.0 + 1e-6);
    }

    #[test]
    fn a_gloss_mark_round_trips_through_json() {
        // The persistence contract: what localStorage holds must come back
        // byte-identical, because the rect IS the anchor. A silently dropped
        // field would put the highlight on the wrong word after a restart.
        let mark = GlossMark {
            id: "g3-1700000000000".to_string(),
            page: 3,
            word: "palimpsest".to_string(),
            context: "a manuscript page, a palimpsest, scraped clean".to_string(),
            rect: GlossBox { x: 120.5, y: 44.25, w: 62.0, h: 13.5, r: 0.0 },
        };
        let json = serde_json::to_string(&mark).expect("serialize");
        let back: GlossMark = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mark, back);
    }

    #[test]
    fn boxes_close_is_field_wise_within_epsilon() {
        let a = GlossBox { x: 1.0, y: 2.0, w: 3.0, h: 4.0, r: 5.0 };
        let b = GlossBox { x: 1.1, y: 2.1, w: 3.1, h: 4.1, r: 5.1 };
        assert!(boxes_close(a, b, 0.2));
        assert!(!boxes_close(a, b, 0.05));
    }

    #[test]
    fn the_spring_converges_to_its_target_within_about_two_seconds() {
        // THE point of the spring: starting from the (small) chip box and at
        // rest, ~120 60-fps frames bring every field to within half a pixel of
        // the target. If this regresses the card either never settles (battery
        // drain from an endless rAF loop) or snaps (no morph).
        let target = GlossBox { x: 200.0, y: 150.0, w: 320.0, h: 420.0, r: 24.0 };
        let mut cur = GlossBox { x: 40.0, y: 30.0, w: 30.0, h: 20.0, r: 10.0 };
        let mut vel = GlossBox::default();
        let dt = 1.0 / 60.0;
        for _ in 0..200 {
            let (next, next_vel) = step_spring(cur, vel, target, dt);
            cur = next;
            vel = next_vel;
        }
        assert!(boxes_close(cur, target, 0.5), "did not settle: {cur:?}");
    }

    #[test]
    fn the_spring_is_stable_on_a_dropped_frame() {
        // A long frame (dt clamped at the caller) must not launch the box off to
        // infinity. Stepping at the stability ceiling stays bounded and still
        // approaches the target.
        let target = GlossBox { x: 0.0, y: 0.0, w: 300.0, h: 300.0, r: 20.0 };
        let mut cur = GlossBox::default();
        let mut vel = GlossBox::default();
        // Several successive "dropped" frames at the caller's clamp (0.032s).
        for _ in 0..400 {
            let (next, next_vel) = step_spring(cur, vel, target, 0.032);
            cur = next;
            vel = next_vel;
            assert!(cur.x.is_finite() && cur.w.is_finite(), "blew up: {cur:?}");
        }
        assert!(boxes_close(cur, target, 0.5), "did not settle on long frames: {cur:?}");
    }
}
