//! The pointer rig: a glove on a telescoping pole held from below the screen.
//!
//! The fingertip sits exactly on the cursor. Everything else follows it physically: the
//! pole's base (the presenter's hand, below the screen) trails behind on a lazy spring, the
//! pole's middle is a damped mass, so the pole bends and wobbles when the tip moves, and the
//! glove turns with the pole's end tangent. Springs are sub-stepped so a stiff one stays
//! stable at any frame rate.

use crate::math::{Spring, Spring2, V2, v2};

/// The glove's model is in units of its size: the fingertip is at the origin, the glove's
/// axis runs down +y, and the bottom of the cuff (where the pole goes in) is here.
pub const ATTACH: V2 = v2(0.08, 1.0);
const STEP: f32 = 1.0 / 600.0;

#[derive(Clone, Copy, Debug)]
pub struct RigInput {
    /// Display size in points.
    pub w: f32,
    pub h: f32,
    /// The cursor, display-local, when it's on this display.
    pub cursor: Option<V2>,
    pub pressed: bool,
    /// Whether the pointer should be out at all.
    pub shown: bool,
    /// The glove's size in points (fingertip to the bottom of the cuff).
    pub size: f32,
    /// Holds the presenter's hand here instead of following the tip (for framed artwork).
    pub base: Option<V2>,
}

#[derive(Clone, Debug)]
pub struct Rig {
    pub ext: Spring,
    pub size: Spring,
    pub aim: V2,
    pub tip: V2,
    pub base: Spring2,
    pub mid: Spring2,
    pub press: Spring,
    pub roll: Spring,
    pub kick: Spring2,
    pub spin: Spring,
    pub vel: V2,
    prev_tip: V2,
    fresh: bool,
    ext_target: f32,
}

/// Where everything is this frame, for drawing.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    pub visible: bool,
    /// The fingertip, display-local points.
    pub tip: V2,
    /// Unit vector from the fingertip down the glove's axis (toward the cuff).
    pub down: V2,
    /// The glove's size in points, after the press squash.
    pub size: f32,
    pub roll: f32,
    pub press: f32,
    /// The pole: a cubic Bézier from `base` (below the screen) to `attach` (the cuff).
    pub base: V2,
    pub c1: V2,
    pub c2: V2,
    pub attach: V2,
    /// How far the glove floats above the screen, in points (sets the shadow's offset).
    pub lift: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Rig {
            ext: Spring::at(0.0),
            size: Spring::at(0.0),
            aim: V2::ZERO,
            tip: V2::ZERO,
            base: Spring2::default(),
            mid: Spring2::default(),
            press: Spring::default(),
            roll: Spring::default(),
            kick: Spring2::default(),
            spin: Spring::default(),
            vel: V2::ZERO,
            prev_tip: V2::ZERO,
            fresh: true,
            ext_target: 0.0,
        }
    }
}

impl Rig {
    /// Where the presenter's hand wants to be for a tip at `tip`: below the screen and to the
    /// right of the tip, following it about two thirds as far.
    fn base_target(&self, tip: V2, w: f32, h: f32, s: f32) -> V2 {
        let u = (tip.x / w.max(1.0)).clamp(-0.15, 1.15);
        v2(w * (0.40 + 0.63 * u), (h + 0.75 * s).max(tip.y + 2.8 * s))
    }

    fn hidden_tip(&self, aim: V2, w: f32, h: f32, s: f32) -> V2 {
        let base = self.base_target(aim, w, h, s);
        v2(aim.x + (base.x - aim.x) * 0.35, h + 0.45 * s)
    }

    /// Snaps every spring to where it would rest for `aim`.
    fn snap(&mut self, input: &RigInput, aim: V2) {
        let s = input.size;
        self.size = Spring::at(s);
        self.aim = aim;
        self.tip = self.hidden_tip(aim, input.w, input.h, s);
        self.prev_tip = self.tip;
        self.base = Spring2::at(self.base_target(self.tip, input.w, input.h, s));
        self.mid = Spring2::at((self.base.x + self.tip) * 0.5);
        self.vel = V2::ZERO;
        self.fresh = false;
    }

    pub fn step(&mut self, dt: f32, input: &RigInput) {
        if self.fresh {
            let aim = input.cursor.unwrap_or(v2(input.w * 0.5, input.h * 0.6));
            self.snap(input, aim);
        }
        if let Some(c) = input.cursor { self.aim = c; }
        self.ext_target = if input.shown && input.cursor.is_some() { 1.0 } else { 0.0 };
        // A pointer that's fully in jumps straight to the cursor's display position, so its
        // next extension starts from below where the cursor is now.
        if self.ext.x < 0.002 && self.ext_target == 0.0 {
            self.ext = Spring::at(0.0);
            self.tip = self.hidden_tip(self.aim, input.w, input.h, input.size);
            self.prev_tip = self.tip;
            self.base = Spring2::at(self.base_target(self.tip, input.w, input.h, input.size));
            self.mid = Spring2::at((self.base.x + self.tip) * 0.5);
            self.vel = V2::ZERO;
            self.size = Spring::at(input.size);
            return;
        }
        let dt = dt.clamp(0.0, 0.1);
        let steps = ((dt / STEP).ceil() as usize).clamp(1, 120);
        let h = dt / steps as f32;
        let (ext_w, ext_z) = if self.ext_target > self.ext.x { (11.0, 0.78) } else { (14.0, 1.0) };
        for _ in 0..steps {
            self.size.step(input.size, 18.0, 0.9, h);
            let s = self.size.x;
            self.ext.step(self.ext_target, ext_w, ext_z, h);
            self.ext.x = self.ext.x.clamp(0.0, 1.03);
            let hidden = self.hidden_tip(self.aim, input.w, input.h, s);
            self.tip = hidden.lerp(self.aim, self.ext.x);
            let target = input.base.unwrap_or_else(|| self.base_target(self.tip, input.w, input.h, s));
            self.base.step(target, 4.6, 0.95, h);
            let straight = (self.base.x + self.tip) * 0.5;
            self.mid.step(straight, 7.5, 0.24, h);
            let (pw, pz) = if input.pressed { (40.0, 0.55) } else { (21.0, 0.36) };
            self.press.step(if input.pressed { 1.0 } else { 0.0 }, pw, pz, h);
            let roll = (-self.vel.x * 0.00042).clamp(-0.5, 0.5);
            self.roll.step(roll, 8.5, 0.55, h);
            self.kick.step(V2::ZERO, 26.0, 0.28, h);
            self.spin.step(0.0, 22.0, 0.26, h);
        }
        if dt > 0.0 {
            let v = (self.tip - self.prev_tip) / dt;
            let k = 1.0 - (-dt * 18.0).exp();
            self.vel = self.vel.lerp(v, k);
        }
        self.prev_tip = self.tip;
    }

    /// A shove from an impact: the glove jolts back along the pole and twists a little.
    pub fn recoil(&mut self, strength: f32) {
        let pose = self.pose_with(ATTACH);
        self.kick.v += pose.down * (900.0 * strength);
        self.spin.v += 9.0 * strength;
        self.press.v -= 6.0 * strength;
    }

    pub fn settled(&self) -> bool {
        let still = self.vel.len() < 2.0 && self.mid.v.len() < 2.0 && self.base.v.len() < 2.0;
        still && self.ext.settled(self.ext_target, 0.002) && self.press.settled(self.press.x.round(), 0.002)
            && self.roll.settled(self.roll.x, 0.002) && self.kick.x.len() < 0.05 && self.kick.v.len() < 0.5
            && self.spin.x.abs() < 0.001 && self.spin.v.abs() < 0.01
    }

    /// Whether the pole is on its way out (or out), rather than in.
    pub fn extending(&self) -> bool {
        self.ext_target > 0.5
    }

    pub fn visible(&self) -> bool {
        self.ext.x > 0.002 || self.ext_target > 0.0
    }

    /// The glove and pole for this frame. `h` is the display height (for the lift).
    #[cfg(test)]
    pub fn pose(&self, _h: f32) -> Pose {
        self.pose_with(ATTACH)
    }

    /// The pose for a pointer whose model meets the pole at `attach` (model units).
    pub fn pose_with(&self, attach_at: V2) -> Pose {
        let press = self.press.x.clamp(-0.4, 1.2);
        let s = self.size.x * (1.0 - 0.07 * press.max(0.0));
        let tip = self.tip + self.kick.x;
        let base = self.base.x;
        let chord = tip - base;
        let length = chord.len();
        let axis = chord.norm_or(v2(0.0, -1.0));
        let straight = (base + tip) * 0.5;
        let mut off = self.mid.x - straight;
        off -= axis * off.dot(axis);
        let mid = straight + off.clamp_len(0.12 * length);
        // The glove points along the pole's last stretch, from its bend to the tip, never
        // far off the straight line from the base.
        let lean = (tip - mid).norm_or(axis);
        let angle = (axis.x * lean.y - axis.y * lean.x).atan2(axis.dot(lean)).clamp(-0.6, 0.6);
        let down = (-axis.rotate(angle)).rotate(self.spin.x);
        let right = v2(down.y, -down.x);
        let attach = tip + right * (attach_at.x * s) + down * (attach_at.y * s);
        // The pole leaves the base toward the bend and runs into the cuff along the glove.
        let reach = (attach - base).len() * 0.38;
        let c1 = base + (mid - base).norm_or(axis) * reach;
        let c2 = attach + down * reach;
        Pose {
            visible: self.ext.x > 0.002,
            tip,
            down,
            size: s,
            roll: self.roll.x,
            press,
            base,
            c1,
            c2,
            attach,
            lift: self.size.x * (0.16 - 0.12 * press.clamp(0.0, 1.0)),
        }
    }
}

impl Pose {
    /// The pole's centreline at `t` (0 at the base, 1 at the cuff).
    pub fn pole_at(&self, t: f32) -> V2 {
        let u = 1.0 - t;
        self.base * (u * u * u) + self.c1 * (3.0 * u * u * t) + self.c2 * (3.0 * u * t * t) + self.attach * (t * t * t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(cursor: V2) -> RigInput {
        RigInput { w: 1920.0, h: 1080.0, cursor: Some(cursor), pressed: false, shown: true, size: 120.0, base: None }
    }

    #[test]
    fn the_tip_lands_on_the_cursor_and_the_glove_points_along_the_pole() {
        let mut rig = Rig::default();
        let at = v2(700.0, 400.0);
        let mut rest = None;
        for i in 0..600 {
            rig.step(1.0 / 120.0, &input(at));
            if rest.is_none() && rig.settled() { rest = Some(i as f32 / 120.0); }
        }
        // It comes to rest (and the overlay stops drawing) a few seconds after it stops.
        assert!(rest.is_some_and(|t| t < 4.0), "rests at {rest:?}");
        let pose = rig.pose(1080.0);
        assert!((pose.tip - at).len() < 0.5, "tip {:?}", pose.tip);
        assert!(pose.base.y > 1080.0, "the base stays below the screen: {:?}", pose.base);
        assert!(pose.base.x > at.x, "held from the right: {:?}", pose.base);
        // The glove's axis points from the tip back toward the base.
        let to_base = (pose.base - pose.tip).norm_or(V2::ZERO);
        assert!(pose.down.dot(to_base) > 0.9, "down {:?} vs {:?}", pose.down, to_base);
        assert!(rig.settled());
    }

    #[test]
    fn a_fast_sweep_bends_the_pole_and_it_springs_back() {
        let mut rig = Rig::default();
        for _ in 0..240 { rig.step(1.0 / 120.0, &input(v2(400.0, 300.0))); }
        let mut max_bend: f32 = 0.0;
        for i in 0..30 {
            let x = 400.0 + i as f32 * 40.0;
            rig.step(1.0 / 120.0, &input(v2(x, 300.0)));
            let p = rig.pose(1080.0);
            let chord_mid = (p.base + p.attach) * 0.5;
            max_bend = max_bend.max((p.pole_at(0.5) - chord_mid).len());
        }
        assert!(max_bend > 8.0, "a sweep bends the pole: {max_bend}");
        for _ in 0..600 { rig.step(1.0 / 120.0, &input(v2(1560.0, 300.0))); }
        let p = rig.pose(1080.0);
        let chord_mid = (p.base + p.attach) * 0.5;
        assert!((p.pole_at(0.5) - chord_mid).len() < 1.0, "and straightens at rest");
    }

    #[test]
    fn the_pole_always_ends_in_the_cuff() {
        // Fast moves near the bottom corner used to tear the glove off its pole.
        let mut rig = Rig::default();
        let mut worst: f32 = 0.0;
        for i in 0..600 {
            let t = i as f32 / 120.0;
            let at = v2(1170.0 + 330.0 * (t * 3.1).sin().abs(), 600.0 + 300.0 * (t * 2.3).sin().abs());
            rig.step(1.0 / 120.0, &input(at));
            let p = rig.pose(1080.0);
            let cuff = p.tip + v2(p.down.y, -p.down.x) * (ATTACH.x * p.size) + p.down * (ATTACH.y * p.size);
            worst = worst.max((p.pole_at(1.0) - cuff).len());
            // And the pole arrives along the glove's axis.
            let end = (p.pole_at(1.0) - p.pole_at(0.98)).norm_or(V2::ZERO);
            assert!(end.dot(-p.down) > 0.95, "pole end {end:?} vs glove {:?}", p.down);
        }
        assert!(worst < 0.01, "{worst}");
    }

    #[test]
    fn hiding_retracts_below_the_screen() {
        let mut rig = Rig::default();
        for _ in 0..240 { rig.step(1.0 / 120.0, &input(v2(900.0, 500.0))); }
        let mut i = input(v2(900.0, 500.0));
        i.shown = false;
        for _ in 0..240 { rig.step(1.0 / 120.0, &i); }
        assert!(!rig.visible());
    }
}
