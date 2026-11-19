//! One display's overlay: where the mouse is, what the pointer and the effects are doing,
//! and what the HUD shows. `poll` runs before every frame; the compositor paints the result.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rune::{LiveText, Tx, Waker};

use crate::audio::{self, Param, Sfx};
use crate::draw::Scene;
use crate::fx::{Fx, Outcome, PEN};
use crate::input;
use crate::math::{V2, smoothstep, v2};
use crate::pointer::{self, Look};
use crate::settings::{Settings, Shared, Toast, Tool};
use crate::sim::{Rig, RigInput};
use crate::style::{BLADES, Style};

/// How far the sword's blade is out for how far the rig is: the hilt arrives, then it lights.
fn ignition(ext: f32) -> f32 {
    smoothstep(0.72, 1.0, ext)
}

/// The effect slots in the view: the glass under the HUD, the pointer over it.
pub const MARK_UNDER: u32 = 1;
pub const MARK_OVER: u32 = 2;

/// Where the mouse comes from.
#[derive(Clone, Copy)]
pub enum Source {
    /// The real, system-wide mouse.
    Live,
    /// Rune's own pointer events in this window (headless scripts).
    Window,
    /// A scripted path: where the mouse is at a time, and whether it's pressed.
    Path(fn(f64) -> (V2, bool)),
    /// The scripted tour in `demo.rs`, tool changes included.
    Demo,
    /// The scripted showcase of every pointer in `demo.rs`.
    Styles,
    /// A shot of the launch film in `promo.rs`.
    Promo(crate::promo::Shot),
}

/// What's behind a headless render, so a still shows the pointer in context.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backdrop {
    None,
    Light,
    Dark,
    /// The app icon's violet field, with ripples round the fingertip at this point.
    Icon(V2),
    /// The icon's field with the app's name: the social card links show.
    Card(V2),
}

#[derive(Default)]
pub struct Perf {
    frames: u32,
    window: f64,
    cpu: f64,
    worst_gap: f64,
    pub fps: f32,
    pub cpu_ms: f32,
}

pub struct Stage {
    pub index: usize,
    /// This display's top-left in global points.
    origin: (f64, f64),
    /// The viewport in points (the compositor keeps it current).
    size: (f32, f32),
    shared: Arc<Shared>,
    pub source: Source,
    pub backdrop: Backdrop,
    win_pointer: Option<V2>,
    win_down: bool,
    pub rig: Rig,
    pub fx: Fx,
    pub st: Settings,
    seen_rev: u64,
    seen_smash: u64,
    seen_clear: u64,
    seen_toast: u64,
    t: f64,
    last_t: Option<f64>,
    cursor: Option<V2>,
    last_sample: Option<(V2, bool)>,
    moved_t: f64,
    btn: bool,
    down_t: f64,
    down_at: V2,
    travel: f32,
    pub toast: Option<(Toast, f64)>,
    pub help: bool,
    /// The frame-rate readout shows on the display the cursor is on.
    pub perf_on: bool,
    pub perf: Perf,
    pub perf_text: LiveText,
    wants_frames: bool,
    resting: Arc<AtomicBool>,
    poked: bool,
    /// Lit ticks on the charge dial at the last frame, to tick once per new one.
    last_lit: i32,
    /// Whether the pole was heading out at the last frame, to sound its slide.
    was_out: bool,
    /// Whether the last change to the pointer's visibility came from its hotkey.
    toggled: bool,
    /// The continuous sounds' levels as last sent.
    levels: [f32; 6],
    /// The pointer being drawn: a new style waits for the old one to slide away.
    shown_style: Style,
    /// The sword's recent blade positions (emitter, tip, time), for its swing trail.
    trail: VecDeque<(V2, V2, f64)>,
    /// The sword flares after a click.
    flash: f32,
    /// The sword's hum eases down while it's held still.
    hum: f32,
    /// Holds the rig's hand still here (framed artwork: the icon and thumbnails).
    pub fixed_base: Option<V2>,
    /// Opens with the pointer already out and at rest (a film shot that starts mid-scene).
    pub start_out: bool,
}

impl Stage {
    pub fn new(index: usize, bounds: [f64; 4], shared: Arc<Shared>, source: Source) -> Stage {
        let st = shared.get();
        let st_style = st.style;
        Stage {
            index,
            origin: (bounds[0], bounds[1]),
            size: (bounds[2] as f32, bounds[3] as f32),
            seen_rev: st.rev,
            seen_smash: st.smash,
            seen_clear: st.clear,
            seen_toast: st.toast_rev,
            st,
            shared,
            source,
            backdrop: Backdrop::None,
            win_pointer: None,
            win_down: false,
            rig: Rig::default(),
            fx: Fx::default(),
            t: 0.0,
            last_t: None,
            cursor: None,
            last_sample: None,
            moved_t: 0.0,
            btn: false,
            down_t: 0.0,
            down_at: V2::ZERO,
            travel: 0.0,
            toast: None,
            help: false,
            perf_on: false,
            perf: Perf::default(),
            perf_text: LiveText::new("000 fps · 00.00 ms"),
            wants_frames: true,
            resting: Arc::new(AtomicBool::new(false)),
            poked: true,
            last_lit: 0,
            was_out: false,
            toggled: false,
            levels: [0.0; 6],
            shown_style: st_style,
            trail: VecDeque::new(),
            flash: 0.0,
            hum: 1.0,
            fixed_base: None,
            start_out: false,
        }
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
    pub fn clock(&self) -> f64 {
        self.t
    }
    pub fn set_viewport(&mut self, w: f32, h: f32) {
        if w > 0.0 && h > 0.0 { self.size = (w, h); }
    }
    pub fn wants_frames(&self) -> bool {
        self.wants_frames
    }
    /// The glove's size here, in points: a ninth of the display's height, scaled.
    pub fn glove_size(&self) -> f32 {
        (self.size.1 * 0.11).clamp(70.0, 420.0) * self.st.scale
    }
    /// Seconds since this stage started (virtual time when headless).
    pub fn time(&self) -> f64 {
        self.t
    }

    /// A HUD scale that keeps text a sensible size on big low-density displays.
    pub fn hud_scale(&self) -> f32 {
        (self.size.1 / 1100.0).clamp(1.0, 2.2)
    }

    fn sample(&self) -> Option<(V2, bool)> {
        match self.source {
            Source::Live => input::sample().map(|s| (v2((s.x - self.origin.0) as f32, (s.y - self.origin.1) as f32), s.left)),
            Source::Window => self.win_pointer.map(|p| (p, self.win_down)),
            Source::Path(f) => Some(f(self.t)),
            Source::Demo => { let (p, down, _) = crate::demo::script(self.t); Some((p, down)) }
            Source::Styles => { let (p, down, _) = crate::demo::styles(self.t); Some((p, down)) }
            Source::Promo(shot) => { let c = crate::promo::cue(shot, self.t); Some((c.at, c.down)) }
        }
    }

    /// Whether this display has the cursor (headless stages always do). Sounds come from here.
    fn here(&self) -> bool {
        self.cursor.is_some() || !matches!(self.source, Source::Live)
    }
    fn pan(&self, x: f32) -> f32 {
        ((x / self.size.0.max(1.0)) * 2.0 - 1.0) * 0.6
    }
    fn sound(&self, sfx: Sfx) {
        if self.st.sound { audio::play(self.t, sfx); }
    }
    /// Sets a continuous sound, sending only changes.
    fn level(&mut self, param: Param, value: f32) {
        let value = if self.st.sound { value } else { 0.0 };
        let last = &mut self.levels[param as usize];
        if (value - *last).abs() > 0.01 || (value == 0.0 && *last != 0.0) {
            *last = value;
            audio::set(self.t, param, value);
        }
    }

    pub fn pointer_moved(&mut self, p: V2) {
        self.win_pointer = Some(p);
    }
    pub fn pointer_button(&mut self, p: V2, down: bool) {
        self.win_pointer = Some(p);
        self.win_down = down;
    }

    fn apply(&mut self, new: Settings, tx: &mut Tx) {
        let now = self.t;
        let here = self.here();
        if new.sound != self.st.sound {
            audio::set_enabled(new.sound);
            if new.sound && here { audio::play(now, Sfx::Chime { pitch: 1.0 }); }
        }
        if new.tool != self.st.tool {
            if self.st.tool == Tool::Pen { self.fx.fade_ink(now); }
            if self.st.tool == Tool::Focus { self.fx.fade_focus(); }
            if self.btn { self.release(); }
            if here && new.shown {
                let sfx = match new.tool {
                    Tool::Spotlight => Sfx::Spot,
                    Tool::Tap => Sfx::Chime { pitch: 1.0 },
                    Tool::Laser => Sfx::Chime { pitch: 1.12 },
                    Tool::Pen => Sfx::Chime { pitch: 1.26 },
                    Tool::Focus => Sfx::Chime { pitch: 0.89 },
                };
                if new.sound { audio::play(now, sfx); }
            }
        }
        if new.shown != self.st.shown { self.toggled = true; }
        if new.smash != self.seen_smash {
            self.seen_smash = new.smash;
            if let Some(c) = self.cursor && self.rig.visible() {
                let k = self.glove_size() / 140.0;
                self.fx.crack(c, now, k, 1.0);
                self.rig.recoil(1.0);
                if new.sound { audio::play(now, Sfx::Crack { pan: self.pan(c.x), power: 1.0, seed: (now * 1000.0) as u32 }); }
            }
        }
        if new.clear != self.seen_clear {
            self.seen_clear = new.clear;
            self.fx.clear(now);
            if here && new.sound { audio::play(now, Sfx::Clear); }
        }
        if new.scale != self.st.scale && here && new.sound {
            audio::play(now, Sfx::Resize { bigger: new.scale > self.st.scale });
        }
        if (new.help != self.st.help || new.perf != self.st.perf || new.hide_cursor != self.st.hide_cursor) && here && new.sound {
            audio::play(now, Sfx::Tick { pan: 0.0, level: if new.help && !self.st.help { 0.9 } else { 0.4 } });
        }
        if new.toast_rev != self.seen_toast {
            self.seen_toast = new.toast_rev;
            if self.cursor.is_some() || !matches!(self.source, Source::Live) {
                self.toast = Some((new.toast.clone(), now + 2.0));
                tx.wake_after(Duration::from_secs_f64(2.05));
            }
        }
        let here = self.cursor.is_some() || !matches!(self.source, Source::Live);
        self.help = new.help && here;
        self.perf_on = new.perf && here;
        self.seen_rev = new.rev;
        self.st = new;
    }

    fn release(&mut self) {
        self.btn = false;
        match self.st.tool {
            Tool::Pen => self.fx.end_stroke(),
            Tool::Focus if self.fx.end_focus() => self.sound(Sfx::Focus { pan: self.pan(self.down_at.x) }),
            _ => {}
        }
    }

    /// One frame's worth of input, physics and effects. True when the view must re-run.
    pub fn step(&mut self, t: f64, tx: &mut Tx) -> bool {
        let dt = self.last_t.map_or(0.0, |l| (t - l).max(0.0)) as f32;
        self.last_t = Some(t);
        self.t = t;
        let was_resting = !self.wants_frames;
        let mut changed = std::mem::take(&mut self.poked);

        if let Source::Demo = self.source {
            let (_, _, tool) = crate::demo::script(t);
            if tool != self.st.tool { crate::actions::set_tool_on(&self.shared, tool); }
        }
        if let Source::Styles = self.source {
            let (_, _, style) = crate::demo::styles(t);
            if style != self.st.style { crate::actions::set_style_on(&self.shared, style); }
        }
        if let Source::Promo(shot) = self.source {
            // The film switches tools and pointers quietly (it has its own titles), and its
            // titles and cards change with time, so the view runs every frame.
            let cue = crate::promo::cue(shot, t);
            let now = self.shared.get();
            if (cue.tool, cue.style, cue.shown) != (now.tool, now.style, now.shown) {
                self.shared.update(|s| {
                    if cue.style == Style::Saber && s.style != Style::Saber { s.blade = crate::promo::BLADE; }
                    (s.tool, s.style, s.shown) = (cue.tool, cue.style, cue.shown);
                });
            }
            changed = true;
        }
        let (w, h) = self.size;
        let sample = self.sample();
        let on = sample.filter(|(p, _)| p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h);
        if matches!(self.source, Source::Live) && on.is_some() && sample.map(|s| s.1) != self.last_sample.map(|s| s.1) {
            input::cursor_click();
        }
        if sample != self.last_sample {
            self.last_sample = sample;
            self.moved_t = t;
            // Moving the pointer gets the audio output ready for a click.
            if matches!(self.source, Source::Live) && on.is_some() && self.st.sound && self.st.shown { audio::prepare(); }
        }
        let was_on = self.cursor.is_some();
        self.cursor = on.map(|(p, _)| p);
        // Settings after the mouse, so a toast or a smash lands on the display it's on now.
        if self.shared.rev() != self.seen_rev {
            let new = self.shared.get();
            self.apply(new, tx);
            changed = true;
        } else if was_on != self.cursor.is_some() {
            // The help card and the readout follow the cursor between displays.
            let here = self.cursor.is_some() || !matches!(self.source, Source::Live);
            let (help, perf) = (self.st.help && here, self.st.perf && here);
            if (help, perf) != (self.help, self.perf_on) { (self.help, self.perf_on) = (help, perf); changed = true; }
        }
        let pressed = on.is_some_and(|(_, down)| down) && self.st.shown;
        let size = self.glove_size();
        let k = size / 140.0;
        // A new pointer waits until the old one has slid away (or was never out).
        if self.shown_style != self.st.style && (!self.rig.visible() || self.rig.ext.x < 0.03) {
            self.shown_style = self.st.style;
            self.trail.clear();
        }
        let swapping = self.shown_style != self.st.style;
        let tint = BLADES[self.st.blade % BLADES.len()];

        if let Some(c) = self.cursor {
            if pressed && !self.btn {
                self.btn = true;
                self.down_t = t;
                self.down_at = c;
                self.travel = 0.0;
                let pan = self.pan(c.x);
                match self.st.tool {
                    Tool::Pen => {
                        self.fx.begin_stroke(c, PEN, 4.2 * k);
                        self.sound(Sfx::PenDown { pan });
                    }
                    Tool::Focus => self.fx.begin_focus(c),
                    _ => {
                        let seed = (t * 977.0) as u32 ^ (c.x as u32).wrapping_mul(31);
                        let style = self.shown_style;
                        if style == Style::Saber { self.flash = 1.0; }
                        // The tap that breaks the glass sounds like breaking glass.
                        if let Outcome::Cracked { power } = self.fx.click(c, t, k, style, tint) {
                            self.rig.recoil(power);
                            self.sound(Sfx::Crack { pan, power, seed });
                        } else {
                            self.sound(match style {
                                Style::Glove => Sfx::Tap { pan, seed },
                                Style::Saber => Sfx::Clash { pan },
                                Style::Wand => Sfx::Cast { pan, seed },
                                Style::Mallet => Sfx::Bonk { pan },
                                Style::Chicken => Sfx::Squawk { pan, seed },
                            });
                        }
                    }
                }
            } else if self.btn && pressed {
                self.travel = self.travel.max((c - self.down_at).len());
                match self.st.tool {
                    Tool::Pen => self.fx.extend_stroke(c),
                    Tool::Focus => self.fx.drag_focus(c),
                    _ => {
                        if let Outcome::Cracked { .. } = self.fx.hold(c, t - self.down_t, self.travel, dt, t, k) {
                            self.rig.recoil(1.0);
                            self.sound(Sfx::Crack { pan: self.pan(c.x), power: 1.0, seed: (t * 977.0) as u32 });
                        }
                    }
                }
            }
        }
        if self.btn && !pressed { self.release(); }

        let input = RigInput { w, h, cursor: self.cursor, pressed: self.btn, shown: self.st.shown && !swapping, size, base: self.fixed_base };
        if self.start_out && self.rig.start_out(&input) { self.was_out = true; }
        self.rig.step(dt, &input);
        let pose = self.rig.pose_with(self.shown_style.attach());
        self.flash *= (-dt * 7.0).exp();
        if self.shown_style == Style::Saber && pose.visible {
            let (emitter, full) = pointer::blade_root(&pose);
            self.trail.push_back((emitter, emitter - pose.down * (full * ignition(self.rig.ext.x)), t));
            while self.trail.front().is_some_and(|(_, _, at)| t - at > 0.085) { self.trail.pop_front(); }
        } else {
            self.trail.clear();
        }
        if self.shown_style == Style::Wand && self.cursor.is_some() && self.rig.ext.x > 0.6 {
            self.fx.sparkle(pose.tip, self.rig.vel, t, dt, k);
        }
        if self.st.tool == Tool::Laser && self.cursor.is_some() && self.rig.ext.x > 0.5 { self.fx.trail_point(pose.tip, t); }
        let spot = self.st.tool == Tool::Spotlight && self.st.shown && self.cursor.is_some();
        self.fx.step(dt, t, spot, self.btn);
        self.sounds(k);

        if matches!(self.source, Source::Live) && self.cursor.is_some() {
            input::set_cursor_hidden(self.st.hide_cursor && self.st.shown && self.rig.ext.x > 0.55);
        }

        if self.toast.as_ref().is_some_and(|(_, until)| t >= *until) {
            self.toast = None;
            changed = true;
        }

        let moving = !self.rig.settled() || t - self.moved_t < 0.4;
        self.wants_frames = (self.rig.visible() && moving) || self.fx.animating() || self.btn || swapping || self.flash > 0.01;
        if !self.wants_frames && !was_resting && self.perf_on { self.perf_text.set("resting · 0 fps"); }
        // Rest, and let the sampler bring us back when the mouse moves here. A hidden pointer
        // has nothing to follow, so it isn't woken for every move.
        self.resting.store(!self.wants_frames && self.st.shown, Ordering::Relaxed);
        if self.wants_frames && was_resting { changed = true; }
        changed
    }

    /// The sounds that follow state rather than events: the pole sliding in and out, the
    /// dial's ticks, and the continuous charge, laser and pen sounds.
    fn sounds(&mut self, k: f32) {
        let here = self.here();
        let out = self.rig.extending();
        if out != self.was_out {
            self.was_out = out;
            if here {
                let gain = if std::mem::take(&mut self.toggled) { 1.0 } else { 0.55 };
                self.sound(match (self.shown_style, out) {
                    (Style::Saber, true) => Sfx::Ignite,
                    (Style::Saber, false) => Sfx::Unignite,
                    (Style::Wand, up) => Sfx::Shimmer { up },
                    (Style::Mallet, up) => Sfx::Whoosh { up },
                    (_, true) => Sfx::Extend { gain },
                    (_, false) => Sfx::Retract { gain },
                });
            }
        }
        let charge = self.fx.dial_charge();
        let lit = (charge * 12.0).floor() as i32;
        if lit > self.last_lit && here {
            let pan = self.cursor.map_or(0.0, |c| self.pan(c.x));
            self.sound(Sfx::Tick { pan, level: lit as f32 / 12.0 });
        }
        self.last_lit = lit;
        let speed = self.rig.vel.len() / (1500.0 * k.max(0.1));
        let laser = self.st.tool == Tool::Laser && self.cursor.is_some() && self.rig.ext.x > 0.5;
        let pen = self.st.tool == Tool::Pen && self.btn;
        self.level(Param::Charge, if here { charge } else { 0.0 });
        self.level(Param::Laser, if laser { (speed * 1.2).min(1.0) } else { 0.0 });
        self.level(Param::Pen, if pen { (speed * 1.5).min(1.0) } else { 0.0 });
        // The sword hums while lit, louder as it swings, and settles to a murmur held still.
        let saber = self.shown_style == Style::Saber && self.cursor.is_some() && self.st.shown;
        let target = if speed > 0.08 { 1.0 } else { 0.3 };
        self.hum += (target - self.hum) * if target > self.hum { 0.3 } else { 0.03 };
        let lit = ignition(self.rig.ext.x);
        self.level(Param::Hum, if saber { lit * self.hum } else { 0.0 });
        self.level(Param::Swing, if saber { (speed * 1.3).min(1.0) * lit } else { 0.0 });
        let wand = self.shown_style == Style::Wand && self.cursor.is_some() && self.rig.ext.x > 0.6;
        self.level(Param::Sparkle, if wand { (speed * 1.4).min(1.0) } else { 0.0 });
    }

    /// Paints the frame into `scene`.
    pub fn paint(&mut self, scene: &mut Scene) {
        let (w, h) = self.size;
        let pose = self.rig.pose_with(self.shown_style.attach());
        let k = self.glove_size() / 140.0;
        {
            let mut under = scene.under();
            self.fx.draw_dim(&mut under, w, h, pose.tip, self.glove_size() * 1.3);
            self.fx.draw_glass(&mut under, self.t, k);
            if pose.visible { self.fx.draw_dial(&mut under, pose.tip, k, self.t); }
        }
        let trail: Vec<(V2, V2)> = self.trail.iter().map(|(e, tip, _)| (*e, *tip)).collect();
        let look = Look {
            style: self.shown_style,
            blade: BLADES[self.st.blade % BLADES.len()],
            ignition: ignition(self.rig.ext.x),
            trail: &trail,
            time: self.t as f32,
            flash: self.flash,
        };
        let mut over = scene.over();
        pointer::draw(&mut over, &pose, &look, h);
        self.fx.draw_above(&mut over, self.t);
    }

    /// The compositor reports each frame it drew, for the frame-rate readout.
    pub fn frame_drawn(&mut self, gap: Option<f64>, cpu_ms: f64, _prims: usize, _verts: usize) {
        let p = &mut self.perf;
        // The first frame after a rest isn't a slow frame: leave it out of the rates.
        let Some(g) = gap.filter(|g| *g < 0.05) else { return };
        p.frames += 1;
        p.cpu += cpu_ms;
        p.window += g;
        p.worst_gap = p.worst_gap.max(g);
        if p.window >= 0.5 {
            p.fps = (p.frames as f64 / p.window) as f32;
            p.cpu_ms = (p.cpu / p.frames as f64) as f32;
            if self.st.perf {
                self.perf_text.set(format!("{:>3.0} fps · {:>5.2} ms", p.fps, p.cpu_ms));
            }
            if std::env::var_os("POINTER_DEBUG").is_some() {
                eprintln!("pointer[{}]: {:.1} fps, cpu {:.3} ms/frame, worst gap {:.1} ms", self.index, p.fps, p.cpu_ms, p.worst_gap * 1000.0);
            }
            p.frames = 0;
            p.window = 0.0;
            p.cpu = 0.0;
            p.worst_gap = 0.0;
        }
    }

    /// Hands the sampler thread what it needs to wake this overlay when the mouse comes back
    /// to its display while it rests.
    pub fn watch(&self, waker: Waker) -> Watch {
        Watch { origin: self.origin, size: self.size, resting: self.resting.clone(), waker }
    }
}

/// A resting overlay the sampler wakes when the mouse moves on its display.
pub struct Watch {
    origin: (f64, f64),
    size: (f32, f32),
    resting: Arc<AtomicBool>,
    waker: Waker,
}

static WATCHES: std::sync::Mutex<Vec<Watch>> = std::sync::Mutex::new(Vec::new());
static SAMPLER: std::sync::Once = std::sync::Once::new();

/// Registers a display's overlay with the sampler: a thread that reads the mouse a few
/// hundred times a second and wakes a resting overlay when the mouse moves or clicks on its
/// display. Overlays that are drawing read the mouse themselves, every frame.
pub fn watch(w: Watch) {
    WATCHES.lock().unwrap_or_else(|e| e.into_inner()).push(w);
    SAMPLER.call_once(|| {
        std::thread::Builder::new().name("pointer-sampler".into()).spawn(|| {
            let mut last: Option<input::Sample> = None;
            let mut was_inside: Vec<bool> = Vec::new();
            let mut checked = std::time::Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(4));
                let Some(s) = input::sample() else { continue };
                // A few times a second: if something showed the system cursor while the glove
                // stands in for it, wake the overlay with the cursor, which hides it again.
                let escaped = checked.elapsed() >= Duration::from_millis(100) && {
                    checked = std::time::Instant::now();
                    input::cursor_escaped()
                };
                if last == Some(s) && !escaped { continue; }
                last = Some(s);
                let watches = WATCHES.lock().unwrap_or_else(|e| e.into_inner());
                was_inside.resize(watches.len(), false);
                for (w, before) in watches.iter().zip(was_inside.iter_mut()) {
                    let (x, y) = (s.x - w.origin.0, s.y - w.origin.1);
                    let inside = x >= 0.0 && y >= 0.0 && x < w.size.0 as f64 && y < w.size.1 as f64;
                    // Moving or clicking on a display wakes it; so does leaving it, so its
                    // pointer can slide away. The one with the cursor also wakes to hide the
                    // system cursor again.
                    let crossed = inside != std::mem::replace(before, inside);
                    let woke = (inside || crossed) && w.resting.swap(false, Ordering::Relaxed);
                    if woke || (inside && escaped) { (w.waker)(); }
                }
            }
        }).expect("sampler thread");
    });
}

pub fn poll(s: &mut Stage, t: f64, tx: &mut Tx) -> bool {
    s.step(t, tx)
}
