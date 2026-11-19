//! A scripted tour for headless videos and stills: where the mouse is, whether it's pressed,
//! and which tool is out, at any time. Laid out on the 1920×1080 backdrop page.

use std::f32::consts::TAU;

use crate::math::{V2, smoothstep, v2};
use crate::settings::Tool;
use crate::style::Style;

pub const LENGTH: f64 = 17.0;

const KEYS: &[(f32, f32, f32)] = &[
    (0.0, 760.0, 330.0),
    (1.2, 760.0, 330.0),
    (2.0, 900.0, 470.0),
    (2.6, 900.0, 470.0),
    (3.2, 862.0, 572.0),
    (3.7, 862.0, 572.0),
    (4.3, 690.0, 760.0),
    (6.2, 690.0, 760.0),
    (6.9, 1160.0, 420.0),
    // Spotlight: drift across the tiles.
    (7.6, 1240.0, 760.0),
    (8.6, 960.0, 780.0),
    (9.2, 820.0, 300.0),
    // Laser: a swoosh under the title is scripted below (9.6 to 10.8).
    (9.6, 520.0, 360.0),
    (10.8, 1400.0, 360.0),
    (11.3, 1230.0, 690.0),
    // Pen: a circle round the orange tile is scripted below (11.6 to 12.7).
    (11.6, 1230.0, 690.0),
    (12.7, 1230.0, 690.0),
    (13.2, 760.0, 515.0),
    // Focus: drag a box round the chips.
    (13.6, 760.0, 515.0),
    (14.4, 1170.0, 600.0),
    (15.6, 1170.0, 600.0),
    (16.4, 1500.0, 900.0),
    (17.0, 1500.0, 900.0),
];

const PRESSES: &[(f32, f32)] = &[
    (2.3, 2.42),
    (3.42, 3.52),
    // Five quick taps crack the glass.
    (4.55, 4.62),
    (4.75, 4.82),
    (4.95, 5.02),
    (5.15, 5.22),
    (5.35, 5.42),
    (8.2, 8.3),
    (11.6, 12.7),
    (13.6, 14.4),
];

fn tool_at(t: f32) -> Tool {
    match t {
        t if t < 6.6 => Tool::Tap,
        t if t < 9.4 => Tool::Spotlight,
        t if t < 11.1 => Tool::Laser,
        t if t < 13.0 => Tool::Pen,
        t if t < 16.0 => Tool::Focus,
        _ => Tool::Tap,
    }
}

fn keyed(t: f32) -> V2 {
    let i = KEYS.partition_point(|k| k.0 <= t).clamp(1, KEYS.len() - 1);
    let (t0, x0, y0) = KEYS[i - 1];
    let (t1, x1, y1) = KEYS[i];
    let e = smoothstep(t0, t1, t);
    v2(x0 + (x1 - x0) * e, y0 + (y1 - y0) * e)
}

/// Where the mouse is at `t`, whether it's down, and the tool that's out.
pub fn script(t: f64) -> (V2, bool, Tool) {
    let t = (t as f32).min(LENGTH as f32);
    let mut p = keyed(t);
    if (9.6..10.8).contains(&t) {
        let u = (t - 9.6) / 1.2;
        p = v2(520.0 + 880.0 * u, 362.0 + 26.0 * (u * TAU * 1.5).sin() * (1.0 - u * 0.4));
    }
    if (11.6..12.7).contains(&t) {
        let u = (t - 11.6) / 1.1;
        let a = -1.9 + u * TAU * 1.08;
        p = v2(1180.0 + 160.0 * a.cos(), 690.0 + 105.0 * a.sin());
    }
    let pressed = PRESSES.iter().any(|(a, b)| t >= *a && t < *b);
    (p, pressed, tool_at(t))
}

/// How long each pointer gets in the showcase.
const TURN: f32 = 3.4;
pub const SHOWCASE: f64 = TURN as f64 * 5.0;

/// The showcase: each pointer in turn comes out, swoops across the page, clicks twice and
/// wiggles. Where the mouse is, whether it's down, and which pointer is out.
pub fn styles(t: f64) -> (V2, bool, Style) {
    let t = (t as f32).min(SHOWCASE as f32 - 0.001);
    let turn = (t / TURN) as usize;
    let u = t - turn as f32 * TURN;
    let style = Style::ALL[turn.min(4)];
    let start = v2(700.0, 470.0);
    let p = if u < 0.9 {
        start
    } else if u < 2.1 {
        // A swoop: out to the right and back up, fast enough to bend and trail.
        let a = smoothstep(0.9, 2.1, u) * std::f32::consts::PI * 1.5;
        v2(1000.0 + 300.0 * (a - std::f32::consts::FRAC_PI_2).sin(), 560.0 - 170.0 * (a * 0.9).sin())
    } else {
        v2(1060.0 + 8.0 * ((u - 2.1) * 20.0).sin(), 560.0)
    };
    let p = if u > 0.9 && u < 1.05 { start.lerp(p, smoothstep(0.9, 1.05, u)) } else { p };
    let pressed = (2.25..2.35).contains(&u) || (2.7..2.8).contains(&u);
    (p, pressed, style)
}
