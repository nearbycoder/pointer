//! Effects on the glass: tap bursts, the charge dial, cracks, shockwaves, the laser trail,
//! ink, the spotlight and the focus box. Each effect is a pure function of its age, so a
//! headless run at a fixed clock draws exactly what the screen did.

use std::collections::VecDeque;
use std::f32::consts::TAU;

use crate::draw::{Dash, Painter, Prim, RPoint, Rgba, kind, rgba, ribbon, with_alpha};
use crate::math::{Rng, Spring, V2, clamp01, ease_out_cubic, ease_out_expo, smoothstep, v2};
use crate::style::Style;

pub const INK: Rgba = rgba(0.13, 0.13, 0.15, 1.0);
pub const GRAY: Rgba = rgba(0.30, 0.30, 0.33, 1.0);
pub const WHITE: Rgba = rgba(1.0, 1.0, 1.0, 1.0);
pub const LASER: Rgba = rgba(1.0, 0.13, 0.22, 1.0);
pub const PEN: Rgba = rgba(1.0, 0.18, 0.40, 1.0);

const TAP_LIFE: f64 = 1.0;
const SHOCK_LIFE: f64 = 0.9;
const CRACK_HOLD: f64 = 3.4;
const CRACK_FADE: f64 = 1.1;
const TRAIL_LIFE: f64 = 0.55;

/// A click's burst, drawn the way the pointer that made it clicks.
struct Tap {
    at: V2,
    t0: f64,
    seed: u64,
    k: f32,
    style: Style,
    /// The sword's blade colour (sRGB), for its clash.
    tint: [f32; 3],
}

impl Tap {
    fn life(&self) -> f64 {
        match self.style {
            Style::Glove => TAP_LIFE,
            Style::Saber => 2.0,
            Style::Wand => 1.3,
            Style::Mallet => 1.6,
            Style::Chicken => 2.0,
        }
    }
}

/// A twinkle shed by the moving wand.
struct Sparkle {
    at: V2,
    vel: V2,
    t0: f64,
    life: f32,
    size: f32,
    color: Rgba,
    spin: f32,
}

const SPARKLE_COLORS: [Rgba; 5] = [
    rgba(1.0, 0.85, 0.35, 1.0),
    rgba(1.0, 0.55, 0.8, 1.0),
    rgba(0.45, 0.85, 1.0, 1.0),
    rgba(0.8, 0.6, 1.0, 1.0),
    rgba(1.0, 1.0, 1.0, 1.0),
];

#[derive(Clone, Copy)]
struct CrackSeg {
    a: V2,
    b: V2,
    w: f32,
    d0: f32,
    d1: f32,
}

struct Crack {
    t0: f64,
    segs: Vec<CrackSeg>,
    reach: f32,
    /// When it starts to fade, and over how long.
    fade_at: f64,
    fade_len: f64,
}

struct Shock {
    at: V2,
    t0: f64,
    k: f32,
    power: f32,
    seed: u64,
}

pub struct Stroke {
    pub pts: Vec<V2>,
    pub color: Rgba,
    pub width: f32,
    pub fade_at: Option<f64>,
}

pub struct FocusBox {
    pub a: V2,
    pub b: V2,
    pub dragging: bool,
}

/// What a press did, for the stage to react to.
pub enum Outcome {
    Nothing,
    Cracked { power: f32 },
}

pub struct Fx {
    taps: Vec<Tap>,
    sparkles: Vec<Sparkle>,
    /// Fractional sparkles owed from frames too short to shed a whole one.
    sparkle_owed: f32,
    cracks: Vec<Crack>,
    shocks: Vec<Shock>,
    pub charge: f32,
    dial: Spring,
    last_click: f64,
    spent: bool,
    trail: VecDeque<(V2, f64)>,
    pub strokes: Vec<Stroke>,
    pub drawing: Option<Stroke>,
    pub focus: Option<FocusBox>,
    focus_amt: Spring,
    focus_fading: bool,
    spot_amt: Spring,
    seed: u64,
    now: f64,
}

impl Default for Fx {
    fn default() -> Self {
        Fx {
            taps: Vec::new(),
            sparkles: Vec::new(),
            sparkle_owed: 0.0,
            cracks: Vec::new(),
            shocks: Vec::new(),
            charge: 0.0,
            dial: Spring::default(),
            last_click: -10.0,
            spent: false,
            trail: VecDeque::new(),
            strokes: Vec::new(),
            drawing: None,
            focus: None,
            focus_amt: Spring::default(),
            focus_fading: false,
            spot_amt: Spring::default(),
            seed: 0x5eed,
            now: 0.0,
        }
    }
}

impl Fx {
    fn next_seed(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.seed
    }

    /// A press at `at` with `style`'s pointer. Rapid presses charge the glass; the fifth in a
    /// row cracks it.
    pub fn click(&mut self, at: V2, now: f64, k: f32, style: Style, tint: [f32; 3]) -> Outcome {
        let seed = self.next_seed();
        self.taps.push(Tap { at, t0: now, seed, k, style, tint });
        self.spent = false;
        let quick = now - self.last_click < 0.55;
        self.last_click = now;
        self.charge = if quick { self.charge + 0.22 } else { 0.22_f32.max(self.charge) };
        if self.charge >= 0.999 {
            let power = 0.7 + 0.3 * Rng::new(seed).f();
            self.crack(at, now, k, power);
            return Outcome::Cracked { power };
        }
        Outcome::Nothing
    }

    /// The button is still down, `held` seconds after the press, having moved `travel`
    /// points. Holding still charges the dial; full charge cracks.
    pub fn hold(&mut self, at: V2, held: f64, travel: f32, dt: f32, now: f64, k: f32) -> Outcome {
        if self.spent || travel > 10.0 || held < 0.32 { return Outcome::Nothing; }
        self.charge += dt * 0.85;
        self.last_click = now;
        if self.charge >= 1.0 {
            self.spent = true;
            self.crack(at, now, k, 1.0);
            return Outcome::Cracked { power: 1.0 };
        }
        Outcome::Nothing
    }

    /// Breaks the glass at `at` with `power` in 0..=1.
    pub fn crack(&mut self, at: V2, now: f64, k: f32, power: f32) {
        let seed = self.next_seed();
        let (segs, reach) = crack_pattern(at, k, power, seed);
        self.cracks.push(Crack { t0: now, segs, reach, fade_at: now + CRACK_HOLD, fade_len: CRACK_FADE });
        self.shocks.push(Shock { at, t0: now, k, power, seed });
        self.charge = 0.0;
        if self.cracks.len() > 6 { self.cracks.remove(0); }
    }

    /// Fades out cracks, ink and the focus box.
    pub fn clear(&mut self, now: f64) {
        for c in &mut self.cracks {
            if c.fade_at > now { c.fade_at = now; c.fade_len = 0.35; }
        }
        for s in &mut self.strokes {
            if s.fade_at.is_none_or(|t| t > now) { s.fade_at = Some(now); }
        }
        self.focus_fading = true;
        self.charge = 0.0;
    }

    /// Ink fades when its tool is put away.
    pub fn fade_ink(&mut self, now: f64) {
        for s in &mut self.strokes {
            if s.fade_at.is_none() { s.fade_at = Some(now + 0.15); }
        }
        if let Some(s) = self.drawing.take() { self.strokes.push(s); }
    }

    /// The wand sheds sparkles from `tip`, more the faster it moves (`vel` in points/s).
    pub fn sparkle(&mut self, tip: V2, vel: V2, now: f64, dt: f32, k: f32) {
        let speed = vel.len();
        self.sparkle_owed += (speed / (14.0 * k.max(0.1))).min(90.0) * dt;
        let mut rng = Rng::new((now * 7919.0) as u64 ^ self.seed);
        while self.sparkle_owed >= 1.0 && self.sparkles.len() < 160 {
            self.sparkle_owed -= 1.0;
            let drift = v2(rng.signed(), rng.signed()) * (40.0 * k) - vel * 0.08;
            self.sparkles.push(Sparkle {
                at: tip + v2(rng.signed(), rng.signed()) * (6.0 * k),
                vel: drift,
                t0: now,
                life: rng.range(0.5, 0.95),
                size: rng.range(7.0, 14.0) * k,
                color: SPARKLE_COLORS[(rng.f() * 5.0) as usize % 5],
                spin: rng.signed() * 3.0,
            });
        }
        self.sparkle_owed = self.sparkle_owed.min(4.0);
    }

    pub fn trail_point(&mut self, p: V2, now: f64) {
        if self.trail.back().is_none_or(|(q, _)| (*q - p).len() > 0.6) { self.trail.push_back((p, now)); }
    }

    pub fn begin_stroke(&mut self, p: V2, color: Rgba, width: f32) {
        if let Some(s) = self.drawing.take() { self.strokes.push(s); }
        self.drawing = Some(Stroke { pts: vec![p], color, width, fade_at: None });
    }
    pub fn extend_stroke(&mut self, p: V2) {
        if let Some(s) = &mut self.drawing
            && s.pts.last().is_none_or(|q| (*q - p).len() > 1.2)
        {
            s.pts.push(p);
        }
    }
    pub fn end_stroke(&mut self) {
        if let Some(mut s) = self.drawing.take() {
            if s.pts.len() == 1 { let p = s.pts[0]; s.pts.push(p + v2(0.01, 0.0)); }
            self.strokes.push(s);
        }
    }

    pub fn begin_focus(&mut self, p: V2) {
        self.focus = Some(FocusBox { a: p, b: p, dragging: true });
        self.focus_fading = false;
    }
    pub fn drag_focus(&mut self, p: V2) {
        if let Some(f) = &mut self.focus && f.dragging { f.b = p; }
    }
    /// Lets go of the box; true when it's big enough to keep.
    pub fn end_focus(&mut self) -> bool {
        let Some(f) = &mut self.focus else { return false };
        f.dragging = false;
        let kept = (f.b.x - f.a.x).abs() >= 12.0 && (f.b.y - f.a.y).abs() >= 12.0;
        if !kept { self.focus_fading = true; }
        kept
    }

    /// The charge level while the dial shows, else 0 (drives the charge whine).
    pub fn dial_charge(&self) -> f32 {
        if self.charge > 0.3 || self.dial.x > 0.05 { self.charge } else { 0.0 }
    }
    pub fn fade_focus(&mut self) {
        self.focus_fading = true;
    }

    /// Steps springs and drops what has finished. `spot` is whether the spotlight is on.
    pub fn step(&mut self, dt: f32, now: f64, spot: bool, holding: bool) {
        self.now = now;
        if !holding && now - self.last_click > 0.6 { self.charge = (self.charge - dt * 1.3).max(0.0); }
        let dial_on = if self.charge > 0.3 { 1.0 } else { 0.0 };
        let focus_on = if self.focus.is_some() && !self.focus_fading { 1.0 } else { 0.0 };
        let steps = ((dt / (1.0 / 480.0)).ceil() as usize).clamp(1, 60);
        let h = dt / steps as f32;
        for _ in 0..steps {
            self.dial.step(dial_on, 22.0, 0.7, h);
            self.focus_amt.step(focus_on, 16.0, 1.0, h);
            self.spot_amt.step(if spot { 1.0 } else { 0.0 }, 14.0, 1.0, h);
        }
        self.dial.x = self.dial.x.max(0.0);
        self.focus_amt.x = clamp01(self.focus_amt.x);
        self.spot_amt.x = clamp01(self.spot_amt.x);
        if self.focus_fading && self.focus_amt.x < 0.003 { self.focus = None; }
        self.taps.retain(|t| now - t.t0 < t.life());
        self.sparkles.retain(|s| ((now - s.t0) as f32) < s.life);
        self.shocks.retain(|s| now - s.t0 < SHOCK_LIFE);
        self.cracks.retain(|c| now < c.fade_at + c.fade_len);
        self.strokes.retain(|s| s.fade_at.is_none_or(|t| now < t + 0.6));
        while self.trail.front().is_some_and(|(_, t)| now - t > TRAIL_LIFE) { self.trail.pop_front(); }
    }

    /// Whether anything still moves on its own (so frames keep coming).
    pub fn animating(&self) -> bool {
        !self.taps.is_empty() || !self.sparkles.is_empty() || !self.shocks.is_empty() || !self.trail.is_empty() || self.charge > 0.0
            || self.dial.x > 0.002 || self.strokes.iter().any(|s| s.fade_at.is_some())
            || self.cracks.iter().any(|c| self.now < c.t0 + 0.4 || self.now >= c.fade_at - 0.05)
            || (self.focus_amt.x > 0.002 && self.focus_amt.x < 0.998)
            || (self.spot_amt.x > 0.002 && self.spot_amt.x < 0.998)
            || self.focus.as_ref().is_some_and(|f| f.dragging) || self.drawing.is_some()
    }

    /// The dimming layer: the spotlight's hole around `tip` and the focus box's.
    pub fn draw_dim(&self, p: &mut Painter<'_>, w: f32, h: f32, tip: V2, spot_r: f32) {
        let spot = self.spot_amt.x;
        let focus = self.focus_amt.x;
        if spot < 0.002 && focus < 0.002 { return; }
        let (a, b) = self.focus.as_ref().map_or((V2::ZERO, V2::ZERO), |f| (f.a, f.b));
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let grow = 8.0 * (1.0 - focus);
        p.prim(Prim {
            a: [0.0, h * 0.5, w, h * 0.5],
            b: [0.0, 0.0, h * 0.5 + 4.0, kind::DIM],
            c: [tip.x, tip.y, spot_r * (0.85 + 0.15 * ease_out_cubic(spot)), spot],
            d: [x0 - grow, y0 - grow, x1 + grow, y1 + grow],
            e: [focus, 0.62, 14.0, if self.focus.as_ref().is_some_and(|f| f.dragging) { 1.0 } else { 0.85 }],
        });
    }

    /// Ink, cracks, taps, shocks and the trail: everything on the glass under the pointer.
    pub fn draw_glass(&self, p: &mut Painter<'_>, now: f64, k: f32) {
        for s in self.strokes.iter().chain(self.drawing.iter()) {
            let a = s.fade_at.map_or(1.0, |t| 1.0 - clamp01(((now - t) / 0.55) as f32));
            if a <= 0.0 { continue; }
            let pts: Vec<RPoint> = s.pts.iter().map(|q| RPoint { p: *q, r: s.width, color: with_alpha(s.color, a), along: 0.0 }).collect();
            p.ribbon(&pts, [ribbon::SOLID, 0.0, 0.0, 0.0], true);
        }
        for c in &self.cracks { draw_crack(p, c, now); }
        for t in &self.taps {
            match t.style {
                Style::Glove => draw_tap(p, t, now),
                Style::Saber => draw_clash(p, t, now),
                Style::Wand => draw_cast(p, t, now),
                Style::Mallet => draw_bonk(p, t, now),
                Style::Chicken => draw_squawk_ring(p, t, now),
            }
        }
        for s in &self.sparkles {
            let age = (now - s.t0) as f32;
            let life = 1.0 - age / s.life;
            if life <= 0.0 { continue; }
            let drag = (1.0 - (-age * 2.5).exp()) / 2.5;
            let at = s.at + s.vel * drag + v2(0.0, 30.0 * s.size / 8.0 * age * age);
            let twinkle = 0.65 + 0.35 * (age * 22.0 + s.spin * 5.0).sin();
            p.twinkle(at, s.size * life.sqrt() * twinkle, with_alpha(s.color, life.sqrt()), s.spin * age);
        }
        for s in &self.shocks { draw_shock(p, s, now); }
        if self.trail.len() >= 2 {
            let pts: Vec<RPoint> = self.trail.iter().map(|(q, t)| {
                let life = 1.0 - clamp01(((now - t) / TRAIL_LIFE) as f32);
                RPoint { p: *q, r: 9.0 * k * (0.35 + 0.65 * life), color: with_alpha(LASER, life.powf(0.35)), along: 0.0 }
            }).collect();
            p.ribbon(&pts, [ribbon::GLOW, 0.3, 0.0, 0.0], true);
        }
    }

    /// What flies in front of the pointer: the mallet's dizzy stars and the chicken's feathers.
    pub fn draw_above(&self, p: &mut Painter<'_>, now: f64) {
        for t in &self.taps {
            match t.style {
                Style::Mallet => draw_dizzy(p, t, now),
                Style::Chicken => draw_feathers(p, t, now),
                _ => {}
            }
        }
    }

    /// The charge dial around the fingertip.
    pub fn draw_dial(&self, p: &mut Painter<'_>, at: V2, k: f32, now: f64) {
        let v = self.dial.x.min(1.2);
        if v <= 0.002 { return; }
        let level = clamp01(self.charge);
        let mut rng = Rng::new((now * 60.0) as u64);
        let shake = v2(rng.signed(), rng.signed()) * (level * level * 1.6 * k);
        let c = at + shake;
        let grow = 0.6 + 0.4 * v;
        p.disc(c, 34.0 * k * grow, with_alpha(WHITE, 0.62 * v.min(1.0)), 1.5);
        p.ring(c, 34.0 * k * grow, 1.4, with_alpha(GRAY, 0.55 * v.min(1.0)));
        p.ring(c, 21.0 * k * grow, 1.2, with_alpha(GRAY, 0.45 * v.min(1.0)));
        let spin = (now as f32) * (0.6 + 4.0 * level);
        p.ring_dashed(c, 60.0 * k * grow, 1.3, with_alpha(GRAY, 0.38 * v.min(1.0)), Dash { count: 40.0, duty: 0.45, phase: spin });
        let lit = (level * 12.0).floor() as usize;
        for i in 0..12 {
            let ang = -std::f32::consts::FRAC_PI_2 + i as f32 * TAU / 12.0;
            let dir = V2::from_angle(ang);
            let on = i < lit;
            let pulse = if on { 1.0 + 0.18 * ((now as f32) * 22.0 + i as f32).sin() * level } else { 1.0 };
            let r0 = 39.0 * k * grow;
            let len = 11.0 * k * pulse;
            let fill = if on { with_alpha(INK, 0.92 * v.min(1.0)) } else { with_alpha(WHITE, 0.9 * v.min(1.0)) };
            p.pill(c + dir * r0, c + dir * (r0 + len), 3.2 * k, fill, with_alpha(INK, 0.8 * v.min(1.0)), 1.2);
        }
        if level > 0.45 {
            for j in 0..3 {
                let ang = spin * 2.5 + j as f32 * TAU / 3.0;
                p.disc(c + V2::from_angle(ang) * (50.0 * k), 2.2 * k, with_alpha(INK, 0.7 * (level - 0.45) * 2.0), 0.0);
            }
        }
    }
}

fn draw_tap(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let mut rng = Rng::new(t.seed);
    let rot = rng.f() * TAU;
    // A soft white disc under the burst.
    let e = ease_out_cubic(age / 0.4);
    p.disc(t.at, 26.0 * k * (0.55 + 0.45 * e), with_alpha(WHITE, 0.42 * (1.0 - smoothstep(0.05, 0.4, age))), 6.0 * k);
    // Pill ticks bursting outward.
    let tick_a = 1.0 - smoothstep(0.16, 0.42, age);
    if tick_a > 0.0 {
        for i in 0..10 {
            let dir = V2::from_angle(rot + i as f32 * TAU / 10.0);
            let r0 = (15.0 + 30.0 * e) * k;
            let len = (13.0 - 6.0 * e) * k;
            p.pill(t.at + dir * r0, t.at + dir * (r0 + len), 3.3 * k, with_alpha(WHITE, 0.92 * tick_a), with_alpha(INK, 0.85 * tick_a), 1.25);
        }
    }
    // An inner ring and two wider ripples.
    let e2 = ease_out_expo(age / 0.5);
    p.ring(t.at, (11.0 + 20.0 * e2) * k, 1.6, with_alpha(GRAY, 0.75 * (1.0 - smoothstep(0.08, 0.5, age))));
    for j in 0..2 {
        let delay = 0.05 + 0.11 * j as f32;
        let a = age - delay;
        if a <= 0.0 { continue; }
        let e3 = ease_out_cubic(a / 0.8);
        let fade = (1.0 - e3).powf(1.3);
        p.ring(t.at, (22.0 + (85.0 + 45.0 * j as f32) * e3) * k, 1.2, with_alpha(GRAY, 0.55 * fade));
    }
    // Specks drifting out.
    let e4 = ease_out_cubic(age / 0.75);
    let speck = 1.0 - smoothstep(0.3, 0.8, age);
    for _ in 0..9 {
        let dir = V2::from_angle(rng.f() * TAU);
        let dist = (18.0 + rng.range(40.0, 110.0) * e4) * k;
        let r = rng.range(1.4, 2.8) * k;
        p.disc(t.at + dir * dist, r, with_alpha(GRAY, 0.75 * speck), 0.0);
    }
}

/// The energy sword striking the glass: a flash, sparks spraying and falling, a scorch mark.
fn draw_clash(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let [r, g, b] = t.tint;
    let mut rng = Rng::new(t.seed);
    // The scorch: a dark mark under an ember that cools.
    let fade = 1.0 - smoothstep(1.2, 2.0, age);
    p.disc(t.at, 9.0 * k, rgba(0.12, 0.08, 0.06, 0.32 * fade), 5.0 * k);
    let ember = (-age / 0.45).exp();
    p.disc(t.at, 9.0 * k, rgba(1.0, 0.55, 0.15, 0.5 * ember), 6.0 * k);
    p.disc(t.at, 3.2 * k, rgba(1.0, 0.9, 0.6, ember), 0.0);
    // The flash, in the blade's colour.
    let e = ease_out_expo(age / 0.25);
    p.disc(t.at, (20.0 + 90.0 * e) * k, rgba(r, g, b, 0.55 * (1.0 - smoothstep(0.0, 0.22, age))), 26.0 * k);
    p.disc(t.at, (8.0 + 20.0 * e) * k, rgba(1.0, 1.0, 1.0, 0.8 * (1.0 - smoothstep(0.0, 0.12, age))), 8.0 * k);
    p.ring(t.at, (12.0 + 110.0 * e) * k, 2.4, rgba(r, g, b, 0.7 * (1.0 - e)));
    // Sparks: hot streaks thrown out and pulled down.
    for _ in 0..26 {
        let ang = -std::f32::consts::FRAC_PI_2 + rng.signed() * 2.3;
        let speed = rng.range(300.0, 850.0) * k;
        let life = rng.range(0.35, 0.8);
        if age > life { continue; }
        let pos = |a: f32| t.at + V2::from_angle(ang) * (speed * a) + v2(0.0, 1000.0 * k * a * a);
        let head = pos(age);
        let tail = pos((age - 0.05).max(0.0));
        let heat = 1.0 - age / life;
        let pts = [
            RPoint { p: tail, r: 2.0 * k, color: rgba(1.0, 0.6, 0.2, 0.0), along: 0.0 },
            RPoint { p: head, r: 2.6 * k, color: rgba(1.0, 0.85 + 0.15 * heat, 0.45 + 0.4 * heat, heat), along: 0.0 },
        ];
        p.ribbon(&pts, [ribbon::GLOW, 0.45, 0.0, 0.0], true);
    }
}

/// The wand casting: a puff, a ring and a burst of twinkling stars.
fn draw_cast(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let mut rng = Rng::new(t.seed);
    let e = ease_out_cubic(age / 0.5);
    p.disc(t.at, (10.0 + 40.0 * e) * k, with_alpha(WHITE, 0.5 * (1.0 - smoothstep(0.0, 0.4, age))), 12.0 * k);
    p.ring_dashed(t.at, (12.0 + 80.0 * e) * k, 1.8, rgba(1.0, 0.82, 0.35, 0.8 * (1.0 - e)), Dash { count: 24.0, duty: 0.5, phase: age * 2.0 });
    for i in 0..14 {
        let ang = i as f32 / 14.0 * TAU + rng.signed() * 0.3;
        let speed = rng.range(220.0, 520.0) * k;
        let life = rng.range(0.75, 1.2);
        if age > life { continue; }
        let drag = (1.0 - (-age / 0.28).exp()) * 0.28;
        let at = t.at + V2::from_angle(ang) * (speed * drag) + v2(0.0, 60.0 * k * age * age);
        let fade = 1.0 - age / life;
        let twinkle = 0.7 + 0.3 * (age * 18.0 + i as f32).sin();
        let size = rng.range(13.0, 24.0) * k * fade.sqrt() * twinkle;
        p.twinkle(at, size, with_alpha(SPARKLE_COLORS[i % 5], fade.sqrt()), age * 2.0 + i as f32);
    }
}

/// The mallet landing: impact lines, a dust puff, a ring, and stars circling the dazed spot.
fn draw_bonk(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let mut rng = Rng::new(t.seed);
    let e = ease_out_cubic(age / 0.3);
    let lines = 1.0 - smoothstep(0.12, 0.32, age);
    for i in 0..10 {
        let dir = V2::from_angle(i as f32 / 10.0 * TAU + rng.signed() * 0.15);
        let r0 = (42.0 + 40.0 * e) * k;
        p.pill(t.at + dir * r0, t.at + dir * (r0 + 28.0 * k * (1.0 - 0.4 * e)), 3.6 * k, with_alpha(INK, 0.9 * lines), [0.0; 4], 0.0);
    }
    p.ring(t.at, (20.0 + 95.0 * ease_out_cubic(age / 0.4)) * k, 5.0 * (1.0 - e) + 1.2, with_alpha(INK, 0.55 * (1.0 - smoothstep(0.1, 0.4, age))));
    for i in 0..7 {
        let dir = V2::from_angle(i as f32 / 7.0 * TAU + rng.signed() * 0.5);
        let puff = ease_out_cubic(age / 0.6);
        p.disc(t.at + dir * ((14.0 + 50.0 * puff) * k), (10.0 + 10.0 * puff) * k, rgba(0.55, 0.52, 0.48, 0.32 * (1.0 - puff)), 7.0 * k);
    }
}

/// Dazed: stars orbiting a flattened circle over the spot the mallet hit.
fn draw_dizzy(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let show = smoothstep(0.05, 0.15, age) * (1.0 - smoothstep(1.25, 1.6, age));
    if show <= 0.0 { return; }
    for i in 0..5 {
        let ang = age * 5.5 + i as f32 * TAU / 5.0;
        let c = t.at + v2(ang.cos() * 72.0 * k - 10.0 * k, -58.0 * k + ang.sin() * 20.0 * k);
        let depth = 0.75 + 0.25 * ang.sin();
        p.cartoon_star(c, 15.0 * k * depth, rgba(1.0, 0.86, 0.2, show), rgba(0.25, 0.18, 0.05, 1.0), ang * 0.5);
    }
}

/// The ring under a squawk; the feathers fly in front of the chicken.
fn draw_squawk_ring(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    p.ring(t.at, (12.0 + 60.0 * ease_out_cubic(age / 0.4)) * t.k, 1.6, with_alpha(GRAY, 0.45 * (1.0 - smoothstep(0.05, 0.4, age))));
}

/// The chicken squawking: feathers fly up and drift down, swaying.
fn draw_feathers(p: &mut Painter<'_>, t: &Tap, now: f64) {
    let age = (now - t.t0) as f32;
    if age < 0.0 { return; }
    let k = t.k;
    let mut rng = Rng::new(t.seed);
    for _ in 0..10 {
        let ang = -std::f32::consts::FRAC_PI_2 + rng.signed() * 1.4;
        let speed = rng.range(220.0, 460.0) * k;
        let phase = rng.f() * TAU;
        let life = rng.range(1.4, 2.0);
        if age > life { continue; }
        // Thrown up, slowed by the air, then drifting down at a gentle terminal speed.
        let tau = 0.25;
        let up = V2::from_angle(ang) * (speed * tau * (1.0 - (-age / tau).exp()));
        let fall = v2((age * 5.0 + phase).sin() * 12.0 * k, 45.0 * k * (age - tau * (1.0 - (-age / tau).exp())));
        let c = t.at + up + fall;
        let tilt = V2::from_angle(0.6 * (age * 5.0 + phase).cos() + phase);
        let fade = 1.0 - smoothstep(life - 0.5, life, age);
        let tone = if rng.f() < 0.5 { rgba(1.0, 0.97, 0.88, fade) } else { rgba(1.0, 0.9, 0.55, fade) };
        p.pill(c - tilt * (11.0 * k), c + tilt * (11.0 * k), 5.5 * k, tone, rgba(0.62, 0.58, 0.48, 0.9 * fade), 1.2);
        p.pill(c - tilt * (10.0 * k), c + tilt * (14.0 * k), 0.7 * k, rgba(0.62, 0.58, 0.48, 0.8 * fade), [0.0; 4], 0.0);
    }
}

fn draw_shock(p: &mut Painter<'_>, s: &Shock, now: f64) {
    let age = (now - s.t0) as f32;
    if age < 0.0 { return; }
    let k = s.k;
    let e = ease_out_expo(age / 0.22);
    p.disc(s.at, (30.0 + 150.0 * e) * k, with_alpha(WHITE, 0.6 * (1.0 - smoothstep(0.0, 0.22, age))), 40.0 * k);
    let e2 = ease_out_cubic(age / 0.6);
    p.ring(s.at, (24.0 + 380.0 * (0.6 + 0.4 * s.power) * e2) * k, 5.0 * (1.0 - e2) + 1.0, with_alpha(GRAY, 0.5 * (1.0 - e2)));
    let e3 = ease_out_cubic((age - 0.07).max(0.0) / 0.7);
    p.ring(s.at, (24.0 + 260.0 * e3) * k, 1.2, with_alpha(GRAY, 0.4 * (1.0 - e3)));
    let mut rng = Rng::new(s.seed ^ 0xabcdef);
    let n = 10 + (s.power * 10.0) as usize;
    let fade = 1.0 - smoothstep(0.35, 0.85, age);
    for _ in 0..n {
        let ang = rng.f() * TAU;
        let speed = rng.range(280.0, 720.0) * k;
        let tau = 0.22;
        let dist = speed * tau * (1.0 - (-age / tau).exp()) + 10.0 * k;
        let dir = V2::from_angle(ang);
        let len = rng.range(5.0, 13.0) * k;
        let tilt = V2::from_angle(ang + rng.signed() * 0.9 + age * rng.signed() * 8.0);
        let c = s.at + dir * dist;
        p.shard(c - tilt * (len * 0.5), c + tilt * (len * 0.5), rng.range(1.5, 2.6) * k, with_alpha(INK, 0.85 * fade));
    }
}

fn draw_crack(p: &mut Painter<'_>, c: &Crack, now: f64) {
    let age = (now - c.t0) as f32;
    if age < 0.0 { return; }
    let grow = c.reach * 1.6 * ease_out_cubic(age / 0.26);
    let alpha = if now < c.fade_at { 1.0 } else { 1.0 - clamp01(((now - c.fade_at) / c.fade_len) as f32) };
    if alpha <= 0.0 { return; }
    let dark = rgba(0.09, 0.09, 0.11, 0.9);
    let glint = rgba(1.0, 1.0, 1.0, 0.7);
    for s in &c.segs {
        if s.d0 >= grow { continue; }
        let b = if s.d1 > grow { s.a + (s.b - s.a) * ((grow - s.d0) / (s.d1 - s.d0)) } else { s.b };
        p.crack(s.a, b, s.w, alpha, dark, glint);
    }
}

/// A spider-web fracture: jagged spokes with branches, a few rings of chords between them
/// and a shattered core.
fn crack_pattern(at: V2, k: f32, power: f32, seed: u64) -> (Vec<CrackSeg>, f32) {
    let mut rng = Rng::new(seed);
    let mut segs = Vec::new();
    let n = (7.0 + power * 8.0).round() as usize;
    let reach = k * (230.0 + 270.0 * power);
    let a0 = rng.f() * TAU;
    let w0 = (2.6 * k).clamp(1.6, 3.6);
    let mut spokes: Vec<Vec<V2>> = Vec::with_capacity(n);
    for i in 0..n {
        let ang = a0 + i as f32 * TAU / n as f32 + rng.signed() * 0.3 * TAU / n as f32;
        let len = reach * rng.range(0.5, 1.15);
        let start = at + V2::from_angle(ang) * (k * rng.range(2.0, 8.0));
        let mut pts = vec![start];
        walk(&mut rng, &mut segs, &mut pts, at, start, ang, len, w0, (start - at).len(), 0, k);
        spokes.push(pts);
    }
    let rings = 1 + (power * 2.2).round() as usize;
    for (ri, frac) in [0.2_f32, 0.43, 0.7].iter().take(rings).enumerate() {
        let rr = reach * frac * rng.range(0.85, 1.1);
        let w = (w0 * (0.75 - 0.15 * ri as f32)).max(1.1);
        for i in 0..n {
            if rng.f() < 0.2 { continue; }
            let a = near_radius(&spokes[i], at, rr * rng.range(0.88, 1.12));
            let b = near_radius(&spokes[(i + 1) % n], at, rr * rng.range(0.88, 1.12));
            let (Some(a), Some(b)) = (a, b) else { continue };
            let mid = at + ((a + b) * 0.5 - at) * rng.range(0.9, 0.98) + v2(rng.signed(), rng.signed()) * (3.0 * k);
            let d = (a - at).len().min((b - at).len());
            segs.push(CrackSeg { a, b: mid, w, d0: d, d1: d + (mid - a).len() });
            segs.push(CrackSeg { a: mid, b, w, d0: d + (mid - a).len(), d1: d + (mid - a).len() + (b - mid).len() });
        }
    }
    for _ in 0..(8 + (power * 12.0) as usize) {
        let ang = rng.f() * TAU;
        let r0 = rng.range(0.0, 13.0) * k;
        let l = rng.range(5.0, 16.0) * k;
        let a = at + V2::from_angle(ang) * r0;
        let b = a + V2::from_angle(ang + rng.signed() * 0.9) * l;
        segs.push(CrackSeg { a, b, w: (1.3 * k).max(1.0), d0: r0, d1: r0 + l });
    }
    (segs, reach)
}

#[allow(clippy::too_many_arguments)]
fn walk(rng: &mut Rng, segs: &mut Vec<CrackSeg>, pts: &mut Vec<V2>, at: V2, mut p: V2, mut ang: f32, len: f32, mut w: f32, mut dist: f32, depth: u32, k: f32) {
    let mut left = len;
    while left > 1.0 {
        let step = (k * rng.range(16.0, 34.0)).min(left.max(2.0));
        ang += rng.signed() * 0.24;
        let q = p + V2::from_angle(ang) * step;
        segs.push(CrackSeg { a: p, b: q, w, d0: dist, d1: dist + step });
        pts.push(q);
        if depth < 2 && rng.f() < 0.13 {
            let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
            let mut branch = Vec::new();
            let blen = left * rng.range(0.25, 0.55);
            let bang = ang + side * rng.range(0.35, 0.85);
            walk(rng, segs, &mut branch, at, q, bang, blen, w * 0.72, dist + step, depth + 1, k);
        }
        p = q;
        dist += step;
        left -= step;
        w = (w * 0.94).max(1.0);
    }
    let _ = at;
}

fn near_radius(pts: &[V2], at: V2, r: f32) -> Option<V2> {
    pts.windows(2).find_map(|w| {
        let (ra, rb) = ((w[0] - at).len(), (w[1] - at).len());
        if (ra - r) * (rb - r) <= 0.0 && (rb - ra).abs() > 1e-3 {
            Some(w[0] + (w[1] - w[0]) * ((r - ra) / (rb - ra)))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_quick_clicks_crack_the_glass() {
        let mut fx = Fx::default();
        let mut cracked = false;
        for i in 0..5 {
            if let Outcome::Cracked { .. } = fx.click(v2(100.0, 100.0), i as f64 * 0.2, 1.0, Style::Glove, [1.0; 3]) { cracked = true; }
        }
        assert!(cracked);
        assert_eq!(fx.cracks.len(), 1);
    }

    #[test]
    fn slow_clicks_never_crack() {
        let mut fx = Fx::default();
        for i in 0..10 {
            fx.step(0.016, i as f64, false, false);
            assert!(matches!(fx.click(v2(100.0, 100.0), i as f64, 1.0, Style::Glove, [1.0; 3]), Outcome::Nothing));
        }
    }

    #[test]
    fn holding_still_charges_then_cracks_once() {
        let mut fx = Fx::default();
        fx.click(v2(0.0, 0.0), 0.0, 1.0, Style::Mallet, [1.0; 3]);
        let mut cracks = 0;
        let mut t = 0.0;
        for _ in 0..600 {
            t += 1.0 / 120.0;
            if let Outcome::Cracked { .. } = fx.hold(v2(0.0, 0.0), t, 0.0, 1.0 / 120.0, t, 1.0) { cracks += 1; }
        }
        assert_eq!(cracks, 1);
    }

    #[test]
    fn a_crack_has_spokes_rings_and_a_core() {
        let (segs, reach) = crack_pattern(v2(500.0, 500.0), 1.0, 1.0, 7);
        assert!(segs.len() > 60, "{} segments", segs.len());
        assert!(reach > 400.0);
        assert!(segs.iter().all(|s| s.d1 >= s.d0 && s.w > 0.0));
    }
}
