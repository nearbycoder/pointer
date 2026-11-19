//! What a frame draws, built on the CPU in painter order and handed to the GPU as two kinds
//! of geometry: instanced quads whose fragment shader evaluates a signed-distance shape
//! (`Prim`), and triangle ribbons along polylines (`RVert`) for the pole, trails and ink, so
//! a translucent stroke never blends twice where its pieces meet. All positions are in
//! points, display-local, y down.

use bytemuck::{Pod, Zeroable};

use crate::math::{V2, v2};

pub type Rgba = [f32; 4];

pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    [r, g, b, a]
}

pub fn with_alpha(c: Rgba, a: f32) -> Rgba {
    [c[0], c[1], c[2], c[3] * a]
}

/// Primitive kinds, as the shader switches on them (`b.w`).
pub mod kind {
    pub const DISC: f32 = 0.0;
    pub const RING: f32 = 1.0;
    pub const PILL: f32 = 2.0;
    pub const CRACK: f32 = 3.0;
    pub const OBJECT: f32 = 4.0;
    pub const OBJECT_SHADOW: f32 = 5.0;
    pub const DIM: f32 = 6.0;
    pub const CHROME_CAP: f32 = 7.0;
    pub const SHARD: f32 = 8.0;
    pub const BLADE: f32 = 9.0;
    pub const STAR: f32 = 10.0;
}

/// Ribbon styles (`style.x`).
pub mod ribbon {
    pub const SOLID: f32 = 0.0;
    pub const GLOW: f32 = 1.0;
    pub const SHADOW: f32 = 2.0;
    pub const CHROME: f32 = 3.0;
    pub const RUBBER: f32 = 4.0;
    pub const GLOSS: f32 = 5.0;
    pub const WOOD: f32 = 6.0;
}

/// One instanced quad. The quad covers the segment `a.xy`→`a.zw` grown by `b.z` points on
/// every side; `b.w` is the kind. The other words are per kind (see `shader.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Prim {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
    pub d: [f32; 4],
    pub e: [f32; 4],
}

/// One ribbon vertex. `ofs` is the offset from the centreline in units of the ribbon's
/// radius (±1 at its edge, past 1 in the antialiasing fringe; 2D in round caps); `aux` is
/// (distance along, radius in points, the side direction x, y); `style` picks the look.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct RVert {
    pub pos: [f32; 2],
    pub ofs: [f32; 2],
    pub aux: [f32; 4],
    pub color: [f32; 4],
    pub style: [f32; 4],
}

/// A ribbon's point: where, how wide (radius, points), what colour, and how far along.
#[derive(Clone, Copy, Debug)]
pub struct RPoint {
    pub p: V2,
    pub r: f32,
    pub color: Rgba,
    pub along: f32,
}

/// Dashes around a ring: `count` of them, each `duty` of its period long, turned by `phase`
/// radians. No dashes (a solid ring) by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dash {
    pub count: f32,
    pub duty: f32,
    pub phase: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Prims(u32, u32),
    Ribbon(u32, u32),
}

/// One effect slot's geometry, in painter order. Indices are into the frame's shared
/// buffers, so both slots upload in one go.
#[derive(Default)]
pub struct Layer {
    pub ops: Vec<Op>,
}

#[derive(Default)]
pub struct Scene {
    pub prims: Vec<Prim>,
    pub verts: Vec<RVert>,
    pub under: Layer,
    pub over: Layer,
    /// Device pixels per point, for antialiasing widths chosen on the CPU.
    pub scale: f32,
}

/// Builds into one layer of a scene.
pub struct Painter<'a> {
    prims: &'a mut Vec<Prim>,
    verts: &'a mut Vec<RVert>,
    layer: &'a mut Layer,
    /// One device pixel, in points.
    pub px: f32,
}

impl Scene {
    pub fn clear(&mut self, scale: f32) {
        self.prims.clear();
        self.verts.clear();
        self.under.ops.clear();
        self.over.ops.clear();
        self.scale = scale.max(0.25);
    }
    pub fn under(&mut self) -> Painter<'_> {
        Painter { prims: &mut self.prims, verts: &mut self.verts, layer: &mut self.under, px: 1.0 / self.scale.max(0.25) }
    }
    pub fn over(&mut self) -> Painter<'_> {
        Painter { prims: &mut self.prims, verts: &mut self.verts, layer: &mut self.over, px: 1.0 / self.scale.max(0.25) }
    }
}

impl Painter<'_> {
    pub fn prim(&mut self, p: Prim) {
        let i = self.prims.len() as u32;
        self.prims.push(p);
        match self.layer.ops.last_mut() {
            Some(Op::Prims(_, end)) if *end == i => *end = i + 1,
            _ => self.layer.ops.push(Op::Prims(i, i + 1)),
        }
    }

    fn verts_pushed(&mut self, start: u32) {
        let end = self.verts.len() as u32;
        if end == start { return; }
        match self.layer.ops.last_mut() {
            Some(Op::Ribbon(_, e)) if *e == start => *e = end,
            _ => self.layer.ops.push(Op::Ribbon(start, end)),
        }
    }

    /// A filled circle with a `feather`-point soft edge (0: a crisp, antialiased one).
    pub fn disc(&mut self, c: V2, r: f32, color: Rgba, feather: f32) {
        if color[3] <= 0.002 || r <= 0.0 { return; }
        self.prim(Prim {
            a: [c.x, c.y, c.x, c.y],
            b: [r, feather, r + feather + 2.0 * self.px, kind::DISC],
            c: color,
            ..Default::default()
        });
    }

    /// A circle outline `width` points wide.
    pub fn ring(&mut self, c: V2, r: f32, width: f32, color: Rgba) {
        self.ring_dashed(c, r, width, color, Dash::default());
    }

    /// A circle outline cut into dashes.
    pub fn ring_dashed(&mut self, c: V2, r: f32, width: f32, color: Rgba, dash: Dash) {
        if color[3] <= 0.002 || r <= 0.0 { return; }
        self.prim(Prim {
            a: [c.x, c.y, c.x, c.y],
            b: [r, width, r + width + 2.0 * self.px, kind::RING],
            c: color,
            d: [dash.count, dash.duty, dash.phase, 0.0],
            ..Default::default()
        });
    }

    /// A capsule from `a` to `b` with radius `r`, filled and outlined.
    pub fn pill(&mut self, a: V2, b: V2, r: f32, fill: Rgba, outline: Rgba, outline_w: f32) {
        if fill[3].max(outline[3]) <= 0.002 { return; }
        self.prim(Prim {
            a: [a.x, a.y, b.x, b.y],
            b: [r, outline_w, r + outline_w + 2.0 * self.px, kind::PILL],
            c: fill,
            d: outline,
            ..Default::default()
        });
    }

    /// A crack in the glass: a dark line `w` points wide with a light glint beside it.
    pub fn crack(&mut self, a: V2, b: V2, w: f32, alpha: f32, dark: Rgba, glint: Rgba) {
        if alpha <= 0.002 { return; }
        self.prim(Prim {
            a: [a.x, a.y, b.x, b.y],
            b: [w * 0.5, alpha, w + 3.0 * self.px, kind::CRACK],
            c: dark,
            d: glint,
            ..Default::default()
        });
    }

    /// A sliver of glass flying off an impact: a short bright line.
    pub fn shard(&mut self, a: V2, b: V2, w: f32, color: Rgba) {
        if color[3] <= 0.002 { return; }
        self.prim(Prim {
            a: [a.x, a.y, b.x, b.y],
            b: [w * 0.5, 0.0, w + 2.0 * self.px, kind::SHARD],
            c: color,
            ..Default::default()
        });
    }

    /// A short chrome cylinder (a collar or ferrule on the pole) from `a` to `b`.
    pub fn chrome_cap(&mut self, a: V2, b: V2, r: f32, tint: f32) {
        self.prim(Prim {
            a: [a.x, a.y, b.x, b.y],
            b: [r, tint, r + 2.0 * self.px, kind::CHROME_CAP],
            ..Default::default()
        });
    }

    /// A glowing energy blade from `a` to `b`: a white core of radius `core` in a halo of
    /// `glow` (the halo's opacity is `color[3]`). `flicker` 0..1 wavers its brightness.
    pub fn blade(&mut self, a: V2, b: V2, core: f32, glow: f32, color: Rgba, flicker: f32) {
        if color[3] <= 0.002 && core <= 0.0 { return; }
        self.prim(Prim {
            a: [a.x, a.y, b.x, b.y],
            b: [core, glow, glow * 2.2 + 2.0 * self.px, kind::BLADE],
            c: color,
            d: [flicker, 0.0, 0.0, 0.0],
            ..Default::default()
        });
    }

    /// A soft four-pointed twinkle, turned by `turn` radians.
    pub fn twinkle(&mut self, c: V2, r: f32, color: Rgba, turn: f32) {
        self.star(c, r, 4.0, color, [0.0; 4], turn);
    }

    /// A five-pointed cartoon star with a one-and-a-half-pixel `outline`.
    pub fn cartoon_star(&mut self, c: V2, r: f32, fill: Rgba, outline: Rgba, turn: f32) {
        self.star(c, r, 5.0, fill, [outline[0], outline[1], outline[2], 1.6], turn);
    }

    fn star(&mut self, c: V2, r: f32, points: f32, fill: Rgba, outline: [f32; 4], turn: f32) {
        if fill[3] <= 0.002 || r <= 0.0 { return; }
        self.prim(Prim {
            a: [c.x, c.y, c.x, c.y],
            b: [r, points, r * 1.7 + 2.0 * self.px, kind::STAR],
            c: fill,
            d: outline,
            e: [turn, 0.0, 0.0, 0.0],
        });
    }

    /// Plain triangles, coloured per corner (a swing trail's fan).
    pub fn triangles(&mut self, corners: &[(V2, Rgba)]) {
        let start = self.verts.len() as u32;
        for (p, color) in corners {
            self.verts.push(RVert {
                pos: [p.x, p.y],
                ofs: [0.0, 0.0],
                aux: [0.0, 1.0, 0.0, 1.0],
                color: *color,
                style: [ribbon::SOLID, 0.0, 0.0, 0.0],
            });
        }
        self.verts_pushed(start);
    }

    /// A ribbon through `pts`. Round caps on both ends when `caps`.
    pub fn ribbon(&mut self, pts: &[RPoint], style: [f32; 4], caps: bool) {
        if pts.len() < 2 { return; }
        let start = self.verts.len() as u32;
        let aa = 1.5 * self.px;
        let n = pts.len();
        let dir = |i: usize| (pts[(i + 1).min(n - 1)].p - pts[i.saturating_sub(1)].p).norm_or(v2(1.0, 0.0));
        // Side vectors (with a bounded miter) per point.
        let mut sides: Vec<(V2, f32)> = Vec::with_capacity(n);
        for i in 0..n {
            let t = dir(i);
            let side = t.cw();
            let miter = if i > 0 && i + 1 < n {
                let seg = (pts[i + 1].p - pts[i].p).norm_or(t).cw();
                (1.0 / side.dot(seg).max(0.4)).min(2.5)
            } else {
                1.0
            };
            sides.push((side, miter));
        }
        let vert = |p: &RPoint, side: V2, miter: f32, sign: f32, verts: &mut Vec<RVert>| {
            let half = p.r + aa;
            let pos = p.p + side * (half * miter * sign);
            verts.push(RVert {
                pos: [pos.x, pos.y],
                ofs: [sign * half / p.r.max(1e-3), 0.0],
                aux: [p.along, p.r, side.x, side.y],
                color: p.color,
                style,
            });
        };
        for i in 0..n - 1 {
            let (a, b) = (&pts[i], &pts[i + 1]);
            if (b.p - a.p).len() < 1e-4 { continue; }
            let (sa, ma) = sides[i];
            let (sb, mb) = sides[i + 1];
            vert(a, sa, ma, -1.0, self.verts);
            vert(a, sa, ma, 1.0, self.verts);
            vert(b, sb, mb, -1.0, self.verts);
            vert(a, sa, ma, 1.0, self.verts);
            vert(b, sb, mb, 1.0, self.verts);
            vert(b, sb, mb, -1.0, self.verts);
        }
        if caps {
            for (end, outward) in [(&pts[0], -dir(0)), (&pts[n - 1], dir(n - 1))] {
                let half = end.r + aa;
                let k = half / end.r.max(1e-3);
                let side = outward.cw();
                let segs = 10;
                for j in 0..segs {
                    let a0 = std::f32::consts::PI * j as f32 / segs as f32;
                    let a1 = std::f32::consts::PI * (j + 1) as f32 / segs as f32;
                    let rim = |a: f32| {
                        // From one side, around the end, to the other.
                        let (s, c) = a.sin_cos();
                        (side * c + outward * s, v2(c, s))
                    };
                    let (d0, o0) = rim(a0);
                    let (d1, o1) = rim(a1);
                    let mk = |pos: V2, ofs: V2| RVert {
                        pos: [pos.x, pos.y],
                        ofs: [ofs.x * k, ofs.y * k],
                        aux: [end.along, end.r, side.x, side.y],
                        color: end.color,
                        style,
                    };
                    self.verts.push(mk(end.p, V2::ZERO));
                    self.verts.push(mk(end.p + d0 * half, o0));
                    self.verts.push(mk(end.p + d1 * half, o1));
                }
            }
        }
        self.verts_pushed(start);
    }
}
