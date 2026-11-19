//! Turns the rig's pose into the pointer's geometry, whichever style is out. The glove and the
//! chicken ride the telescoping pole (a rubber grip, a chrome ferrule and five chrome
//! sections with a collar at each joint); the sword, the wand and the mallet are props of
//! their own. Raymarched models (glove, chicken, mallet head) are drawn by the shader.

use crate::draw::{Painter, Prim, RPoint, Rgba, kind, rgba, ribbon, with_alpha};
use crate::math::{V2, clamp01, v2};
use crate::sim::Pose;
use crate::style::{BLADE, CHICKEN, HILT, MALLET_HANDLE, Style, WAND};

/// Where shadows fall: down and to the right, as from a light up and to the left.
pub const SHADOW_DIR: V2 = v2(0.52, 0.86);

/// Section radii, base to tip, in pointer sizes.
const SECTIONS: [f32; 5] = [0.046, 0.041, 0.036, 0.031, 0.026];
const GRIP_R: f32 = 0.088;
const FERRULE_R: f32 = 0.062;

/// What the pose doesn't say about how the pointer looks this frame.
pub struct Look<'a> {
    pub style: Style,
    /// The sword's blade colour (sRGB).
    pub blade: [f32; 3],
    /// How far the sword's blade is out, 0..1.
    pub ignition: f32,
    /// Recent blade positions (emitter, tip), oldest first: the swing's afterimage.
    pub trail: &'a [(V2, V2)],
    /// Seconds, for flicker and twinkle.
    pub time: f32,
    /// 0..1, just after a click (the sword flares).
    pub flash: f32,
}

/// The pointer's frame: its tip, the axis running back from it, and the side.
struct Frame {
    tip: V2,
    down: V2,
    right: V2,
    s: f32,
}

impl Frame {
    fn new(pose: &Pose) -> Frame {
        Frame { tip: pose.tip, down: pose.down, right: v2(pose.down.y, -pose.down.x), s: pose.size }
    }
    /// A point given in pointer units (x across, y back along the axis).
    fn at(&self, l: V2) -> V2 {
        self.tip + self.right * (l.x * self.s) + self.down * (l.y * self.s)
    }
}

/// Where the sword's blade starts (the emitter) and how long it is when fully lit.
pub fn blade_root(pose: &Pose) -> (V2, f32) {
    (pose.tip + pose.down * (BLADE * pose.size), BLADE * pose.size)
}

/// Draws the pointer into the slot above the HUD.
pub fn draw(p: &mut Painter<'_>, pose: &Pose, look: &Look<'_>, screen_h: f32) {
    if !pose.visible { return; }
    let f = Frame::new(pose);
    match look.style {
        Style::Glove => {
            let curve = Curve::new(pose, 72);
            pole_shadow(p, pose, &curve, screen_h);
            object_shadow(p, pose, &f, 0.0, v2(0.075, 0.5), 0.6);
            pole(p, pose, &curve);
            object(p, pose, &f, 0.0, v2(0.075, 0.5), 0.6);
        }
        Style::Chicken => {
            let curve = Curve::new(pose, 72);
            let big = Frame { s: f.s * CHICKEN, ..Frame::new(pose) };
            pole_shadow(p, pose, &curve, screen_h);
            object_shadow(p, pose, &big, 1.0, v2(0.0, 0.55), 0.64);
            pole(p, pose, &curve);
            object(p, pose, &big, 1.0, v2(0.0, 0.55), 0.64);
        }
        Style::Saber => saber(p, pose, &f, look),
        Style::Wand => wand(p, pose, &f, look),
        Style::Mallet => mallet(p, pose, &f),
    }
}

fn object(p: &mut Painter<'_>, pose: &Pose, f: &Frame, model: f32, center: V2, radius: f32) {
    let c = f.at(center);
    p.prim(Prim {
        a: [c.x, c.y, c.x, c.y],
        b: [f.s, 0.0, radius * f.s + 2.0, kind::OBJECT],
        c: [f.tip.x, f.tip.y, f.down.x, f.down.y],
        d: [pose.roll, -0.32 * clamp01(pose.press), pose.press, model],
        ..Default::default()
    });
}

fn object_shadow(p: &mut Painter<'_>, pose: &Pose, f: &Frame, model: f32, center: V2, radius: f32) {
    let off = SHADOW_DIR * pose.lift;
    let c = f.at(center) + off;
    let blur = 0.035 * f.s + pose.lift * 0.28;
    p.prim(Prim {
        a: [c.x, c.y, c.x, c.y],
        b: [f.s, blur, radius * f.s + 2.5 * blur, kind::OBJECT_SHADOW],
        c: [f.tip.x + off.x, f.tip.y + off.y, f.down.x, f.down.y],
        d: [0.30 - 0.06 * clamp01(pose.press), pose.roll, model, 0.0],
        ..Default::default()
    });
}

/// A soft shadow under a straight prop, from `a` to `b` (radius `r`).
fn prop_shadow(p: &mut Painter<'_>, pose: &Pose, a: V2, b: V2, r: f32, alpha: f32) {
    let off = SHADOW_DIR * (pose.lift * 1.3);
    let blur = r + pose.lift * 0.3;
    let pts = [a, b].map(|q| RPoint { p: q + off, r: blur, color: rgba(0.0, 0.0, 0.0, alpha), along: 0.0 });
    p.ribbon(&pts, [ribbon::SHADOW, 0.0, 0.0, 0.0], true);
}

/// A straight ribbon from `a` to `b`, with round ends when `caps`.
fn rod(p: &mut Painter<'_>, (a, b): (V2, V2), r: f32, color: Rgba, style: f32, caps: bool) {
    let len = (b - a).len();
    let pts = [RPoint { p: a, r, color, along: 0.0 }, RPoint { p: b, r, color, along: len / (r * 20.0).max(1.0) }];
    p.ribbon(&pts, [style, 0.0, 0.0, 0.0], caps);
}

fn saber(p: &mut Painter<'_>, pose: &Pose, f: &Frame, look: &Look<'_>) {
    let s = f.s;
    let (emitter, full) = blade_root(pose);
    let end = emitter + f.down * (HILT * s);
    let lit = look.ignition;
    let [r, g, b] = look.blade;
    prop_shadow(p, pose, emitter, end, 0.08 * s, 0.22);
    if lit > 0.01 {
        let tip = emitter - f.down * (full * lit);
        // The blade lights the page around it.
        p.blade(emitter, tip, 0.0, 0.6 * s, rgba(r, g, b, 0.16), 0.0);
        // The swing's afterimage: a fan between where the blade was and where it is, fading
        // with age. The tip stays on the cursor while the hilt swings, so most of the sweep is
        // down the blade.
        let n = look.trail.len();
        let mut fan = Vec::with_capacity(n * 6);
        for (i, w) in look.trail.windows(2).enumerate() {
            let (a0, a1) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let c0 = rgba(r, g, b, 0.38 * a0 * a0);
            let c1 = rgba(r, g, b, 0.38 * a1 * a1);
            let (e0, t0) = w[0];
            let (e1, t1) = w[1];
            fan.extend_from_slice(&[(e0, with_alpha(c0, 0.3)), (t0, c0), (t1, c1), (e0, with_alpha(c0, 0.3)), (t1, c1), (e1, with_alpha(c1, 0.3))]);
        }
        let swept = look.trail.first().map_or(0.0, |(e, t)| (*e - emitter).len().max((*t - tip).len()));
        if swept > 4.0 { p.triangles(&fan); }
        let flicker = 0.5 + 0.5 * (look.time * 47.0).sin() * (look.time * 31.0).cos();
        let core = 0.03 * s * (1.0 + 0.35 * look.flash);
        p.blade(emitter, tip, core, 0.12 * s * (1.0 + 0.4 * look.flash), rgba(r, g, b, 0.92), flicker);
    }
    // The hilt: emitter shroud, grip, a chrome ring and the pommel.
    let at = |d: f32| emitter + f.down * (d * s);
    p.chrome_cap(at(-0.02), at(0.21), 0.09 * s, 0.0);
    rod(p, (at(0.21), at(0.33)), 0.068 * s, [0.0; 4], ribbon::RUBBER, false);
    p.chrome_cap(at(0.33), at(0.40), 0.075 * s, 0.3);
    rod(p, (at(0.40), at(0.82)), 0.072 * s, [0.0; 4], ribbon::RUBBER, false);
    p.chrome_cap(at(0.82), at(0.89), 0.078 * s, 0.0);
    p.chrome_cap(at(0.89), at(HILT), 0.07 * s, 0.6);
    // Its button glows the blade's colour.
    p.disc(at(0.28) + f.right * (0.05 * s), 0.022 * s, rgba(r, g, b, 1.0), 0.0);
    p.disc(at(0.28) + f.right * (0.05 * s), 0.045 * s, rgba(r, g, b, 0.35), 0.03 * s);
}

fn wand(p: &mut Painter<'_>, pose: &Pose, f: &Frame, look: &Look<'_>) {
    let s = f.s;
    let at = |d: f32| f.tip + f.down * (d * s);
    let len = WAND;
    prop_shadow(p, pose, at(0.0), at(len), 0.045 * s, 0.24);
    let white = rgba(0.97, 0.97, 0.95, 1.0);
    let black = rgba(0.05, 0.05, 0.06, 1.0);
    let radius = |d: f32| (0.033 + 0.012 * d / len) * s;
    let body: Vec<RPoint> = [0.3, len - 0.26].iter().map(|d| RPoint { p: at(*d), r: radius(*d), color: black, along: *d }).collect();
    p.ribbon(&body, [ribbon::GLOSS, 0.0, 0.0, 0.0], false);
    rod(p, (at(0.0), at(0.3)), radius(0.15), white, ribbon::GLOSS, true);
    rod(p, (at(len - 0.26), at(len)), radius(len - 0.13), white, ribbon::GLOSS, true);
    // The tip twinkles.
    let pulse = 0.5 + 0.5 * (look.time * 5.0).sin();
    p.disc(f.tip, 0.12 * s, rgba(1.0, 0.86, 0.45, 0.22 + 0.1 * pulse), 0.1 * s);
    p.twinkle(f.tip, (0.16 + 0.05 * pulse) * s, rgba(1.0, 0.9, 0.55, 0.95), look.time * 0.8);
}

fn mallet(p: &mut Painter<'_>, pose: &Pose, f: &Frame) {
    let s = f.s;
    let top = f.at(v2(0.5, 0.25));
    let end = f.at(v2(0.5, 0.25 + MALLET_HANDLE));
    prop_shadow(p, pose, top, end, 0.06 * s, 0.22);
    object_shadow(p, pose, f, 2.0, v2(0.5, 0.05), 0.62);
    rod(p, (top, end), 0.055 * s, [0.0; 4], ribbon::WOOD, false);
    rod(p, (f.at(v2(0.5, 0.2 + MALLET_HANDLE)), end), 0.07 * s, [0.0; 4], ribbon::WOOD, true);
    object(p, pose, f, 2.0, v2(0.5, 0.05), 0.62);
}

struct Curve {
    pts: Vec<V2>,
    cum: Vec<f32>,
}

impl Curve {
    fn new(pose: &Pose, n: usize) -> Curve {
        let pts: Vec<V2> = (0..=n).map(|i| pose.pole_at(i as f32 / n as f32)).collect();
        let mut cum = Vec::with_capacity(pts.len());
        let mut acc = 0.0;
        cum.push(0.0);
        for w in pts.windows(2) {
            acc += (w[1] - w[0]).len();
            cum.push(acc);
        }
        Curve { pts, cum }
    }
    fn total(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }
    /// The point `s` points along (clamped, extrapolated a little past the end).
    fn at(&self, s: f32) -> V2 {
        let total = self.total();
        if s >= total {
            let n = self.pts.len();
            let dir = (self.pts[n - 1] - self.pts[n - 2]).norm_or(v2(0.0, -1.0));
            return self.pts[n - 1] + dir * (s - total);
        }
        let s = s.max(0.0);
        let i = self.cum.partition_point(|c| *c <= s).clamp(1, self.pts.len() - 1);
        let (c0, c1) = (self.cum[i - 1], self.cum[i]);
        let t = if c1 > c0 { (s - c0) / (c1 - c0) } else { 0.0 };
        self.pts[i - 1].lerp(self.pts[i], t)
    }
    /// Points from `s0` to `s1`, at most `step` apart.
    fn span(&self, s0: f32, s1: f32, step: f32) -> Vec<(V2, f32)> {
        let n = (((s1 - s0) / step).ceil() as usize).max(1);
        (0..=n).map(|i| {
            let s = s0 + (s1 - s0) * i as f32 / n as f32;
            (self.at(s), s)
        }).collect()
    }
}

/// The pole's shadow, lifted further off the screen toward the base, fading out where the
/// model's own shadow takes over.
fn pole_shadow(p: &mut Painter<'_>, pose: &Pose, curve: &Curve, screen_h: f32) {
    let s = pose.size;
    let total = curve.total();
    let step = (8.0 * p.px).max(4.0);
    let grip = (1.35 * s).min(total * 0.45);
    let lift_tip = pose.lift + 0.05 * s;
    let lift_base = 1.6 * s;
    let shadow: Vec<RPoint> = curve.span(0.0, total, step * 2.0).into_iter().map(|(q, along)| {
        let t = along / total.max(1.0);
        let lift = lift_base + (lift_tip - lift_base) * t;
        let r = s * if along < grip { GRIP_R } else { 0.038 } + lift * 0.22;
        let fade = clamp01((total - along) / (0.45 * s)) * clamp01((screen_h + 0.2 * s - q.y) / (0.6 * s) + 0.6);
        RPoint { p: q + SHADOW_DIR * lift, r, color: rgba(0.0, 0.0, 0.0, 0.17 * fade), along }
    }).collect();
    p.ribbon(&shadow, [ribbon::SHADOW, 0.0, 0.0, 0.0], false);
}

/// The telescoping pole itself: grip, ferrule, sections and collars.
fn pole(p: &mut Painter<'_>, pose: &Pose, curve: &Curve) {
    let s = pose.size;
    let total = curve.total();
    let step = (8.0 * p.px).max(4.0);
    let grip = (1.35 * s).min(total * 0.45);
    let ferrule = grip + 0.11 * s;
    let tip_end = total + 0.06 * s; // a little into the model, so no seam shows
    let section = ((tip_end - ferrule) / SECTIONS.len() as f32).max(0.05 * s);
    let grip_pts: Vec<RPoint> = curve.span(0.0, grip, step).into_iter()
        .map(|(q, along)| RPoint { p: q, r: GRIP_R * s, color: [0.0; 4], along: along / s })
        .collect();
    p.ribbon(&grip_pts, [ribbon::RUBBER, 0.0, 0.0, 0.0], false);
    for (i, r) in SECTIONS.iter().enumerate() {
        let s0 = ferrule + section * i as f32 - if i == 0 { 0.02 * s } else { 0.0 };
        let s1 = (ferrule + section * (i + 1) as f32).min(tip_end);
        if s1 <= s0 { continue; }
        let pts: Vec<RPoint> = curve.span(s0, s1, step).into_iter()
            .map(|(q, along)| RPoint { p: q, r: r * s, color: [0.0; 4], along: along / s })
            .collect();
        p.ribbon(&pts, [ribbon::CHROME, i as f32, 0.0, 0.0], false);
    }
    // A collar where each thinner section slides out of the one before it.
    for i in 1..SECTIONS.len() {
        let at = ferrule + section * i as f32;
        if at > total { break; }
        let (a, b) = (curve.at(at - 0.05 * s), curve.at(at + 0.002 * s));
        p.chrome_cap(a, b, (SECTIONS[i - 1] + 0.008) * s, 0.0);
    }
    p.chrome_cap(curve.at(grip - 0.04 * s), curve.at(ferrule), FERRULE_R * s, 1.0);
}
