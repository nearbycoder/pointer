//! Sound effects, synthesized as they play, so there are no audio files. Each effect is a
//! small voice (oscillators, noise, a filter or two and an envelope); three continuous voices
//! (the charge whine, the laser hum, the pen's scribble) follow values the stage sets every
//! frame. The mix goes out through a CoreAudio output unit with about 10 ms of latency, so a tap
//! sounds when it lands. The unit stops after a few quiet seconds so an idle pointer costs nothing.
//!
//! Headless runs don't make sound. With `POINTER_AUDIO=out.wav` they log every effect
//! against the virtual clock instead, and render the same synth into a WAV afterwards, so a
//! demo video gets a soundtrack in sync with its frames.

use std::f32::consts::{PI, TAU};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const RATE: u32 = 48_000;
const DT: f32 = 1.0 / RATE as f32;
const MAX_VOICES: usize = 48;
/// The mix's level, leaving headroom for a crack landing on top of everything else.
const MASTER: f32 = 0.78;

/// One-shot effects. `pan` is -1 (left) to 1 (right).
#[derive(Clone, Copy, Debug)]
pub enum Sfx {
    /// A click on the glass: a soft pop with a little tick.
    Tap { pan: f32, seed: u32 },
    /// One more tick lit on the charge dial; `level` 0..1 raises its pitch.
    Tick { pan: f32, level: f32 },
    /// The glass breaking: a thump, a snap, crackle running outward, tinkling shards, a whoosh.
    Crack { pan: f32, power: f32, seed: u32 },
    /// The pole sliding out (a rising rush and a click per section) or back in.
    Extend { gain: f32 },
    Retract { gain: f32 },
    /// A two-note chime for a tool change; `pitch` scales it.
    Chime { pitch: f32 },
    /// A stage light switching on.
    Spot,
    /// A focus box landing.
    Focus { pan: f32 },
    /// Everything swept away.
    Clear,
    /// The glove growing or shrinking.
    Resize { bigger: bool },
    /// The pen touching down.
    PenDown { pan: f32 },
    /// The energy sword lighting (a snap, then a hum rising out of the hilt) and going out.
    Ignite,
    Unignite,
    /// The sword striking the glass: a crackle and a zap.
    Clash { pan: f32 },
    /// The wand casting: a rising arpeggio and a shimmer of tinkles.
    Cast { pan: f32, seed: u32 },
    /// The wand appearing (tinkles rising) or vanishing (falling).
    Shimmer { up: bool },
    /// The mallet landing: a woodblock knock, a thud and a cartoon boing.
    Bonk { pan: f32 },
    /// The mallet swinging in, or away.
    Whoosh { up: bool },
    /// The rubber chicken.
    Squawk { pan: f32, seed: u32 },
}

/// Continuous sounds, each driven by a value from 0 (silent) to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// The charge dial's level: a whine that climbs as it fills.
    Charge = 0,
    /// The laser's speed: a hum with a sizzle.
    Laser = 1,
    /// The pen's speed: felt tip on glass.
    Pen = 2,
    /// The energy sword's hum while it's lit.
    Hum = 3,
    /// How fast the sword swings: the hum bends up and whooshes.
    Swing = 4,
    /// How fast the wand moves: it tinkles as it goes.
    Sparkle = 5,
}

const PARAMS: [Param; 6] = [Param::Charge, Param::Laser, Param::Pen, Param::Hum, Param::Swing, Param::Sparkle];

/// Notes of a major pentatonic scale, as ratios, for anything that tinkles.
const PENTATONIC: [f32; 6] = [1.0, 1.125, 1.25, 1.5, 1.667, 2.0];

#[derive(Clone, Copy, Debug)]
enum Cmd {
    Play(Sfx),
    Set(Param, f32),
}

// ------------------------------------------------------------------------------- building blocks

#[derive(Clone, Copy)]
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Noise {
        Noise(seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453) | 1)
    }
    /// Uniform in [0, 1).
    fn f(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / 16_777_216.0
    }
    /// White noise in [-1, 1).
    fn white(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
}

/// A state-variable filter (Simper's trapezoidal form): band-pass and low-pass at once.
#[derive(Clone, Copy, Default)]
struct Svf {
    a: f32,
    b: f32,
}

impl Svf {
    fn run(&mut self, x: f32, fc: f32, q: f32) -> (f32, f32) {
        let g = (PI * fc.clamp(20.0, RATE as f32 * 0.45) * DT).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.b;
        let v1 = a1 * self.a + a2 * v3;
        let v2 = self.b + a2 * self.a + a3 * v3;
        self.a = 2.0 * v1 - self.a;
        self.b = 2.0 * v2 - self.b;
        (v1, v2)
    }
}

/// A one-pole high-pass, for crisp clicks.
#[derive(Clone, Copy, Default)]
struct HighPass {
    x1: f32,
    y1: f32,
}

impl HighPass {
    fn run(&mut self, x: f32, fc: f32) -> f32 {
        let rc = 1.0 / (TAU * fc);
        let a = rc / (rc + DT);
        let y = a * (self.y1 + x - self.x1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

fn decay(t: f32, tau: f32) -> f32 {
    (-t / tau).exp()
}
fn attack(t: f32, len: f32) -> f32 {
    (t / len).clamp(0.0, 1.0)
}
/// A sine started at time zero, evaluated `t` seconds in (before zero it's silent).
fn ping(t: f32, f: f32) -> f32 {
    if t < 0.0 { 0.0 } else { (TAU * f * t).sin() }
}

// ------------------------------------------------------------------------------- one-shot voices

#[derive(Clone, Copy, Default)]
struct Grain {
    start: f32,
    f: f32,
    decay: f32,
    amp: f32,
}

// The crack keeps its shards inline, so starting a voice never allocates on the audio thread.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy)]
enum Kind {
    Tap { f0: f32, f1: f32, ph: f32, hp: HighPass },
    Tick { f: f32 },
    Crack { power: f32, ph: f32, snap: Svf, sweep: Svf, crackle: HighPass, grains: [Grain; 28], count: usize },
    Slide { out: bool, band: Svf },
    Chime { f: f32 },
    Spot { ph: f32, hp: HighPass },
    Focus { ph: f32, low: Svf },
    Clear { low: Svf },
    Resize { bigger: bool, ph: f32 },
    PenDown { hp: HighPass },
    Ignite { on: bool, ph: f32, low: Svf, hiss: Svf, hp: HighPass },
    Clash { snap: Svf, crackle: HighPass },
    Cast { grains: [Grain; 12], count: usize, low: Svf },
    Shimmer { up: bool, grains: [Grain; 12] },
    Bonk { ph: f32, ph2: f32, hp: HighPass },
    Whoosh { up: bool, band: Svf },
    Squawk { dur: f32, ph: f32, f1: Svf, f2: Svf, breath: Svf, jitter: Follow },
}

#[derive(Clone, Copy)]
struct Voice {
    kind: Kind,
    t: f32,
    pan: f32,
    gain: f32,
    noise: Noise,
}

impl Voice {
    fn new(sfx: Sfx, seed: u32) -> Voice {
        let mut noise = Noise::new(seed);
        let (kind, pan, gain) = match sfx {
            Sfx::Tap { pan, seed } => {
                let mut r = Noise::new(seed);
                let v = r.range(0.93, 1.07);
                (Kind::Tap { f0: 1500.0 * v, f1: 520.0 * v, ph: 0.0, hp: HighPass::default() }, pan, 0.42)
            }
            Sfx::Tick { pan, level } => (Kind::Tick { f: 900.0 + 1300.0 * level.clamp(0.0, 1.0) }, pan, 0.2),
            Sfx::Crack { pan, power, seed } => {
                let mut r = Noise::new(seed ^ 0x9e37_79b9);
                let mut grains = [Grain::default(); 28];
                let count = (12.0 + 16.0 * power.clamp(0.0, 1.0)) as usize;
                // Shards land over most of a second, densest at first. Their amplitudes are scaled
                // so however many ring at once (their phases are random), they stay a sparkle.
                let each = 0.16 / (count as f32).sqrt();
                for g in grains.iter_mut().take(count) {
                    let start = (-(r.f().max(1e-4)).ln() * 0.13).min(0.85);
                    *g = Grain {
                        start: 0.015 + start,
                        f: 2200.0 * (8800.0f32 / 2200.0).powf(r.f()),
                        decay: r.range(0.025, 0.11),
                        amp: r.range(0.5, 1.3) * each * (1.0 - start),
                    };
                }
                let kind = Kind::Crack { power, ph: 0.0, snap: Svf::default(), sweep: Svf::default(), crackle: HighPass::default(), grains, count };
                (kind, pan, 0.6)
            }
            Sfx::Extend { gain } => (Kind::Slide { out: true, band: Svf::default() }, 0.15, 0.55 * gain),
            Sfx::Retract { gain } => (Kind::Slide { out: false, band: Svf::default() }, 0.15, 0.5 * gain),
            Sfx::Chime { pitch } => (Kind::Chime { f: 660.0 * pitch }, 0.0, 0.16),
            Sfx::Spot => (Kind::Spot { ph: 0.0, hp: HighPass::default() }, 0.0, 0.38),
            Sfx::Focus { pan } => (Kind::Focus { ph: 0.0, low: Svf::default() }, pan, 0.42),
            Sfx::Clear => (Kind::Clear { low: Svf::default() }, 0.0, 0.45),
            Sfx::Resize { bigger } => (Kind::Resize { bigger, ph: 0.0 }, 0.0, 0.32),
            Sfx::PenDown { pan } => (Kind::PenDown { hp: HighPass::default() }, pan, 0.3),
            Sfx::Ignite => (Kind::Ignite { on: true, ph: 0.0, low: Svf::default(), hiss: Svf::default(), hp: HighPass::default() }, 0.1, 0.42),
            Sfx::Unignite => (Kind::Ignite { on: false, ph: 0.0, low: Svf::default(), hiss: Svf::default(), hp: HighPass::default() }, 0.1, 0.36),
            Sfx::Clash { pan } => (Kind::Clash { snap: Svf::default(), crackle: HighPass::default() }, pan, 0.42),
            Sfx::Cast { pan, seed } => {
                let mut r = Noise::new(seed ^ 0x5bd1_e995);
                let mut grains = [Grain::default(); 12];
                for g in grains.iter_mut() {
                    *g = Grain {
                        start: r.range(0.06, 0.6),
                        f: 2093.0 * PENTATONIC[(r.f() * 6.0) as usize % 6] * if r.f() < 0.3 { 2.0 } else { 1.0 },
                        decay: r.range(0.05, 0.12),
                        amp: r.range(0.05, 0.11),
                    };
                }
                (Kind::Cast { grains, count: 12, low: Svf::default() }, pan, 0.26)
            }
            Sfx::Shimmer { up } => {
                let mut grains = [Grain::default(); 12];
                for (i, g) in grains.iter_mut().enumerate() {
                    let step = if up { i } else { 11 - i };
                    *g = Grain { start: 0.03 * i as f32, f: 1046.5 * PENTATONIC[step % 6] * (1 + step / 6) as f32, decay: 0.11, amp: 0.12 };
                }
                (Kind::Shimmer { up, grains }, 0.0, 0.24)
            }
            Sfx::Bonk { pan } => (Kind::Bonk { ph: 0.0, ph2: 0.0, hp: HighPass::default() }, pan, 0.38),
            Sfx::Whoosh { up } => (Kind::Whoosh { up, band: Svf::default() }, 0.0, 0.34),
            Sfx::Squawk { pan, seed } => {
                let mut r = Noise::new(seed);
                let kind = Kind::Squawk { dur: r.range(0.42, 0.56), ph: 0.0, f1: Svf::default(), f2: Svf::default(), breath: Svf::default(), jitter: Follow::default() };
                (kind, pan, 0.55)
            }
        };
        noise.f();
        Voice { kind, t: 0.0, pan: pan.clamp(-1.0, 1.0), gain, noise }
    }

    /// The next sample, or `None` once the voice has finished.
    fn next(&mut self) -> Option<f32> {
        let t = self.t;
        self.t += DT;
        let n = &mut self.noise;
        let s = match &mut self.kind {
            Kind::Tap { f0, f1, ph, hp } => {
                if t > 0.32 { return None; }
                let f = *f1 + (*f0 - *f1) * decay(t, 0.018);
                *ph += TAU * f * DT;
                let tone = ph.sin() * attack(t, 0.0012) * decay(t, 0.05);
                let body = ping(t, 230.0) * decay(t, 0.05) * 0.35;
                let click = hp.run(n.white(), 2500.0) * decay(t, 0.0022) * 0.55;
                tone * 0.8 + body + click
            }
            Kind::Tick { f } => {
                if t > 0.09 { return None; }
                (ping(t, *f) + 0.35 * ping(t, *f * 2.0)) * attack(t, 0.0008) * decay(t, 0.014)
            }
            Kind::Crack { power, ph, snap, sweep, crackle, grains, count } => {
                if t > 2.0 { return None; }
                let p = *power;
                let f = 52.0 + 110.0 * decay(t, 0.03);
                *ph += TAU * f * DT;
                let thump = ph.sin() * attack(t, 0.002) * decay(t, 0.16) * (0.45 + 0.4 * p);
                let w = n.white();
                let snap_s = snap.run(w, 3800.0, 0.9).0 * decay(t, 0.035) * 0.75;
                // Crackle running outward while the crack grows (about a quarter second).
                let density = (1.0 - t / 0.3).max(0.0);
                let impulse = if n.f() < 0.03 * density { n.range(-1.0, 1.0) } else { 0.0 };
                let crackle_s = crackle.run(impulse, 1800.0) * 0.9;
                let mut glass = 0.0;
                for g in grains.iter().take(*count) {
                    let tt = t - g.start;
                    if tt < 0.0 || tt > g.decay * 7.0 { continue; }
                    glass += (ping(tt, g.f) + 0.45 * ping(tt, g.f * 2.76)) * g.amp * attack(tt, 0.0005) * decay(tt, g.decay);
                }
                let cutoff = 300.0 + 2400.0 * decay(t, 0.18);
                let whoosh = sweep.run(n.white(), cutoff, 0.7).1 * attack(t, 0.02) * decay(t, 0.35) * (0.2 + 0.25 * p);
                thump + snap_s + crackle_s + glass + whoosh
            }
            Kind::Slide { out, band } => {
                if t > 0.62 { return None; }
                let u = (t / 0.38).min(1.0);
                let center = if *out { 900.0 + 2600.0 * u } else { 3500.0 - 2600.0 * u };
                let env = attack(t, 0.03) * (1.0 - crate::math::smoothstep(0.24, 0.42, t));
                let rush = band.run(n.white(), center, 2.2).0 * env * 0.9;
                let mut clicks = 0.0;
                for i in 0..5 {
                    let at = 0.07 + 0.06 * i as f32;
                    let dt = t - at;
                    if (0.0..0.05).contains(&dt) {
                        let rise = if *out { 1.0 + 0.08 * i as f32 } else { 1.32 - 0.08 * i as f32 };
                        clicks += (ping(dt, 3200.0 * rise) + 0.6 * ping(dt, 5100.0 * rise)) * decay(dt, 0.006) * (0.16 + 0.03 * i as f32);
                    }
                }
                let end = t - 0.38;
                let clack = if end >= 0.0 { (ping(end, 150.0) * decay(end, 0.05) * 0.7 + ping(end, 2900.0) * decay(end, 0.012) * 0.3) * if *out { 1.0 } else { 0.6 } } else { 0.0 };
                rush + clicks + clack
            }
            Kind::Chime { f } => {
                if t > 0.7 { return None; }
                let bell = |tt: f32, f: f32| if tt < 0.0 { 0.0 } else {
                    (ping(tt, f) + 0.25 * ping(tt, 2.0 * f) + 0.1 * ping(tt, 3.01 * f)) * attack(tt, 0.004) * decay(tt, 0.16)
                };
                bell(t, *f) + bell(t - 0.075, *f * 1.5)
            }
            Kind::Spot { ph, hp } => {
                if t > 0.75 { return None; }
                let f = 62.0 + 70.0 * decay(t, 0.025);
                *ph += TAU * f * DT;
                let thump = ph.sin() * attack(t, 0.002) * decay(t, 0.12);
                let click = hp.run(n.white(), 3000.0) * decay(t, 0.003) * 0.7;
                let hum = (ping(t, 120.0) * 0.3 + ping(t, 240.0) * 0.12) * attack(t, 0.03) * decay(t, 0.25) * 0.5;
                thump + click + hum
            }
            Kind::Focus { ph, low } => {
                if t > 0.55 { return None; }
                let f = 70.0 + 160.0 * decay(t, 0.07);
                *ph += TAU * f * DT;
                let tone = ph.sin() * attack(t, 0.008) * decay(t, 0.2);
                let air = low.run(n.white(), 900.0, 0.7).1 * decay(t, 0.08) * 0.35;
                tone * 0.8 + air
            }
            Kind::Clear { low } => {
                if t > 0.8 { return None; }
                let cutoff = 200.0 + 4500.0 * decay(t, 0.16);
                low.run(n.white(), cutoff, 0.8).1 * attack(t, 0.03) * decay(t, 0.22) * 1.2
            }
            Kind::Resize { bigger, ph } => {
                if t > 0.3 { return None; }
                let u = (t / 0.08).min(1.0);
                let f = if *bigger { 380.0 + 260.0 * u } else { 640.0 - 260.0 * u };
                *ph += TAU * f * DT;
                ph.sin() * attack(t, 0.003) * decay(t, 0.09)
            }
            Kind::PenDown { hp } => {
                if t > 0.08 { return None; }
                ping(t, 1250.0) * decay(t, 0.01) * 0.8 + hp.run(n.white(), 3000.0) * decay(t, 0.002) * 0.5
            }
            Kind::Ignite { on, ph, low, hiss, hp } => {
                if t > 0.8 { return None; }
                let snap = if *on { hp.run(n.white(), 2200.0) * decay(t, 0.006) * 0.9 } else { 0.0 };
                let d = (t - if *on { 0.03 } else { 0.0 }).max(0.0);
                // The hum glides up out of the hilt (or down into it).
                let f = if *on { 38.0 + 72.0 * (1.0 - decay(d, 0.09)) } else { 110.0 - 80.0 * (1.0 - decay(d, 0.12)) };
                *ph = (*ph + TAU * f * DT) % TAU;
                let saw = ph.sin() + 0.5 * (2.0 * *ph).sin() + 0.33 * (3.0 * *ph).sin() + 0.25 * (4.0 * *ph).sin() + 0.2 * (5.0 * *ph).sin();
                let cutoff = if *on { 300.0 + 2600.0 * attack(d, 0.12) * decay(d, 0.5) } else { 2400.0 * decay(d, 0.15) + 200.0 };
                let tone = low.run(saw, cutoff, 0.9).1;
                let env = if *on { attack(d, 0.04) * (0.55 + 0.45 * decay(d, 0.18)) * (1.0 - crate::math::smoothstep(0.45, 0.78, t)) } else { decay(d, 0.16) };
                let air = hiss.run(n.white(), 2500.0, 0.8).0 * decay(d, if *on { 0.15 } else { 0.08 }) * 0.25;
                let click = if !*on && t > 0.3 { ping(t - 0.3, 1800.0) * decay(t - 0.3, 0.008) * 0.25 } else { 0.0 };
                snap * 0.6 + (tone * 0.5 + air) * env + click
            }
            Kind::Clash { snap, crackle } => {
                if t > 0.5 { return None; }
                let crack = snap.run(n.white(), 3200.0, 1.4).0 * decay(t, 0.05);
                let density = (1.0 - t / 0.25).max(0.0);
                let impulse = if n.f() < 0.05 * density { n.range(-1.0, 1.0) } else { 0.0 };
                let sparks = crackle.run(impulse, 2500.0) * 0.8;
                let zap = (TAU * 1500.0 * t + 6.0 * decay(t, 0.06) * (TAU * 230.0 * t).sin()).sin() * decay(t, 0.12) * 0.4;
                let thump = ping(t, 90.0) * decay(t, 0.06) * 0.4;
                crack + sparks + zap + thump
            }
            Kind::Cast { grains, count, low } => {
                if t > 1.0 { return None; }
                let mut notes = 0.0;
                for (i, ratio) in [1.0, 1.26, 1.5, 2.0].iter().enumerate() {
                    let tt = t - 0.055 * i as f32;
                    let f = 1046.5 * ratio;
                    if tt >= 0.0 {
                        notes += (ping(tt, f) + 0.3 * ping(tt, 2.01 * f) + 0.12 * ping(tt, 3.02 * f)) * attack(tt, 0.002) * decay(tt, 0.22);
                    }
                }
                let mut shimmer = 0.0;
                for g in grains.iter().take(*count) {
                    let tt = t - g.start;
                    if (0.0..g.decay * 7.0).contains(&tt) { shimmer += ping(tt, g.f) * g.amp * attack(tt, 0.001) * decay(tt, g.decay); }
                }
                let puff = low.run(n.white(), 1200.0, 0.7).1 * decay(t, 0.1) * 0.4;
                notes * 0.5 + shimmer * 2.0 + puff
            }
            Kind::Shimmer { up, grains } => {
                if t > 0.75 { return None; }
                let mut s = 0.0;
                for g in grains.iter() {
                    let tt = t - g.start;
                    if (0.0..0.7).contains(&tt) { s += (ping(tt, g.f) + 0.25 * ping(tt, 2.0 * g.f)) * g.amp * attack(tt, 0.002) * decay(tt, g.decay); }
                }
                let _ = up;
                s * 2.4
            }
            Kind::Bonk { ph, ph2, hp } => {
                if t > 0.7 { return None; }
                let wood = (ping(t, 760.0) * 0.7 + ping(t, 1290.0) * 0.4 + ping(t, 2050.0) * 0.2) * attack(t, 0.0008) * decay(t, 0.05);
                let f = 60.0 + 90.0 * decay(t, 0.02);
                *ph = (*ph + TAU * f * DT) % TAU;
                let thud = ph.sin() * decay(t, 0.09) * 0.8;
                let click = hp.run(n.white(), 2500.0) * decay(t, 0.002) * 0.5;
                let d = (t - 0.02).max(0.0);
                let fb = 210.0 * (1.0 + 0.28 * (TAU * 7.5 * d).sin() * decay(d, 0.25));
                *ph2 = (*ph2 + TAU * fb * DT) % TAU;
                let boing = ph2.sin() * attack(d, 0.01) * decay(d, 0.22) * 0.35;
                wood + thud + click + boing
            }
            Kind::Whoosh { up, band } => {
                if t > 0.5 { return None; }
                let u = (t / 0.3).min(1.0);
                let center = if *up { 350.0 + 1050.0 * u } else { 1400.0 - 1050.0 * u };
                let rush = band.run(n.white(), center, 1.2).0 * attack(t, 0.05) * (1.0 - crate::math::smoothstep(0.18, 0.36, t));
                let clack = if *up && t > 0.3 { (ping(t - 0.3, 900.0) * 0.6 + ping(t - 0.3, 1500.0) * 0.3) * decay(t - 0.3, 0.025) } else { 0.0 };
                rush * 0.9 + clack
            }
            Kind::Squawk { dur, ph, f1, f2, breath, jitter } => {
                if t > *dur + 0.02 { return None; }
                let u = (t / *dur).min(1.0);
                jitter.target = n.white();
                let wobble = jitter.step(0.004, 0.004);
                // `max(0)`: in f32, sin(π) is a hair below zero, and a fractional power of that is NaN.
                let f = (650.0 + 850.0 * (PI * u).sin().max(0.0).powf(0.7)) * (1.0 + 0.035 * (TAU * 31.0 * t).sin() + 0.02 * wobble);
                *ph = (*ph + TAU * f * DT) % TAU;
                // A buzzy pulse (every harmonic at once) through two nasal formants.
                let mut pulse = 0.0;
                for k in 1..=10 { pulse += (k as f32 * *ph).cos(); }
                pulse *= 0.1;
                let voice = f1.run(pulse, 1150.0, 4.0).0 * 0.25 + f2.run(pulse, 2700.0, 5.0).0 * 0.16 + pulse * 0.12;
                let air = breath.run(n.white(), 3200.0, 1.2).0 * 0.12;
                (voice + air) * attack(t, 0.012) * (1.0 - crate::math::smoothstep(*dur - 0.08, *dur, t)) * 3.0
            }
        };
        Some(s * self.gain)
    }
}

// ------------------------------------------------------------------------------- continuous voices

/// Follows a 0..1 target with a quick attack and a slower release.
#[derive(Clone, Copy, Default)]
struct Follow {
    target: f32,
    value: f32,
}

impl Follow {
    fn step(&mut self, attack: f32, release: f32) -> f32 {
        let tau = if self.target > self.value { attack } else { release };
        self.value += (self.target - self.value) * (1.0 - (-DT / tau).exp());
        self.value
    }
}

#[derive(Clone, Copy, Default)]
struct Continuous {
    charge: Follow,
    charge_amp: Follow,
    charge_ph: f32,
    vib_ph: f32,
    crackle: HighPass,
    laser: Follow,
    laser_ph: f32,
    laser_mod: f32,
    sizzle: Svf,
    pen: Follow,
    scribble: Svf,
    flutter: Follow,
    hum: Follow,
    swing: Follow,
    hum_ph: [f32; 3],
    hum_low: Svf,
    swish: Svf,
    sparkle: Follow,
    /// Tinkles the moving wand has set off: frequency, age, loudness.
    pings: [(f32, f32, f32); 8],
}

// ------------------------------------------------------------------------------- the synth

pub struct Synth {
    voices: Vec<Voice>,
    cont: Continuous,
    noise: Noise,
    seed: u32,
    /// Master level, eased so muting never clicks.
    level: Follow,
}

impl Default for Synth {
    fn default() -> Self {
        Synth { voices: Vec::with_capacity(MAX_VOICES), cont: Continuous::default(), noise: Noise::new(7), seed: 1, level: Follow { target: MASTER, value: MASTER } }
    }
}

impl Synth {
    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play(sfx) => {
                self.seed = self.seed.wrapping_add(0x9e37_79b9);
                if self.voices.len() == MAX_VOICES { self.voices.remove(0); }
                self.voices.push(Voice::new(sfx, self.seed));
            }
            Cmd::Set(Param::Charge, v) => self.cont.charge.target = v.clamp(0.0, 1.0),
            Cmd::Set(Param::Laser, v) => self.cont.laser.target = v.clamp(0.0, 1.0),
            Cmd::Set(Param::Pen, v) => self.cont.pen.target = v.clamp(0.0, 1.0),
            Cmd::Set(Param::Hum, v) => self.cont.hum.target = v.clamp(0.0, 1.0),
            Cmd::Set(Param::Swing, v) => self.cont.swing.target = v.clamp(0.0, 1.0),
            Cmd::Set(Param::Sparkle, v) => self.cont.sparkle.target = v.clamp(0.0, 1.0),
        }
    }

    fn set_level(&mut self, level: f32) {
        self.level.target = level;
    }

    /// Whether anything is still sounding (or about to).
    fn busy(&self) -> bool {
        let c = &self.cont;
        !self.voices.is_empty() || c.charge_amp.value > 1e-4 || c.charge.target > 0.0 || c.laser.value > 1e-4
            || c.laser.target > 0.0 || c.pen.value > 1e-4 || c.pen.target > 0.0 || c.hum.value > 1e-4 || c.hum.target > 0.0
            || c.sparkle.value > 1e-4 || c.sparkle.target > 0.0 || c.pings.iter().any(|p| p.2 > 0.0)
    }

    fn continuous(&mut self) -> f32 {
        let c = &mut self.cont;
        let n = &mut self.noise;
        let mut out = 0.0;
        // The charge whine: climbs with the dial, shivers, and crackles near the top.
        let level = c.charge.step(0.03, 0.07);
        c.charge_amp.target = crate::math::smoothstep(0.06, 0.3, level);
        let amp = c.charge_amp.step(0.04, 0.06);
        if amp > 1e-5 {
            c.vib_ph += TAU * 7.0 * DT;
            let f = 160.0 + 950.0 * level.powf(1.5);
            c.charge_ph = (c.charge_ph + TAU * f * (1.0 + 0.015 * level * c.vib_ph.sin()) * DT) % TAU;
            let tone = c.charge_ph.sin() + 0.35 * (2.0 * c.charge_ph).sin() + 0.18 * (3.0 * c.charge_ph).sin();
            let impulse = if n.f() < 0.004 * level { n.white() } else { 0.0 };
            out += (tone * 0.075 + c.crackle.run(impulse, 2000.0) * 0.25) * amp * (0.5 + 0.5 * level);
        }
        // The laser: an FM hum with a sizzle on top, louder and higher the faster it moves.
        let laser = c.laser.step(0.025, 0.12);
        if laser > 1e-5 {
            let f = 380.0 + 320.0 * laser;
            c.laser_mod = (c.laser_mod + TAU * f * 1.51 * DT) % TAU;
            c.laser_ph = (c.laser_ph + TAU * f * DT) % TAU;
            let hum = (c.laser_ph + 1.6 * laser * c.laser_mod.sin()).sin();
            let sizzle = c.sizzle.run(n.white(), 5200.0, 1.2).0;
            out += (hum * 0.06 + sizzle * 0.05) * laser;
        }
        // The pen: band-passed noise, like a felt tip on glass.
        let pen = c.pen.step(0.02, 0.08);
        if pen > 1e-5 {
            c.flutter.target = n.f();
            let flutter = c.flutter.step(0.01, 0.01);
            out += c.scribble.run(n.white(), 1800.0 + 2200.0 * pen, 1.6).0 * 0.22 * pen * (0.75 + 0.25 * flutter);
        }
        // The energy sword: two beating saw-ish drones an octave over a sub, darker at rest; a
        // swing bends them up, opens them out and adds a whoosh.
        let hum = c.hum.step(0.08, 0.12);
        let swing = c.swing.step(0.03, 0.15);
        if hum > 1e-5 {
            let pitch = 1.0 + 0.35 * swing;
            for (i, f) in [92.0, 92.6, 46.0].iter().enumerate() {
                c.hum_ph[i] = (c.hum_ph[i] + TAU * f * pitch * DT) % TAU;
            }
            let [a, b, sub] = c.hum_ph;
            let drone = a.sin() + 0.5 * (2.0 * a).sin() + 0.33 * (3.0 * a).sin() + 0.6 * (b.sin() + 0.5 * (2.0 * b).sin()) + 0.3 * sub.sin();
            let body = c.hum_low.run(drone, 500.0 + 2200.0 * swing, 0.8).1;
            let whoosh = c.swish.run(n.white(), 700.0 + 900.0 * swing, 1.0).0 * swing;
            out += (body * 0.05 * (1.0 + 1.4 * swing) + whoosh * 0.12) * hum;
        }
        // The wand: soft pentatonic tinkles, more of them the faster it moves.
        let sparkle = c.sparkle.step(0.05, 0.2);
        if sparkle > 1e-4 && n.f() < 22.0 * sparkle * DT
            && let Some(slot) = c.pings.iter_mut().find(|p| p.2 <= 0.0)
        {
            *slot = (2093.0 * PENTATONIC[(n.f() * 6.0) as usize % 6] * if n.f() < 0.4 { 2.0 } else { 1.0 }, 0.0, n.range(0.025, 0.05));
        }
        for p in c.pings.iter_mut().filter(|p| p.2 > 0.0) {
            out += ping(p.1, p.0) * p.2 * decay(p.1, 0.07);
            p.1 += DT;
            if p.1 > 0.5 { p.2 = 0.0; }
        }
        out
    }

    /// Mixes the next `out.len() / 2` stereo frames (interleaved) into `out`.
    pub fn render(&mut self, out: &mut [f32]) {
        for frame in out.as_chunks_mut::<2>().0 {
            let (mut l, mut r) = (0.0, 0.0);
            // A voice that ever goes non-finite is dropped before it can poison the mix.
            self.voices.retain_mut(|v| match v.next() {
                Some(s) if s.is_finite() => {
                    let a = (v.pan + 1.0) * PI * 0.25;
                    l += s * a.cos();
                    r += s * a.sin();
                    true
                }
                _ => false,
            });
            let mut c = self.continuous() * std::f32::consts::FRAC_1_SQRT_2;
            if !c.is_finite() {
                self.cont = Continuous::default();
                c = 0.0;
            }
            let level = self.level.step(0.02, 0.02);
            frame[0] = soft_clip((l + c) * level);
            frame[1] = soft_clip((r + c) * level);
        }
    }
}

/// Leaves quiet signals alone and rounds off peaks above 0.6 instead of clipping them.
fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a < 0.6 { x } else { x.signum() * (0.6 + 0.4 * ((a - 0.6) / 0.4).tanh()) }
}

// ------------------------------------------------------------------------------- live output

#[cfg(target_os = "macos")]
mod coreaudio {
    use std::ffi::c_void;

    #[repr(C)]
    pub struct Desc {
        pub ty: u32,
        pub sub: u32,
        pub manufacturer: u32,
        pub flags: u32,
        pub mask: u32,
    }
    #[repr(C)]
    pub struct Format {
        pub rate: f64,
        pub format: u32,
        pub flags: u32,
        pub bytes_per_packet: u32,
        pub frames_per_packet: u32,
        pub bytes_per_frame: u32,
        pub channels: u32,
        pub bits: u32,
        pub reserved: u32,
    }
    #[repr(C)]
    pub struct Buffer {
        pub channels: u32,
        pub bytes: u32,
        pub data: *mut c_void,
    }
    #[repr(C)]
    pub struct BufferList {
        pub count: u32,
        pub buffers: [Buffer; 1],
    }
    pub type Render = unsafe extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut BufferList) -> i32;
    #[repr(C)]
    pub struct Callback {
        pub render: Render,
        pub context: *mut c_void,
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        pub fn AudioComponentFindNext(after: *mut c_void, desc: *const Desc) -> *mut c_void;
        pub fn AudioComponentInstanceNew(component: *mut c_void, out: *mut *mut c_void) -> i32;
        pub fn AudioUnitSetProperty(unit: *mut c_void, id: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> i32;
        pub fn AudioUnitInitialize(unit: *mut c_void) -> i32;
        pub fn AudioOutputUnitStart(unit: *mut c_void) -> i32;
        pub fn AudioOutputUnitStop(unit: *mut c_void) -> i32;
    }

    pub const fn fourcc(s: &[u8; 4]) -> u32 {
        (s[0] as u32) << 24 | (s[1] as u32) << 16 | (s[2] as u32) << 8 | s[3] as u32
    }
    pub const STREAM_FORMAT: u32 = 8;
    pub const SET_RENDER_CALLBACK: u32 = 23;
    pub const SCOPE_INPUT: u32 = 1;
    pub const FLOAT: u32 = 1;
    pub const PACKED: u32 = 8;
}

/// The audio thread's side: commands in, samples out.
struct Engine {
    rx: Receiver<Cmd>,
    synth: Synth,
    /// The level when sound is on (zero for a silent check).
    level: f32,
    scratch: Vec<f32>,
    live: &'static Live,
}

struct Live {
    tx: SyncSender<Cmd>,
    params: [AtomicU32; 6],
    enabled: AtomicBool,
    /// Milliseconds since `epoch` when something last sounded or was asked for.
    busy_at: AtomicU64,
    asked_at: AtomicU64,
    /// Rendered frames, for checking the output runs (`POINTER_DEBUG`).
    frames: AtomicU64,
    epoch: Instant,
    unit: Mutex<Unit>,
    /// Whether the output runs. Only the control thread changes it; anyone reads it without
    /// waiting on a start in progress.
    running: AtomicBool,
    /// The thread that starts and stops the output, woken when sound is wanted.
    control: OnceLock<std::thread::Thread>,
}

struct Unit {
    #[cfg(target_os = "macos")]
    au: *mut std::ffi::c_void,
}

// SAFETY: the unit pointer is only used under the mutex, through thread-safe AudioToolbox calls.
unsafe impl Send for Unit {}

/// How long the output keeps running after the last sound, or the last time the pointer
/// moved. A running output sounds a click at once but costs a couple of percent of a core in
/// the system's audio I/O even while silent; a stopped one costs nothing, and moving the
/// pointer starts it again (in the background) before the click that usually follows.
const KEEP_RUNNING_MS: u64 = 5_000;

impl Live {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
    /// Asks for the output to run. Never waits: starting an output can take a fifth of a
    /// second (a display's speakers over DisplayPort do), which on the main thread froze the
    /// pointer on the first click after a quiet spell. The control thread starts it, and what
    /// was asked for plays as soon as it runs.
    fn wake(&self) {
        // Never zero, which reads as "never asked".
        self.asked_at.store(self.now_ms().max(1), Ordering::Relaxed);
        if !self.running.load(Ordering::Acquire) && let Some(control) = self.control.get() { control.unpark(); }
    }
    /// The control thread: starts the output when it's wanted and stops it after a quiet
    /// spell, checking again after each change (a sound asked for while it stopped).
    fn control(&self) -> ! {
        loop {
            loop {
                let last = self.busy_at.load(Ordering::Relaxed).max(self.asked_at.load(Ordering::Relaxed));
                let want = self.asked_at.load(Ordering::Relaxed) > 0 && self.now_ms().saturating_sub(last) < KEEP_RUNNING_MS;
                if want == self.running.load(Ordering::Acquire) { break; }
                let unit = self.unit.lock().unwrap_or_else(|e| e.into_inner());
                #[cfg(target_os = "macos")]
                // SAFETY: the unit was initialised in `start_live` and lives for the process.
                unsafe {
                    if want { coreaudio::AudioOutputUnitStart(unit.au); } else { coreaudio::AudioOutputUnitStop(unit.au); }
                }
                drop(unit);
                self.running.store(want, Ordering::Release);
            }
            std::thread::park_timeout(Duration::from_millis(500));
        }
    }
}

impl Engine {
    fn fill(&mut self, out: &mut [f32]) {
        while let Ok(cmd) = self.rx.try_recv() { self.synth.apply(cmd); }
        for (i, p) in PARAMS.into_iter().enumerate() {
            self.synth.apply(Cmd::Set(p, f32::from_bits(self.live.params[i].load(Ordering::Relaxed))));
        }
        let on = self.live.enabled.load(Ordering::Relaxed);
        self.synth.set_level(if on { self.level } else { 0.0 });
        if !on { self.synth.voices.clear(); }
        self.live.frames.fetch_add((out.len() / 2) as u64, Ordering::Relaxed);
        if !self.synth.busy() {
            // Nothing is sounding (the output runs a while after the last sound, ready for
            // the next): silence, without stepping every voice and smoother for each sample.
            out.fill(0.0);
            return;
        }
        self.synth.render(out);
        self.live.busy_at.store(self.live.now_ms(), Ordering::Relaxed);
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn render(context: *mut std::ffi::c_void, _flags: *mut u32, _time: *const std::ffi::c_void, _bus: u32, frames: u32, data: *mut coreaudio::BufferList) -> i32 {
    // SAFETY: `context` is the leaked Engine, used only on this (the render) thread; CoreAudio
    // hands a valid buffer list with `count` buffers.
    unsafe {
        let engine = &mut *context.cast::<Engine>();
        let list = &mut *data;
        let buffers = std::slice::from_raw_parts_mut(list.buffers.as_mut_ptr(), list.count as usize);
        let frames = frames as usize;
        if let [only] = buffers
            && only.channels == 2
            && only.bytes as usize >= frames * 8
        {
            let out = std::slice::from_raw_parts_mut(only.data.cast::<f32>(), frames * 2);
            engine.fill(out);
            return 0;
        }
        // Non-interleaved (one buffer a channel): mix interleaved, then split.
        let frames = frames.min(engine.scratch.len() / 2);
        let mut scratch = std::mem::take(&mut engine.scratch);
        engine.fill(&mut scratch[..frames * 2]);
        for (ch, b) in buffers.iter_mut().enumerate() {
            let n = (b.bytes as usize / 4).min(frames);
            let dst = std::slice::from_raw_parts_mut(b.data.cast::<f32>(), n);
            for (i, d) in dst.iter_mut().enumerate() { *d = scratch[i * 2 + ch.min(1)]; }
        }
        engine.scratch = scratch;
        0
    }
}

enum Backend {
    Live(&'static Live),
    Record(Mutex<Vec<(f64, Cmd)>>),
}

static BACKEND: OnceLock<Backend> = OnceLock::new();

/// Opens the default output. Sound is off when `enabled` is false (it can change later).
pub fn start_live(enabled: bool, level: f32) {
    #[cfg(target_os = "macos")]
    {
        use coreaudio::*;
        let (tx, rx) = sync_channel(256);
        let live: &'static Live = Box::leak(Box::new(Live {
            tx,
            params: Default::default(),
            enabled: AtomicBool::new(enabled),
            busy_at: AtomicU64::new(0),
            asked_at: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            epoch: Instant::now(),
            unit: Mutex::new(Unit { au: std::ptr::null_mut() }),
            running: AtomicBool::new(false),
            control: OnceLock::new(),
        }));
        let level = MASTER * level;
        let synth = Synth { level: Follow { target: level, value: level }, ..Synth::default() };
        let engine: *mut Engine = Box::into_raw(Box::new(Engine { rx, synth, level, scratch: vec![0.0; 16384], live }));
        // SAFETY: plain AudioToolbox setup with C structs laid out as the headers declare them.
        let au = unsafe {
            let desc = Desc { ty: fourcc(b"auou"), sub: fourcc(b"def "), manufacturer: fourcc(b"appl"), flags: 0, mask: 0 };
            let component = AudioComponentFindNext(std::ptr::null_mut(), &desc);
            if component.is_null() { eprintln!("pointer: no audio output; sounds are off"); return; }
            let mut au = std::ptr::null_mut();
            if AudioComponentInstanceNew(component, &mut au) != 0 || au.is_null() { eprintln!("pointer: couldn't open the audio output"); return; }
            let format = Format {
                rate: RATE as f64,
                format: fourcc(b"lpcm"),
                flags: FLOAT | PACKED,
                bytes_per_packet: 8,
                frames_per_packet: 1,
                bytes_per_frame: 8,
                channels: 2,
                bits: 32,
                reserved: 0,
            };
            let callback = Callback { render, context: engine.cast() };
            let ok = AudioUnitSetProperty(au, STREAM_FORMAT, SCOPE_INPUT, 0, (&format as *const Format).cast(), std::mem::size_of::<Format>() as u32) == 0
                && AudioUnitSetProperty(au, SET_RENDER_CALLBACK, SCOPE_INPUT, 0, (&callback as *const Callback).cast(), std::mem::size_of::<Callback>() as u32) == 0
                && AudioUnitInitialize(au) == 0;
            if !ok { eprintln!("pointer: couldn't set up the audio output; sounds are off"); return; }
            au
        };
        live.unit.lock().unwrap_or_else(|e| e.into_inner()).au = au;
        if BACKEND.set(Backend::Live(live)).is_err() { return; }
        let control = std::thread::Builder::new().name("pointer-audio".into()).spawn(move || live.control()).expect("audio control thread");
        let _ = live.control.set(control.thread().clone());
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (enabled, level);
}

/// Headless: log effects against the virtual clock, for `write_wav`.
pub fn start_recording() {
    let _ = BACKEND.set(Backend::Record(Mutex::new(Vec::new())));
}

pub fn set_enabled(on: bool) {
    if let Some(Backend::Live(live)) = BACKEND.get() {
        live.enabled.store(on, Ordering::Relaxed);
        if !on { for p in &live.params { p.store(0f32.to_bits(), Ordering::Relaxed); } }
    }
}

/// Plays `sfx` now (or, headless, at virtual time `t`).
pub fn play(t: f64, sfx: Sfx) {
    match BACKEND.get() {
        Some(Backend::Live(live)) => {
            if !live.enabled.load(Ordering::Relaxed) { return; }
            live.wake();
            let _ = live.tx.try_send(Cmd::Play(sfx));
        }
        Some(Backend::Record(log)) => log.lock().unwrap_or_else(|e| e.into_inner()).push((t, Cmd::Play(sfx))),
        None => {}
    }
}

/// The pointer is moving: get the output running, so a click that follows sounds at once.
pub fn prepare() {
    if let Some(Backend::Live(live)) = BACKEND.get() && live.enabled.load(Ordering::Relaxed) { live.wake(); }
}

/// Sets a continuous sound's level, 0 to 1.
pub fn set(t: f64, param: Param, value: f32) {
    match BACKEND.get() {
        Some(Backend::Live(live)) => {
            let value = if live.enabled.load(Ordering::Relaxed) { value } else { 0.0 };
            live.params[param as usize].store(value.to_bits(), Ordering::Relaxed);
            if value > 0.0 { live.wake(); }
        }
        Some(Backend::Record(log)) => log.lock().unwrap_or_else(|e| e.into_inner()).push((t, Cmd::Set(param, value))),
        None => {}
    }
}

/// Frames the live output has rendered so far (to check it runs).
pub fn rendered_frames() -> u64 {
    match BACKEND.get() {
        Some(Backend::Live(live)) => live.frames.load(Ordering::Relaxed),
        _ => 0,
    }
}

/// Renders a timed list of commands, as the live output would have played them.
fn render_offline(mut events: Vec<(f64, Cmd)>, seconds: f64) -> Vec<f32> {
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total = (seconds * RATE as f64).ceil() as usize;
    let mut out = vec![0.0f32; total * 2];
    let mut synth = Synth::default();
    let mut next = 0;
    const BLOCK: usize = 64;
    let mut frame = 0;
    while frame < total {
        let now = frame as f64 / RATE as f64;
        while next < events.len() && events[next].0 <= now {
            synth.apply(events[next].1);
            next += 1;
        }
        let end = (frame + BLOCK).min(total);
        synth.render(&mut out[frame * 2..end * 2]);
        frame = end;
    }
    out
}

/// Headless: renders what was logged into a 16-bit stereo WAV `seconds` long (or until the
/// last effect has rung out).
pub fn write_wav(path: &str, seconds: Option<f64>) -> std::io::Result<()> {
    let Some(Backend::Record(log)) = BACKEND.get() else { return Ok(()) };
    let events = std::mem::take(&mut *log.lock().unwrap_or_else(|e| e.into_inner()));
    if std::env::var_os("POINTER_AUDIO_LOG").is_some() {
        for (t, cmd) in &events { eprintln!("audio {t:7.3} {cmd:?}"); }
    }
    let last = events.iter().map(|e| e.0).fold(0.0, f64::max);
    let seconds = seconds.unwrap_or(last + 1.6).max(0.1);
    let samples = render_offline(events, seconds);
    let mut bytes = Vec::with_capacity(44 + samples.len() * 2);
    let data_len = (samples.len() * 2) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn every_effect_sounds_then_ends_without_clipping() {
        let all = [
            Sfx::Tap { pan: -0.4, seed: 3 },
            Sfx::Tick { pan: 0.0, level: 0.7 },
            Sfx::Crack { pan: 0.3, power: 1.0, seed: 9 },
            Sfx::Extend { gain: 1.0 },
            Sfx::Retract { gain: 1.0 },
            Sfx::Chime { pitch: 1.2 },
            Sfx::Spot,
            Sfx::Focus { pan: 0.0 },
            Sfx::Clear,
            Sfx::Resize { bigger: true },
            Sfx::PenDown { pan: 0.0 },
            Sfx::Ignite,
            Sfx::Unignite,
            Sfx::Clash { pan: 0.2 },
            Sfx::Cast { pan: 0.0, seed: 4 },
            Sfx::Shimmer { up: true },
            Sfx::Shimmer { up: false },
            Sfx::Bonk { pan: 0.0 },
            Sfx::Whoosh { up: true },
            Sfx::Squawk { pan: 0.0, seed: 11 },
        ];
        for sfx in all {
            let out = render_offline(vec![(0.0, Cmd::Play(sfx))], 3.0);
            let head = &out[..RATE as usize * 2];
            let tail = &out[RATE as usize * 4..];
            let p = peak(head);
            assert!(p > 0.02, "{sfx:?} is audible: {p}");
            assert!(p < 0.7, "{sfx:?} stays clear of the limiter: {p}");
            assert!(peak(tail) < 1e-3, "{sfx:?} has rung out within two seconds");
            assert!(out.iter().all(|s| s.is_finite()));
        }
    }

    #[test]
    fn continuous_sounds_follow_their_level_and_fall_silent() {
        for p in [Param::Charge, Param::Laser, Param::Pen, Param::Hum, Param::Sparkle] {
            let events = vec![(0.0, Cmd::Set(p, 0.8)), (0.5, Cmd::Set(p, 0.0))];
            let out = render_offline(events, 1.2);
            let on = peak(&out[(0.2 * RATE as f32) as usize * 2..(0.45 * RATE as f32) as usize * 2]);
            let off = peak(&out[(1.0 * RATE as f32) as usize * 2..]);
            assert!(on > 0.01, "{p:?} sounds while set: {on}");
            assert!(off < 1e-3, "{p:?} stops when cleared: {off}");
        }
    }

    #[test]
    fn no_crack_reaches_the_limiter_whatever_its_seed() {
        let mut worst: f32 = 0.0;
        for seed in 0..96u32 {
            let power = 0.5 + 0.5 * (seed % 6) as f32 / 5.0;
            let out = render_offline(vec![(0.0, Cmd::Play(Sfx::Crack { pan: 0.0, power, seed: seed.wrapping_mul(7919) }))], 1.2);
            worst = worst.max(peak(&out));
        }
        assert!(worst < 0.6, "loudest crack peaks at {worst}");
    }

    #[test]
    fn a_crowd_of_taps_stays_bounded() {
        let events = (0..200).map(|i| (i as f64 * 0.002, Cmd::Play(Sfx::Tap { pan: 0.0, seed: i }))).collect();
        let out = render_offline(events, 1.0);
        assert!(peak(&out) <= 1.0);
    }
}

#[cfg(test)]
mod levels {
    use super::*;

    /// `cargo test --release levels -- --nocapture --ignored` prints each effect's peak and loudness.
    #[test]
    #[ignore]
    fn print_levels() {
        let all = [
            ("tap", Sfx::Tap { pan: 0.0, seed: 3 }),
            ("tick", Sfx::Tick { pan: 0.0, level: 0.7 }),
            ("crack", Sfx::Crack { pan: 0.0, power: 1.0, seed: 9 }),
            ("crack.7", Sfx::Crack { pan: 0.0, power: 0.7, seed: 9 }),
            ("extend", Sfx::Extend { gain: 1.0 }),
            ("retract", Sfx::Retract { gain: 1.0 }),
            ("chime", Sfx::Chime { pitch: 1.0 }),
            ("spot", Sfx::Spot),
            ("focus", Sfx::Focus { pan: 0.0 }),
            ("clear", Sfx::Clear),
            ("resize", Sfx::Resize { bigger: true }),
            ("pendown", Sfx::PenDown { pan: 0.0 }),
            ("ignite", Sfx::Ignite),
            ("unignite", Sfx::Unignite),
            ("clash", Sfx::Clash { pan: 0.0 }),
            ("cast", Sfx::Cast { pan: 0.0, seed: 4 }),
            ("shimmer", Sfx::Shimmer { up: true }),
            ("bonk", Sfx::Bonk { pan: 0.0 }),
            ("whoosh", Sfx::Whoosh { up: true }),
            ("squawk", Sfx::Squawk { pan: 0.0, seed: 11 }),
        ];
        for (name, sfx) in all {
            let out = render_offline(vec![(0.0, Cmd::Play(sfx))], 1.5);
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            // Loudness: the loudest 50 ms window's RMS.
            let win = (RATE as usize / 20) * 2;
            let rms = out.chunks(win).map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt()).fold(0.0f32, f32::max);
            println!("{name:>8}: peak {peak:.3}  loudest-50ms rms {rms:.3}");
        }
        for (name, p) in [("charge", Param::Charge), ("laser", Param::Laser), ("pen", Param::Pen), ("hum", Param::Hum), ("sparkle", Param::Sparkle)] {
            let out = render_offline(vec![(0.0, Cmd::Set(p, 1.0))], 0.5);
            let tail = &out[out.len() / 2..];
            let peak = tail.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
            println!("{name:>8}: peak {peak:.3}  steady rms {rms:.3} (at full level)");
        }
    }
}
