//! The launch film: shots the app performs headlessly on a 1920×1080 stage, each a scripted
//! run of the mouse with its own backdrop and titles. The settings window is filmed on its
//! own, over the `Wall` shot's still. `scripts/promo.sh` renders them and cuts the film.

use std::f32::consts::TAU;

use rune::{Color, El, Icon, TextStyle, Transition, col, el, icon, row, text};

use crate::hud::{full, keycap, srgb, tool_color, white};
use crate::math::{V2, smoothstep, v2};
use crate::settings::Tool;
use crate::style::Style;

/// The laser sword's colour in the film (blue).
pub const BLADE: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shot {
    /// A screen share where nobody can find the cursor.
    Hook,
    /// The name, and the glove tapping it.
    Reveal,
    /// Taps, then five quick ones crack the glass.
    Tap,
    /// Spotlight, laser, pen and focus.
    Tools,
    /// Every pointer in turn.
    Pointers,
    /// What it's made of, card by card.
    Tech,
    /// Where to get it.
    Cta,
    /// The still behind the settings window's shot.
    Wall,
}

/// The film's shots in order, by the names their scenes take (`promo-<name>`). The settings
/// window's shot goes between `pointers` and `tech`.
pub const SHOTS: [(&str, Shot); 7] = [
    ("hook", Shot::Hook),
    ("reveal", Shot::Reveal),
    ("tap", Shot::Tap),
    ("tools", Shot::Tools),
    ("pointers", Shot::Pointers),
    ("tech", Shot::Tech),
    ("cta", Shot::Cta),
];

impl Shot {
    /// The shot's length in seconds; the cut follows these.
    pub const fn length(self) -> f32 {
        match self {
            Shot::Hook => 8.0,
            Shot::Reveal => 5.0,
            Shot::Tap => 6.5,
            Shot::Tools => 12.5,
            Shot::Pointers => 12.0,
            Shot::Tech => 9.0,
            Shot::Cta => 6.5,
            Shot::Wall => 1.0,
        }
    }

    /// The pointer's size setting in the shot.
    pub const fn scale(self) -> f32 {
        match self {
            Shot::Reveal => 1.7,
            Shot::Cta => 1.5,
            Shot::Tech => 1.25,
            _ => 1.35,
        }
    }

    /// Whether the shot opens with the pointer already out (cut in mid-scene).
    pub const fn starts_out(self) -> bool {
        matches!(self, Shot::Tap | Shot::Tools | Shot::Pointers | Shot::Tech | Shot::Cta)
    }
}

/// What the mouse and the app are doing at a moment of a shot.
#[derive(Clone, Copy, Debug)]
pub struct Cue {
    pub at: V2,
    pub down: bool,
    pub tool: Tool,
    pub style: Style,
    pub shown: bool,
}

/// Where the mouse is at `t`, gliding between key positions `(time, x, y)`.
fn glide(keys: &[(f32, f32, f32)], t: f32) -> V2 {
    let i = keys.partition_point(|k| k.0 <= t).clamp(1, keys.len() - 1);
    let (t0, x0, y0) = keys[i - 1];
    let (t1, x1, y1) = keys[i];
    let e = smoothstep(t0, t1, t);
    v2(x0 + (x1 - x0) * e, y0 + (y1 - y0) * e)
}

fn pressed(presses: &[(f32, f32)], t: f32) -> bool {
    presses.iter().any(|(a, b)| t >= *a && t < *b)
}

fn moment(at: V2, down: bool, tool: Tool, style: Style, shown: bool) -> Cue {
    Cue { at, down, tool, style, shown }
}

// The sample slide's layout (see `hud::backdrop`): chips at y 458 to 495, tiles at y 553 to 703.
const RETENTION: V2 = v2(905.0, 478.0);
const ORANGE: V2 = v2(826.0, 628.0);
const GREEN: V2 = v2(1094.0, 628.0);

// The tech shot's five cards.
const CARD_W: f32 = 320.0;
const CARD_GAP: f32 = 24.0;
const CARD_X0: f32 = (1920.0 - (CARD_W * 5.0 + CARD_GAP * 4.0)) / 2.0;
const CARD_Y: f32 = 420.0;
const CARD_H: f32 = 250.0;
// Each card lands on its word in the voiceover.
const CARDS: [(f32, &str, &str); 5] = [
    (1.6, "Rust", "Native code"),
    (2.6, "120", "Frames a second"),
    (4.6, "0", "Permissions to grant"),
    (5.55, "0", "Accounts to make"),
    (6.5, "MIT", "Open source"),
];

/// Where the glove taps card `i` of the tech shot: low on its right, so the words stay clear.
fn card_tap(i: usize) -> V2 {
    v2(CARD_X0 + i as f32 * (CARD_W + CARD_GAP) + CARD_W - 64.0, CARD_Y + CARD_H - 38.0)
}

// The call to action's link, where the glove taps it.
const LINK_TAP: V2 = v2(1262.0, 692.0);
// The reveal's wordmark, where the glove taps it.
const MARK_TAP: V2 = v2(1440.0, 560.0);

/// What's happening at `t` seconds into `shot`.
pub fn cue(shot: Shot, t: f64) -> Cue {
    let t = t as f32;
    let (glove, tap) = (Style::Glove, Tool::Tap);
    match shot {
        Shot::Hook => {
            // The system cursor, tiny, wandering a slide; then the "see this?" wiggle.
            let mut p = glide(&[(0.0, 1250.0, 650.0), (1.0, 1250.0, 650.0), (2.4, 760.0, 600.0), (2.9, 880.0, 490.0)], t);
            if t > 2.9 {
                let a = smoothstep(2.9, 3.3, t) * (1.0 - smoothstep(5.0, 5.6, t));
                p = RETENTION + v2(42.0 * (t * 29.0).sin(), 16.0 * (t * 21.0).sin()) * a;
                if t > 5.0 { p = p.lerp(v2(1180.0, 770.0), smoothstep(5.2, 7.4, t)); }
            }
            moment(p, false, tap, glove, false)
        }
        Shot::Reveal => {
            let p = glide(&[(0.0, MARK_TAP.x, MARK_TAP.y), (3.2, MARK_TAP.x, MARK_TAP.y), (4.4, MARK_TAP.x - 26.0, MARK_TAP.y - 14.0)], t);
            moment(p, pressed(&[(2.4, 2.5)], t), tap, glove, t > 0.35)
        }
        Shot::Tap => {
            let p = glide(&[
                (0.0, 1100.0, 760.0), (0.15, 1100.0, 760.0), (0.75, RETENTION.x, RETENTION.y), (1.4, RETENTION.x, RETENTION.y),
                (1.9, ORANGE.x, ORANGE.y), (2.5, ORANGE.x, ORANGE.y), (3.3, 690.0, 790.0), (6.5, 720.0, 805.0),
            ], t);
            moment(p, pressed(&[(0.95, 1.05), (2.1, 2.2), (3.9, 3.97), (4.1, 4.17), (4.3, 4.37), (4.5, 4.57), (4.7, 4.77)], t), tap, glove, true)
        }
        Shot::Tools => {
            let tool = match t {
                t if t < 3.1 => Tool::Spotlight,
                t if t < 6.2 => Tool::Laser,
                t if t < 9.3 => Tool::Pen,
                _ => Tool::Focus,
            };
            let mut p = glide(&[
                (0.0, 1000.0, 700.0), (0.3, 1000.0, 700.0), (1.3, 1160.0, 430.0), (2.3, 1250.0, 640.0), (3.0, 980.0, 560.0),
                (3.5, 520.0, 362.0), (3.6, 520.0, 362.0), (4.9, 1400.0, 362.0), (5.4, 1320.0, 500.0), (6.2, 1240.0, 700.0),
                // (The pen's circle, 6.2 to 9.0, is drawn below.)
                (9.0, 1300.0, 760.0), (9.3, 1300.0, 760.0),
                (9.8, 690.0, 540.0), (10.0, 690.0, 540.0), (11.0, 1232.0, 716.0), (12.5, 1250.0, 740.0),
            ], t);
            // The laser's swoosh under the title, and the pen's circle round the green tile.
            if (3.6..4.9).contains(&t) {
                let u = (t - 3.6) / 1.3;
                p = v2(520.0 + 880.0 * u, 362.0 + 26.0 * (u * TAU * 1.5).sin() * (1.0 - u * 0.4));
            }
            let ring = |a: f32| GREEN + v2(178.0 * a.cos(), 112.0 * a.sin());
            if (6.2..9.0).contains(&t) {
                let start = -1.9;
                p = if t < 6.6 {
                    v2(1240.0, 700.0).lerp(ring(start), smoothstep(6.2, 6.6, t))
                } else if t < 8.2 {
                    ring(start + smoothstep(6.7, 8.1, t) * TAU * 1.08)
                } else {
                    ring(start + TAU * 1.08).lerp(v2(1300.0, 760.0), smoothstep(8.2, 9.0, t))
                };
            }
            moment(p, pressed(&[(6.7, 8.1), (10.0, 11.0)], t), tool, glove, true)
        }
        Shot::Pointers => {
            // Each pointer in turn: it slides in, loops out to the left and back, and clicks twice.
            const TURN: f32 = 2.4;
            let turn = ((t / TURN) as usize).min(4);
            let u = t - turn as f32 * TURN;
            let home = v2(1140.0, 560.0);
            let a = smoothstep(0.85, 1.6, u) * TAU;
            let mut p = home + v2(-330.0 * (a / 2.0).sin(), -150.0 * a.sin());
            if u > 1.6 { p += v2(6.0 * ((u - 1.6) * 18.0).sin(), 0.0); }
            moment(p, pressed(&[(1.7, 1.78), (2.05, 2.13)], u), tap, Style::ALL[turn], true)
        }
        Shot::Tech => {
            let mut keys = vec![(0.0, card_tap(0).x, card_tap(0).y + 40.0)];
            let mut presses = Vec::new();
            for (i, (at, _, _)) in CARDS.iter().enumerate() {
                let p = card_tap(i);
                keys.push((at - 0.45, p.x, p.y));
                keys.push((at + 0.35, p.x, p.y));
                presses.push((at + 0.1, at + 0.18));
            }
            keys.push((9.0, card_tap(4).x - 40.0, card_tap(4).y + 30.0));
            moment(glide(&keys, t), pressed(&presses, t), tap, glove, true)
        }
        Shot::Cta => {
            let p = glide(&[(0.0, LINK_TAP.x + 30.0, LINK_TAP.y + 40.0), (1.2, LINK_TAP.x, LINK_TAP.y), (6.5, LINK_TAP.x + 8.0, LINK_TAP.y + 6.0)], t);
            moment(p, pressed(&[(1.6, 1.7), (3.7, 3.8)], t), tap, glove, true)
        }
        Shot::Wall => moment(v2(-100.0, -100.0), false, tap, glove, false),
    }
}

// ---------------------------------------------------------------------------------- pictures

fn violet() -> Vec<(f32, Color)> {
    vec![(0.0, srgb(0.56, 0.44, 1.0, 1.0)), (0.55, srgb(0.36, 0.22, 0.93, 1.0)), (1.0, srgb(0.16, 0.08, 0.5, 1.0))]
}

fn night() -> Vec<(f32, Color)> {
    vec![(0.0, srgb(0.13, 0.09, 0.27, 1.0)), (0.6, srgb(0.06, 0.05, 0.11, 1.0)), (1.0, srgb(0.03, 0.03, 0.05, 1.0))]
}

/// A soft light behind a spot.
fn glow(at: V2, r: f32, a: f32) -> El {
    el().size(r * 2.0, r * 2.0).pin(Some(at.x - r), Some(at.y - r), None, None)
        .radial_gradient([0.5, 0.5], r, vec![(0.0, white(a)), (0.5, white(a * 0.3)), (1.0, white(0.0))])
}

/// Text that rises in at `from` seconds (absent before, so its entrance animates).
fn rise(t: f32, from: f32, key: &str, e: El) -> Option<El> {
    (t >= from).then(|| el().key(key).child(e).transition(Transition::offset(0.0, 26.0).combined(Transition::opacity())))
}

/// The backdrop for `shot` at `t`, under the pointer's effects.
pub fn backdrop(shot: Shot, t: f32) -> Option<El> {
    match shot {
        Shot::Reveal => {
            let mut words = col().key("reveal-words").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).justify_center().items_center().gap(4.0).pb(20.0);
            if let Some(e) = rise(t, 0.15, "mark", text("Pointer").font(TextStyle::new(232.0, 800.0)).color(white(1.0))) { words = words.child(e); }
            if let Some(e) = rise(t, 0.7, "tagline", text("A giant pointer for your screen.").font(TextStyle::new(58.0, 600.0)).color(white(0.92))) { words = words.child(e); }
            Some(full().key("reveal").linear_gradient(2.4, violet()).child(glow(MARK_TAP, 520.0, 0.30)).child(words))
        }
        Shot::Tech => {
            let mut head = col().key("tech-head").pin(Some(0.0), Some(170.0), Some(0.0), None).items_center().gap(14.0);
            if let Some(e) = rise(t, 0.2, "eyebrow", text("UNDER THE HOOD").font(TextStyle::new(24.0, 700.0)).color(srgb(0.79, 0.74, 1.0, 1.0))) { head = head.child(e); }
            if let Some(e) = rise(t, 0.4, "headline", text("Native. Fast. Private.").font(TextStyle::new(84.0, 800.0)).color(white(1.0))) { head = head.child(e); }
            let mut back = full().key("tech").linear_gradient(2.6, night()).child(glow(v2(960.0, 300.0), 760.0, 0.10)).child(head);
            for (i, (at, big, small)) in CARDS.iter().enumerate() {
                if t < *at { continue; }
                let x = CARD_X0 + i as f32 * (CARD_W + CARD_GAP);
                let lit = (t - at) < 1.2;
                back = back.child(
                    col().key(format!("card{i}")).size(CARD_W, CARD_H).pin(Some(x), Some(CARD_Y), None, None).padding(30.0).gap(10.0).radius(28.0)
                        .fill(if lit { srgb(0.36, 0.25, 0.9, 0.38) } else { white(0.06) })
                        .border(1.5, if lit { srgb(0.64, 0.55, 1.0, 0.9) } else { white(0.12) })
                        .child(text(*big).font(TextStyle::new(92.0, 800.0).features("'tnum' 1")).color(white(1.0)))
                        .child(text(*small).font(TextStyle::new(26.0, 550.0)).color(white(0.72)).wrap().w_full())
                        .transition(Transition::offset(0.0, 34.0).combined(Transition::opacity())),
                );
            }
            Some(back)
        }
        Shot::Cta => {
            let mut words = col().key("cta-words").pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).justify_center().items_center().gap(14.0).pb(40.0);
            if let Some(e) = rise(t, 0.1, "mark", text("Pointer").font(TextStyle::new(190.0, 800.0)).color(white(1.0))) { words = words.child(e); }
            if let Some(e) = rise(t, 0.35, "free", text("Free for Mac.").font(TextStyle::new(60.0, 650.0)).color(white(0.95))) { words = words.child(e.mb(30.0)); }
            if let Some(e) = rise(t, 0.6, "link", row().px(40.0).py(22.0).radius(46.0).fill(white(0.96)).shadow(14.0, 40.0, Color { r: 0.05, g: 0.0, b: 0.2, a: 0.45 })
                .child(text("nearbycoder.github.io/pointer").font(TextStyle::new(40.0, 650.0)).color(srgb(0.21, 0.1, 0.62, 1.0)))) { words = words.child(e.mb(14.0)); }
            if let Some(e) = rise(t, 0.85, "source", text("Open source on GitHub · MIT").font(TextStyle::new(26.0, 550.0)).color(white(0.75))) { words = words.child(e); }
            Some(full().key("cta").linear_gradient(2.4, violet()).child(glow(LINK_TAP, 560.0, 0.22)).child(words))
        }
        Shot::Wall => Some(full().key("wall").linear_gradient(2.5, violet()).child(glow(v2(960.0, 540.0), 900.0, 0.16))
            .child(col().pin(Some(0.0), Some(70.0), Some(0.0), None).items_center()
                .child(text("Every hotkey, yours.").font(TextStyle::new(64.0, 800.0)).color(white(1.0))))
            .child(col().pin(Some(0.0), None, Some(0.0), Some(64.0)).items_center()
                .child(text("Click a shortcut, press new keys. It applies at once.").font(TextStyle::new(28.0, 550.0)).color(white(0.82))))),
        Shot::Hook | Shot::Tap | Shot::Tools | Shot::Pointers => None,
    }
}

/// A title in the top-left corner: words, an accent dot, and the hotkey if there is one.
fn title(key: &str, words: &str, accent: Color, keys: &str) -> El {
    let mut pill = row().key(key).items_center().gap(18.0).pl(26.0).pr(if keys.is_empty() { 34.0 } else { 20.0 }).py(18.0).radius(26.0)
        .fill(srgb(0.07, 0.06, 0.11, 0.9)).border(1.0, white(0.12)).shadow(12.0, 38.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 })
        .child(el().size(14.0, 14.0).radius(7.0).fill(accent))
        .child(text(words.to_string()).font(TextStyle::new(46.0, 750.0)).color(white(0.98)));
    if !keys.is_empty() { pill = pill.child(keycap(keys, 1.6)); }
    pill.transition(Transition::offset(-30.0, 0.0).combined(Transition::opacity()))
}

/// The titles a shot shows: (from, until, words, accent, hotkey).
fn titles(shot: Shot) -> &'static [(f32, f32, &'static str, Accent, &'static str)] {
    match shot {
        Shot::Hook => &[
            (0.4, 2.1, "You're sharing your screen.", Accent::Rec, ""),
            (2.2, 4.3, "“See that button?”", Accent::Rec, ""),
            (4.4, 8.0, "…where's the cursor?", Accent::Rec, ""),
        ],
        Shot::Tap => &[(0.3, 2.3, "Tap to make a point", Accent::Tool(Tool::Tap), ""), (2.4, 6.5, "…or crack the glass", Accent::Tool(Tool::Tap), "")],
        Shot::Tools => &[
            (0.15, 3.0, "Spotlight", Accent::Tool(Tool::Spotlight), "⌃⌥2"),
            (3.15, 6.1, "Laser", Accent::Tool(Tool::Laser), "⌃⌥3"),
            (6.25, 9.2, "Pen", Accent::Tool(Tool::Pen), "⌃⌥4"),
            (9.35, 12.5, "Focus", Accent::Tool(Tool::Focus), "⌃⌥5"),
        ],
        Shot::Pointers => &[
            (0.1, 2.35, "Glove", Accent::Style(Style::Glove), "⌃⌥S"),
            (2.5, 4.75, "Laser sword", Accent::Style(Style::Saber), "⌃⌥S"),
            (4.9, 7.15, "Magic wand", Accent::Style(Style::Wand), "⌃⌥S"),
            (7.3, 9.55, "Mallet", Accent::Style(Style::Mallet), "⌃⌥S"),
            (9.7, 12.0, "Rubber chicken", Accent::Style(Style::Chicken), "⌃⌥S"),
        ],
        Shot::Reveal | Shot::Tech | Shot::Cta | Shot::Wall => &[],
    }
}

#[derive(Clone, Copy)]
enum Accent {
    Rec,
    Tool(Tool),
    Style(Style),
}

impl Accent {
    fn color(self) -> Color {
        match self {
            Accent::Rec => srgb(1.0, 0.27, 0.23, 1.0),
            Accent::Tool(tool) => tool_color(tool),
            Accent::Style(Style::Saber) => srgb(0.35, 0.65, 1.0, 1.0),
            Accent::Style(style) => { let [r, g, b] = style.accent(); srgb(r, g, b, 1.0) }
        }
    }
}

/// The macOS arrow, for the hook's lost cursor (tip at 5, 2.5 in its 24-point box).
static ARROW: Icon = Icon {
    name: "promo-arrow",
    view: 24.0,
    body: r##"<path d="M5 2.5v17l4.1-3.9 2.8 6.4 2.8-1.2-2.8-6.3h5.9z" fill="#111" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/>"##,
};

/// What `shot` draws over the effects and under the pointer at `t`: its titles, and the
/// hook's screen-share bar and tiny cursor.
pub fn overlay(shot: Shot, t: f32) -> El {
    let mut layer = full().key("promo-overlay");
    if shot == Shot::Hook {
        let bar = row().key("sharing").items_center().gap(14.0).pl(20.0).pr(8.0).py(8.0).radius(22.0)
            .fill(srgb(0.1, 0.1, 0.12, 0.92)).shadow(6.0, 22.0, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.3 })
            .child(el().size(12.0, 12.0).radius(6.0).fill(srgb(1.0, 0.27, 0.23, 1.0)))
            .child(text("You're sharing your screen").font(TextStyle::new(19.0, 600.0)).color(white(0.95)))
            .child(row().px(14.0).py(7.0).radius(16.0).fill(srgb(0.86, 0.2, 0.18, 1.0))
                .child(text("Stop").font(TextStyle::new(17.0, 650.0)).color(white(1.0))));
        layer = layer.child(col().key("sharing-wrap").pin(Some(0.0), Some(26.0), Some(0.0), None).items_center().child(bar));
        let p = cue(shot, f64::from(t)).at;
        let size = 30.0;
        layer = layer.child(icon(&ARROW, size).key("tiny-cursor").pin(Some(p.x - 5.0 * size / 24.0), Some(p.y - 2.5 * size / 24.0), None, None));
    }
    let mut corner = col().key("titles").pin(Some(72.0), Some(64.0), None, None).items_start();
    for (from, until, words, accent, keys) in titles(shot) {
        if t >= *from && t < *until {
            corner = corner.child(title(words, words, accent.color(), keys));
        }
    }
    layer.child(corner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shot_has_a_cue_through_its_length() {
        for (_, shot) in SHOTS {
            let n = (shot.length() * 60.0) as usize;
            for f in 0..=n {
                let c = cue(shot, f as f64 / 60.0);
                assert!(c.at.x.is_finite() && c.at.y.is_finite(), "{shot:?} at frame {f}");
                assert!((-200.0..2200.0).contains(&c.at.x) && (-200.0..1300.0).contains(&c.at.y), "{shot:?} at frame {f}: {:?}", c.at);
            }
        }
    }

    #[test]
    fn the_pointers_shot_shows_all_five() {
        let seen: Vec<Style> = (0..5).map(|i| cue(Shot::Pointers, f64::from(i as f32 * 2.4 + 1.0)).style).collect();
        assert_eq!(seen, Style::ALL.to_vec());
    }

    #[test]
    fn the_tap_shot_cracks_on_five_quick_taps() {
        let downs = (0..(6.5 * 600.0) as usize).filter(|&i| {
            let t = i as f64 / 600.0;
            cue(Shot::Tap, t).down && !cue(Shot::Tap, t - 1.0 / 600.0).down
        }).count();
        assert_eq!(downs, 7);
    }
}
