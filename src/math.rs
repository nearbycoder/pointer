//! Small 2D vector and spring helpers for the rig and the effects.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}

pub const fn v2(x: f32, y: f32) -> V2 {
    V2 { x, y }
}

impl V2 {
    pub const ZERO: V2 = v2(0.0, 0.0);

    pub fn len(self) -> f32 {
        self.x.hypot(self.y)
    }
    pub fn dot(self, o: V2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    /// Unit vector, or `fallback` when this one is (nearly) zero.
    pub fn norm_or(self, fallback: V2) -> V2 {
        let l = self.len();
        if l > 1e-6 { self / l } else { fallback }
    }
    /// Rotated 90° clockwise on screen (y points down): up (0,-1) becomes right (1,0).
    pub fn cw(self) -> V2 {
        v2(-self.y, self.x)
    }
    pub fn rotate(self, angle: f32) -> V2 {
        let (s, c) = angle.sin_cos();
        v2(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    pub fn lerp(self, o: V2, t: f32) -> V2 {
        self + (o - self) * t
    }
    pub fn from_angle(angle: f32) -> V2 {
        let (s, c) = angle.sin_cos();
        v2(c, s)
    }
    pub fn clamp_len(self, max: f32) -> V2 {
        let l = self.len();
        if l > max && l > 0.0 { self * (max / l) } else { self }
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for V2 {
    type Output = V2;
    fn mul(self, k: f32) -> V2 {
        v2(self.x * k, self.y * k)
    }
}
impl Div<f32> for V2 {
    type Output = V2;
    fn div(self, k: f32) -> V2 {
        v2(self.x / k, self.y / k)
    }
}
impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        v2(-self.x, -self.y)
    }
}
impl AddAssign for V2 {
    fn add_assign(&mut self, o: V2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl SubAssign for V2 {
    fn sub_assign(&mut self, o: V2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}

/// A damped spring on a scalar, stepped with semi-implicit Euler. Callers sub-step, so even a
/// stiff spring stays stable.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
}

impl Spring {
    pub fn at(x: f32) -> Spring {
        Spring { x, v: 0.0 }
    }
    /// `omega` is the natural frequency in rad/s, `zeta` the damping ratio (1 = critical).
    pub fn step(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) {
        let a = omega * omega * (target - self.x) - 2.0 * zeta * omega * self.v;
        self.v += a * dt;
        self.x += self.v * dt;
    }
    pub fn settled(&self, target: f32, eps: f32) -> bool {
        (self.x - target).abs() < eps && self.v.abs() < eps * 10.0
    }
}

/// The same spring on a 2D point.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring2 {
    pub x: V2,
    pub v: V2,
}

impl Spring2 {
    pub fn at(x: V2) -> Spring2 {
        Spring2 { x, v: V2::ZERO }
    }
    pub fn step(&mut self, target: V2, omega: f32, zeta: f32, dt: f32) {
        let a = (target - self.x) * (omega * omega) - self.v * (2.0 * zeta * omega);
        self.v += a * dt;
        self.x += self.v * dt;
    }
}

pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = clamp01((x - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = clamp01(t);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_out_expo(t: f32) -> f32 {
    let t = clamp01(t);
    if t >= 1.0 { 1.0 } else { 1.0 - 2f32.powf(-10.0 * t) }
}

/// A tiny deterministic PRNG (PCG-ish) so effects replay identically in headless runs.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x2545_F491_4F6C_DD1D));
        r.next_u32();
        r
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let x = ((self.0 >> 18) ^ self.0) >> 27;
        let rot = (self.0 >> 59) as u32;
        (x as u32).rotate_right(rot)
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
    /// Uniform in [a, b).
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
}
