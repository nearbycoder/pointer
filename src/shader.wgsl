// Pointer's shaders. Positions are in points, display-local, y down. Colours come in as
// sRGB with straight alpha and leave premultiplied, in the encoding the frame blends in.

struct U {
    vsize: vec2f,   // viewport, points
    tsize: vec2f,   // target, device pixels
    origin: vec2f,  // device-pixel origin of the pass
    scale: f32,     // device pixels per point
    srgb: f32,      // 1: the frame blends sRGB-encoded values
    time: f32,
    p0: f32,
    p1: f32,
    p2: f32,
}
@group(0) @binding(0) var<uniform> u: U;

const PI: f32 = 3.14159265;
const TAU: f32 = 6.2831853;

fn to_clip(p: vec2f) -> vec4f {
    let d = p * u.scale - u.origin;
    return vec4f(d.x / u.tsize.x * 2.0 - 1.0, 1.0 - d.y / u.tsize.y * 2.0, 0.0, 1.0);
}

fn lin_to_srgb(c: vec3f) -> vec3f {
    let c0 = max(c, vec3f(0.0));
    let lo = c0 * 12.92;
    let hi = 1.055 * pow(c0, vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c0 <= vec3f(0.0031308));
}
fn srgb_to_lin(c: vec3f) -> vec3f {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3f(0.0)) + 0.055) / 1.055, vec3f(2.4));
    return select(hi, lo, c <= vec3f(0.04045));
}
// An sRGB colour at coverage `a`, premultiplied, in the frame's encoding.
fn out_srgb(c: vec3f, a: f32) -> vec4f {
    let rgb = select(srgb_to_lin(c), c, u.srgb > 0.5);
    return vec4f(rgb * a, a);
}
// A linear-light colour at coverage `a`.
fn out_lin(c: vec3f, a: f32) -> vec4f {
    let rgb = select(c, lin_to_srgb(c), u.srgb > 0.5);
    return vec4f(rgb * a, a);
}
// Premultiplied sRGB in, frame encoding out (for sums of premultiplied layers).
fn out_pm(c: vec3f, a: f32) -> vec4f {
    if (u.srgb > 0.5 || a <= 0.0) { return vec4f(c, a); }
    return vec4f(srgb_to_lin(c / a) * a, a);
}

// Coverage of the inside of a shape at signed distance `d` points.
fn aa(d: f32) -> f32 {
    return clamp(0.5 - d * u.scale, 0.0, 1.0);
}

fn sd_seg(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return length(pa - ba * h);
}
fn seg_closest(p: vec2f, a: vec2f, b: vec2f) -> vec2f {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    return a + ba * h;
}

// ---------------------------------------------------------------- the raymarched models
// Units of the pointer's size: the tip at the origin, +y down the pointer, +z toward you.
// Model 0 is the glove, 1 the rubber chicken, 2 the mallet's head.

fn sd_cap3(p: vec3f, a: vec3f, b: vec3f, r: f32) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h) - r;
}
fn sd_rbox3(p: vec3f, c: vec3f, half: vec3f, r: f32) -> f32 {
    let q = abs(p - c) - (half - vec3f(r));
    return length(max(q, vec3f(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}
// A rounded cylinder around the y axis: outer radius `big`, half-height `hh`, rounding `rb`.
fn sd_rcyl(p: vec3f, c: vec3f, big: f32, hh: f32, rb: f32) -> f32 {
    let q = p - c;
    let d = vec2f(length(q.xz) - (big - rb), abs(q.y) - (hh - rb));
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2f(0.0))) - rb;
}
// The same around the x axis.
fn sd_rcyl_x(p: vec3f, c: vec3f, big: f32, hh: f32, rb: f32) -> f32 {
    let q = p - c;
    let d = vec2f(length(q.yz) - (big - rb), abs(q.x) - (hh - rb));
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2f(0.0))) - rb;
}
fn sd_ellipsoid(p: vec3f, r: vec3f) -> f32 {
    let k0 = length(p / r);
    let k1 = length(p / (r * r));
    return k0 * (k0 - 1.0) / max(k1, 1e-6);
}
// A cone with rounded ends: radius r1 at a, r2 at b.
fn sd_round_cone(p: vec3f, a: vec3f, b: vec3f, r1: f32, r2: f32) -> f32 {
    let ba = b - a;
    let l2 = dot(ba, ba);
    let rr = r1 - r2;
    let a2 = l2 - rr * rr;
    let il2 = 1.0 / l2;
    let pa = p - a;
    let y = dot(pa, ba);
    let z = y - l2;
    let xv = pa * l2 - ba * y;
    let x2 = dot(xv, xv);
    let y2 = y * y * l2;
    let z2 = z * z * l2;
    let k = sign(rr) * rr * rr * x2;
    if (sign(z) * a2 * z2 > k) { return sqrt(x2 + z2) * il2 - r2; }
    if (sign(y) * a2 * y2 < k) { return sqrt(x2 + y2) * il2 - r1; }
    return (sqrt(x2 * a2 * il2) + y * rr) * il2 - r1;
}
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k * 0.25;
}

fn glove(p: vec3f) -> f32 {
    let index = sd_cap3(p, vec3f(0.0, 0.094, 0.0), vec3f(0.012, 0.46, -0.01), 0.094);
    let palm = sd_rbox3(p, vec3f(0.10, 0.62, -0.035), vec3f(0.178, 0.175, 0.09), 0.085);
    let f1 = sd_cap3(p, vec3f(0.075, 0.49, 0.06), vec3f(0.24, 0.50, 0.035), 0.08);
    let f2 = sd_cap3(p, vec3f(0.075, 0.607, 0.065), vec3f(0.255, 0.617, 0.035), 0.079);
    let f3 = sd_cap3(p, vec3f(0.075, 0.72, 0.055), vec3f(0.225, 0.725, 0.03), 0.069);
    let thumb = sd_cap3(p, vec3f(-0.105, 0.70, 0.06), vec3f(0.065, 0.555, 0.11), 0.075);
    let wrist = sd_cap3(p, vec3f(0.09, 0.74, -0.02), vec3f(0.08, 0.84, -0.01), 0.13);
    let cuff = sd_rcyl(p, vec3f(0.08, 0.905, -0.01), 0.228, 0.095, 0.062);
    var d = smin(index, palm, 0.06);
    d = smin(d, wrist, 0.05);
    let fingers = smin(smin(f1, f2, 0.008), f3, 0.008);
    d = smin(d, fingers, 0.028);
    d = smin(d, thumb, 0.022);
    d = smin(d, cuff, 0.01);
    return d;
}

// The chicken, beak first, stretched out along the pointer with its feet on the pole.
fn ck_beak(p: vec3f) -> f32 { return sd_round_cone(p, vec3f(0.0, 0.016, 0.0), vec3f(0.0, 0.115, 0.0), 0.013, 0.05); }
fn ck_red(p: vec3f) -> f32 {
    let comb = min(min(length(p - vec3f(-0.078, 0.13, 0.0)) - 0.036, length(p - vec3f(-0.097, 0.19, 0.0)) - 0.043), length(p - vec3f(-0.085, 0.25, 0.0)) - 0.034);
    let wattle = sd_ellipsoid(p - vec3f(0.056, 0.15, 0.0), vec3f(0.03, 0.047, 0.026));
    return min(comb, wattle);
}
fn ck_eye(p: vec3f) -> f32 { return length(p - vec3f(0.026, 0.165, 0.074)) - 0.017; }
fn ck_feet(p: vec3f) -> f32 {
    let leg1 = sd_cap3(p, vec3f(-0.03, 0.93, 0.0), vec3f(-0.045, 1.05, 0.0), 0.019);
    let leg2 = sd_cap3(p, vec3f(0.07, 0.93, 0.0), vec3f(0.085, 1.05, 0.0), 0.019);
    let foot1 = sd_ellipsoid(p - vec3f(-0.055, 1.066, 0.0), vec3f(0.058, 0.02, 0.036));
    let foot2 = sd_ellipsoid(p - vec3f(0.095, 1.066, 0.0), vec3f(0.058, 0.02, 0.036));
    return min(min(leg1, leg2), min(foot1, foot2));
}
fn ck_yellow(p: vec3f) -> f32 {
    let head = length(p - vec3f(0.0, 0.18, 0.0)) - 0.088;
    let neck = sd_cap3(p, vec3f(0.0, 0.22, 0.0), vec3f(0.015, 0.52, 0.0), 0.048);
    let body = sd_ellipsoid(p - vec3f(0.02, 0.75, 0.0), vec3f(0.165, 0.24, 0.13));
    let wing = sd_ellipsoid(p - vec3f(0.07, 0.72, 0.1), vec3f(0.09, 0.15, 0.042));
    let tail = sd_cap3(p, vec3f(-0.11, 0.86, 0.0), vec3f(-0.175, 0.945, 0.0), 0.046);
    var d = smin(head, neck, 0.04);
    d = smin(d, body, 0.07);
    d = smin(d, wing, 0.02);
    return smin(d, tail, 0.03);
}
fn chicken(p: vec3f) -> f32 {
    var d = smin(ck_yellow(p), ck_beak(p), 0.015);
    d = smin(d, ck_red(p), 0.012);
    return min(min(d, ck_eye(p)), ck_feet(p));
}

// The mallet's head: a wooden barrel across the pointer with iron bands, its face at the tip.
fn ml_bands(p: vec3f) -> f32 {
    return min(sd_rcyl_x(p, vec3f(0.17, 0.0, 0.0), 0.318, 0.035, 0.012), sd_rcyl_x(p, vec3f(0.83, 0.0, 0.0), 0.318, 0.035, 0.012));
}
fn mallet(p: vec3f) -> f32 {
    let head = sd_rcyl_x(p, vec3f(0.5, 0.0, 0.0), 0.3, 0.5, 0.07);
    let socket = sd_cap3(p, vec3f(0.5, 0.2, 0.0), vec3f(0.5, 0.36, 0.0), 0.075);
    return min(min(head, socket), ml_bands(p));
}

fn model_sdf(p: vec3f, m: i32) -> f32 {
    if (m == 1) { return chicken(p); }
    if (m == 2) { return mallet(p); }
    return glove(p);
}

fn sd_cap2(p: vec2f, a: vec2f, b: vec2f, r: f32) -> f32 {
    return sd_seg(p, a, b) - r;
}
fn sd_rbox2(p: vec2f, c: vec2f, half: vec2f, r: f32) -> f32 {
    let q = abs(p - c) - (half - vec2f(r));
    return length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
}
fn sd_ellipse2(p: vec2f, r: vec2f) -> f32 {
    let k0 = length(p / r);
    let k1 = length(p / (r * r));
    return k0 * (k0 - 1.0) / max(k1, 1e-6);
}
// Each model's silhouette, seen straight on: for skipping empty pixels and for shadows.
fn glove2(q: vec2f) -> f32 {
    let index = sd_cap2(q, vec2f(0.0, 0.094), vec2f(0.012, 0.46), 0.094);
    let palm = sd_rbox2(q, vec2f(0.10, 0.62), vec2f(0.178, 0.175), 0.085);
    let f1 = sd_cap2(q, vec2f(0.075, 0.49), vec2f(0.24, 0.50), 0.08);
    let f2 = sd_cap2(q, vec2f(0.075, 0.607), vec2f(0.255, 0.617), 0.079);
    let f3 = sd_cap2(q, vec2f(0.075, 0.72), vec2f(0.225, 0.725), 0.069);
    let thumb = sd_cap2(q, vec2f(-0.105, 0.70), vec2f(0.065, 0.555), 0.075);
    let wrist = sd_cap2(q, vec2f(0.09, 0.74), vec2f(0.08, 0.84), 0.13);
    let cuff = sd_rbox2(q, vec2f(0.08, 0.905), vec2f(0.228, 0.095), 0.062);
    var d = min(index, palm);
    d = min(d, min(wrist, min(f1, min(f2, f3))));
    d = min(d, min(thumb, cuff));
    return d;
}
fn chicken2(q: vec2f) -> f32 {
    let beak = sd_cap2(q, vec2f(0.0, 0.03), vec2f(0.0, 0.11), 0.045);
    let head = length(q - vec2f(0.0, 0.18)) - 0.088;
    let comb = length(q - vec2f(-0.092, 0.19)) - 0.075;
    let neck = sd_cap2(q, vec2f(0.0, 0.22), vec2f(0.015, 0.52), 0.048);
    let body = sd_ellipse2(q - vec2f(0.02, 0.75), vec2f(0.165, 0.24));
    let tail = sd_cap2(q, vec2f(-0.11, 0.86), vec2f(-0.175, 0.945), 0.046);
    let feet = sd_rbox2(q, vec2f(0.02, 1.0), vec2f(0.14, 0.08), 0.02);
    return min(min(min(beak, head), min(comb, neck)), min(min(body, tail), feet));
}
fn mallet2(q: vec2f) -> f32 {
    return min(sd_rbox2(q, vec2f(0.5, 0.0), vec2f(0.5, 0.318), 0.07), sd_cap2(q, vec2f(0.5, 0.2), vec2f(0.5, 0.36), 0.075));
}
fn model2(q: vec2f, m: i32) -> f32 {
    if (m == 1) { return chicken2(q); }
    if (m == 2) { return mallet2(q); }
    return glove2(q);
}

fn model_normal(p: vec3f, m: i32) -> vec3f {
    let e = vec2f(1.0, -1.0) * 0.0007;
    return normalize(e.xyy * model_sdf(p + e.xyy, m) + e.yyx * model_sdf(p + e.yyx, m) + e.yxy * model_sdf(p + e.yxy, m) + e.xxx * model_sdf(p + e.xxx, m));
}

fn model_ao(p: vec3f, n: vec3f, m: i32) -> f32 {
    var occ = 0.0;
    var w = 1.0;
    for (var i = 1; i <= 5; i++) {
        let h = 0.014 * f32(i);
        occ += (h - model_sdf(p + n * h, m)) * w;
        w *= 0.62;
    }
    return clamp(1.0 - 3.2 * occ, 0.0, 1.0);
}

// What the surface is made of where the ray landed: albedo (linear) and how glossy.
struct Mat { albedo: vec3f, spec: f32, power: f32 }

fn model_mat(p: vec3f, m: i32) -> Mat {
    if (m == 1) {
        let yellow = ck_yellow(p);
        let orange = min(ck_beak(p), ck_feet(p));
        let red = ck_red(p);
        let eye = ck_eye(p);
        let best = min(min(yellow, orange), min(red, eye));
        if (eye <= best + 0.001) { return Mat(vec3f(0.01, 0.01, 0.012), 1.2, 90.0); }
        if (red <= best + 0.001) { return Mat(srgb_to_lin(vec3f(0.92, 0.16, 0.12)), 0.45, 50.0); }
        if (orange <= best + 0.001) { return Mat(srgb_to_lin(vec3f(1.0, 0.56, 0.1)), 0.45, 50.0); }
        return Mat(srgb_to_lin(vec3f(1.0, 0.86, 0.2)), 0.55, 60.0);
    }
    if (m == 2) {
        if (ml_bands(p) < 0.004) { return Mat(srgb_to_lin(vec3f(0.24, 0.23, 0.24)), 0.6, 40.0); }
        let q = p - vec3f(0.5, 0.0, 0.0);
        let ring = length(q.yz);
        if (abs(q.x) > 0.455 && ring < 0.24) {
            // End grain on the faces: growth rings.
            let rings = 0.5 + 0.5 * sin(ring * 95.0 + sin(atan2(q.z, q.y) * 3.0) * 1.5);
            return Mat(srgb_to_lin(mix(vec3f(0.80, 0.6, 0.38), vec3f(0.93, 0.76, 0.52), rings)), 0.08, 16.0);
        }
        let grain = 0.5 + 0.5 * sin(atan2(q.z, q.y) * 14.0 + q.x * 6.0 + sin(q.x * 13.0) * 0.8);
        return Mat(srgb_to_lin(mix(vec3f(0.62, 0.38, 0.19), vec3f(0.80, 0.55, 0.30), grain)), 0.16, 22.0);
    }
    return Mat(vec3f(0.92, 0.92, 0.905), 0.2, 38.0);
}

fn model_pivot(m: i32) -> vec3f {
    if (m == 1) { return vec3f(0.02, 0.6, 0.0); }
    if (m == 2) { return vec3f(0.5, 0.0, 0.0); }
    return vec3f(0.08, 0.6, 0.0);
}

struct Rot { cr: f32, sr: f32, cp: f32, sp: f32 }

// View (the model's 2D frame plus depth) to model: undo the pitch, then the roll.
fn to_model(v: vec3f, r: Rot) -> vec3f {
    let a = vec3f(v.x, r.cp * v.y + r.sp * v.z, -r.sp * v.y + r.cp * v.z);
    return vec3f(r.cr * a.x - r.sr * a.z, a.y, r.sr * a.x + r.cr * a.z);
}
// Model to view: roll, then pitch.
fn from_model(v: vec3f, r: Rot) -> vec3f {
    let a = vec3f(r.cr * v.x + r.sr * v.z, v.y, -r.sr * v.x + r.cr * v.z);
    return vec3f(a.x, r.cp * a.y - r.sp * a.z, r.sp * a.y + r.cp * a.z);
}

fn draw_object(p: vec2f, tip: vec2f, down: vec2f, s: f32, roll: f32, pitch: f32, m: i32) -> vec4f {
    let right = vec2f(down.y, -down.x);
    let rel = p - tip;
    let q = vec2f(dot(rel, right), dot(rel, down)) / s;
    if (model2(q, m) > 0.08) { return vec4f(0.0); }
    let rot = Rot(cos(roll), sin(roll), cos(pitch), sin(pitch));
    let pivot = model_pivot(m);
    let ro = to_model(vec3f(q, 0.5) - pivot, rot) + pivot;
    let rd = to_model(vec3f(0.0, 0.0, -1.0), rot);
    var t = 0.0;
    var hit = false;
    var dmin = 1e9;
    var tmin = 0.0;
    for (var i = 0; i < 72; i++) {
        let d = model_sdf(ro + rd * t, m);
        if (d < dmin) { dmin = d; tmin = t; }
        if (d < 0.0004) { hit = true; break; }
        t += d * 0.9;
        if (t > 1.05) { break; }
    }
    let px = 1.0 / (u.scale * s);
    var cov = 1.0;
    if (!hit) {
        cov = clamp(1.0 - dmin / (1.2 * px), 0.0, 1.0);
        if (cov <= 0.0) { return vec4f(0.0); }
        t = tmin;
    }
    let pos = ro + rd * t;
    let nm = model_normal(pos, m);
    let nv = from_model(nm, rot);
    let n = normalize(vec3f(right * nv.x + down * nv.y, nv.z));
    let ao = model_ao(pos, nm, m);
    let mat = model_mat(pos, m);

    let key = normalize(vec3f(-0.45, -0.72, 0.78));
    let fill = normalize(vec3f(0.8, 0.3, 0.5));
    let wrap = clamp((dot(n, key) + 0.38) / 1.38, 0.0, 1.0);
    let sky = 0.55 + 0.45 * n.z;
    var col = mat.albedo * (0.30 * sky * ao + 0.80 * wrap * mix(1.0, ao, 0.55) + 0.14 * max(dot(n, fill), 0.0) * ao);
    let h = normalize(key + vec3f(0.0, 0.0, 1.0));
    col += vec3f(mat.spec) * pow(max(dot(n, h), 0.0), mat.power) * ao;
    // Cool in the creases, a little warm in the light.
    col *= mix(vec3f(0.80, 0.82, 0.90), vec3f(1.0, 0.995, 0.98), ao);
    // A soft dark rim keeps a light model readable on a white page.
    col *= mix(0.42, 1.0, smoothstep(0.0, 0.42, n.z));
    return out_lin(col, cov);
}

// ---------------------------------------------------------------- stars

// A five-pointed star of radius r, its points `rf` of the way in (Inigo Quilez's).
fn sd_star5(p0: vec2f, r: f32, rf: f32) -> f32 {
    let k1 = vec2f(0.809016994375, -0.587785252292);
    let k2 = vec2f(-k1.x, k1.y);
    var p = vec2f(abs(p0.x), p0.y);
    p -= 2.0 * max(dot(k1, p), 0.0) * k1;
    p -= 2.0 * max(dot(k2, p), 0.0) * k2;
    p.x = abs(p.x);
    p.y -= r;
    let ba = rf * vec2f(-k1.y, k1.x) - vec2f(0.0, 1.0);
    let h = clamp(dot(p, ba) / dot(ba, ba), 0.0, r);
    return length(p - ba * h) * sign(p.y * ba.x - p.x * ba.y);
}

// ---------------------------------------------------------------- chrome and rubber

// A studio-ish environment for chrome: a dark room in front, the bright screen behind, two
// softboxes and a horizon.
// Polished aluminium: a bright face, a dark band where it reflects the floor, a hard
// highlight from a softbox up and to the left, and darker grazing edges.
fn chrome_env(n: vec3f) -> vec3f {
    let r = vec3f(2.0 * n.z * n.x, 2.0 * n.z * n.y, 2.0 * n.z * n.z - 1.0);
    let z = r.z;
    var e = 0.50 + 0.36 * smoothstep(0.55, 0.97, z);
    e -= 0.36 * exp(-pow((z - 0.12) / 0.2, 2.0)) * smoothstep(-0.2, 0.6, r.y + 0.3);
    e += 0.12 * exp(-pow((z + 0.55) / 0.25, 2.0));
    e += 1.5 * pow(max(dot(r, normalize(vec3f(-0.7, -0.45, 0.55))), 0.0), 16.0);
    e += 0.45 * pow(max(dot(r, normalize(vec3f(0.75, 0.2, 0.62))), 0.0), 24.0);
    return vec3f(e) * vec3f(0.80, 0.82, 0.86);
}

fn cylinder_normal(side: vec2f, u_: f32) -> vec3f {
    let u2 = clamp(u_, -1.0, 1.0);
    return vec3f(side * u2, sqrt(max(1.0 - u2 * u2, 0.0)));
}

fn rubber(n: vec3f, along: f32) -> vec3f {
    let key = normalize(vec3f(-0.62, -0.5, 0.6));
    let diff = max(dot(n, key), 0.0);
    let h = normalize(key + vec3f(0.0, 0.0, 1.0));
    let sheen = pow(max(dot(n, h), 0.0), 10.0);
    let ridge = 0.5 + 0.5 * sin(along * 46.0);
    var c = vec3f(0.018, 0.018, 0.02) + vec3f(0.05) * diff + vec3f(0.12) * sheen * (0.6 + 0.4 * ridge);
    c *= 0.8 + 0.2 * ridge;
    return c * mix(0.6, 1.0, n.z);
}

// ---------------------------------------------------------------- primitives

struct PIn {
    @location(0) a: vec4f,
    @location(1) b: vec4f,
    @location(2) c: vec4f,
    @location(3) d: vec4f,
    @location(4) e: vec4f,
}
struct POut {
    @builtin(position) pos: vec4f,
    @location(0) p: vec2f,
    @location(1) @interpolate(flat) a: vec4f,
    @location(2) @interpolate(flat) b: vec4f,
    @location(3) @interpolate(flat) c: vec4f,
    @location(4) @interpolate(flat) d: vec4f,
    @location(5) @interpolate(flat) e: vec4f,
}

@vertex
fn vs_prim(@builtin(vertex_index) vi: u32, i: PIn) -> POut {
    let p0 = i.a.xy;
    let p1 = i.a.zw;
    let ext = i.b.z;
    let v = p1 - p0;
    let l = length(v);
    let dir = select(vec2f(1.0, 0.0), v / max(l, 1e-6), l > 1e-4);
    let nrm = vec2f(-dir.y, dir.x);
    let corner = vec2f(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = p0 + dir * mix(-ext, l + ext, corner.x) + nrm * mix(-ext, ext, corner.y);
    var o: POut;
    o.pos = to_clip(p);
    o.p = p;
    o.a = i.a;
    o.b = i.b;
    o.c = i.c;
    o.d = i.d;
    o.e = i.e;
    return o;
}

@fragment
fn fs_prim(i: POut) -> @location(0) vec4f {
    let k = i32(i.b.w + 0.5);
    let p = i.p;
    switch k {
        case 0: { // DISC: radius b.x, feather b.y
            let d = length(p - i.a.xy) - i.b.x;
            var cov = aa(d);
            if (i.b.y > 0.0) { cov = 1.0 - smoothstep(-i.b.y, i.b.y + 1.0 / u.scale, d); }
            return out_srgb(i.c.rgb, i.c.a * cov);
        }
        case 1: { // RING: radius b.x, width b.y; dashes d.x, duty d.y, phase d.z
            let rel = p - i.a.xy;
            let rr = length(rel);
            var cov = aa(abs(rr - i.b.x) - i.b.y * 0.5);
            if (i.d.x > 0.5) {
                let period = TAU * i.b.x / i.d.x;
                let ang = atan2(rel.y, rel.x) + i.d.z;
                let s = fract(ang / TAU * i.d.x) * period;
                let on = i.d.y * period;
                let edge = max(-s, s - on);
                cov *= aa(min(edge, period - s));
            }
            return out_srgb(i.c.rgb, i.c.a * cov);
        }
        case 2: { // PILL: radius b.x, outline b.y px; fill c, outline d
            let d = sd_seg(p, i.a.xy, i.a.zw) - i.b.x;
            let ow = i.b.y / u.scale;
            let fill_cov = aa(d + ow);
            let band = max(aa(d) - fill_cov, 0.0);
            let fa = i.c.a * fill_cov;
            let oa = i.d.a * band;
            let pm = i.c.rgb * fa + i.d.rgb * oa;
            let a = fa + oa;
            return out_pm(pm, a);
        }
        case 3: { // CRACK: half-width b.x, alpha b.y; dark c, glint d
            let off = vec2f(0.9, 1.0) / u.scale;
            let dg = sd_seg(p - off, i.a.xy, i.a.zw) - i.b.x * 0.6;
            let dd = sd_seg(p, i.a.xy, i.a.zw) - i.b.x;
            let ga = i.d.a * aa(dg) * i.b.y;
            let da = i.c.a * aa(dd) * i.b.y;
            let pm = i.c.rgb * da + i.d.rgb * ga * (1.0 - da);
            let a = da + ga * (1.0 - da);
            return out_pm(pm, a);
        }
        case 4: { // OBJECT: size b.x; tip c.xy, down c.zw; roll d.x, pitch d.y, model d.w
            return draw_object(p, i.c.xy, normalize(i.c.zw), i.b.x, i.d.x, i.d.y, i32(i.d.w + 0.5));
        }
        case 5: { // OBJECT_SHADOW: size b.x, blur b.y; tip c.xy, down c.zw; alpha d.x, model d.z
            let down = normalize(i.c.zw);
            let right = vec2f(down.y, -down.x);
            let rel = p - i.c.xy;
            let q = vec2f(dot(rel, right), dot(rel, down)) / i.b.x;
            let d = model2(q, i32(i.d.z + 0.5)) * i.b.x;
            let a = i.d.x * (1.0 - smoothstep(-i.b.y, i.b.y * 1.15, d));
            return vec4f(0.0, 0.0, 0.0, a);
        }
        case 6: { // DIM: spot c (x, y, radius, amount); focus rect d; e = (focus, dim, radius, border)
            let spot = i.c.w;
            let focus = i.e.x;
            let dc = length(p - i.c.xy) - i.c.z;
            let hole_c = spot * (1.0 - smoothstep(-0.22 * i.c.z, 0.0, dc));
            let c = (i.d.xy + i.d.zw) * 0.5;
            let half = abs(i.d.zw - i.d.xy) * 0.5;
            let dr = sd_rbox2(p, c, half + vec2f(0.001), min(i.e.z, min(half.x, half.y)));
            let hole_r = focus * aa(dr);
            let amount = max(spot, focus) * i.e.y * (1.0 - max(hole_c, hole_r));
            let line = aa(abs(dr) - 1.0) * focus * i.e.w;
            let glow = focus * i.e.w * 0.22 * exp(-max(dr, 0.0) * 0.12) * step(0.0, dr);
            let ring = spot * 0.22 * aa(abs(dc) - 0.6);
            let white = clamp(line + glow + ring, 0.0, 1.0);
            let a = white + amount * (1.0 - white);
            return vec4f(vec3f(white), a);
        }
        case 7: { // CHROME_CAP: a short flat-ended tube from a to b; radius b.x, tint b.y
            let ba = i.a.zw - i.a.xy;
            let l = max(length(ba), 1e-4);
            let dir = ba / l;
            let nrm = vec2f(-dir.y, dir.x);
            let rel = p - i.a.xy;
            let x = dot(rel, dir);
            let y = dot(rel, nrm);
            let rr = min(i.b.x * 0.3, l * 0.3);
            let q = vec2f(abs(x - l * 0.5) - l * 0.5, abs(y) - i.b.x) + vec2f(rr);
            let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - rr;
            let n = cylinder_normal(nrm, y / i.b.x);
            var col = chrome_env(n) * mix(1.08, 0.72, i.b.y);
            col *= mix(0.5, 1.0, smoothstep(0.0, 0.45, n.z));
            let lip = smoothstep(0.0, 2.0 / u.scale, min(x, l - x));
            col *= 0.62 + 0.38 * lip;
            return out_lin(col, aa(d));
        }
        case 8: { // SHARD: half-width b.x, colour c
            let d = sd_seg(p, i.a.xy, i.a.zw) - i.b.x;
            return out_srgb(i.c.rgb, i.c.a * aa(d));
        }
        case 9: { // BLADE: emitter a.xy to tip a.zw; core radius b.x, glow radius b.y; colour c; flicker d.x
            let d = sd_seg(p, i.a.xy, i.a.zw);
            let core = 1.0 - smoothstep(i.b.x * 0.55, i.b.x + 0.75 / u.scale, d);
            let glow = exp(-pow(d / i.b.y, 2.0) * 2.2) * i.c.a * (0.84 + 0.16 * i.d.x);
            let a = clamp(max(core, glow), 0.0, 1.0);
            return out_srgb(mix(i.c.rgb, vec3f(1.0), core * 0.93), a);
        }
        case 10: { // STAR: centre a.xy; radius b.x; points b.y (4: a twinkle, 5: a cartoon star); fill c; outline d; turn e.x
            let rel = p - i.a.xy;
            let cs = cos(i.e.x);
            let sn = sin(i.e.x);
            let q = vec2f(cs * rel.x + sn * rel.y, -sn * rel.x + cs * rel.y);
            let r = i.b.x;
            if (i.b.y < 4.5) {
                let a = abs(q) / r;
                let arm = min(a.x + a.y / 0.17, a.y + a.x / 0.17);
                let shape = 1.0 - smoothstep(0.8, 1.0 + 1.5 / (r * u.scale), arm);
                let glow = exp(-length(q) / (0.3 * r)) * 0.75;
                let core = exp(-length(q) / (0.11 * r));
                let alpha = clamp(max(shape, glow), 0.0, 1.0) * i.c.a;
                return out_srgb(mix(i.c.rgb, vec3f(1.0), clamp(core + shape * 0.25, 0.0, 1.0)), alpha);
            }
            let d = sd_star5(vec2f(q.x, -q.y), r, 0.5);
            let ow = i.d.a / u.scale;
            let fill_cov = aa(d + ow);
            let band = max(aa(d) - fill_cov, 0.0);
            let fa = i.c.a * fill_cov;
            let oa = band * min(i.c.a * 1.5, 1.0);
            return out_pm(i.c.rgb * fa + i.d.rgb * oa, fa + oa);
        }
        default: {
            return vec4f(0.0);
        }
    }
}

// ---------------------------------------------------------------- ribbons

struct RIn {
    @location(0) pos: vec2f,
    @location(1) ofs: vec2f,
    @location(2) aux: vec4f,
    @location(3) color: vec4f,
    @location(4) style: vec4f,
}
struct ROut {
    @builtin(position) pos: vec4f,
    @location(0) ofs: vec2f,
    @location(1) aux: vec4f,
    @location(2) color: vec4f,
    @location(3) @interpolate(flat) style: vec4f,
}

@vertex
fn vs_ribbon(i: RIn) -> ROut {
    var o: ROut;
    o.pos = to_clip(i.pos);
    o.ofs = i.ofs;
    o.aux = i.aux;
    o.color = i.color;
    o.style = i.style;
    return o;
}

@fragment
fn fs_ribbon(i: ROut) -> @location(0) vec4f {
    let r = i.aux.y;
    let dist = length(i.ofs) * r;
    let k = i32(i.style.x + 0.5);
    switch k {
        case 0: { // SOLID
            return out_srgb(i.color.rgb, i.color.a * aa(dist - r));
        }
        case 1: { // GLOW: a white-hot core (style.y of the radius) in a coloured halo
            let core_r = r * i.style.y;
            let core = aa(dist - core_r);
            let body = aa(dist - r * 0.55);
            let halo = exp(-pow(dist / (r * 0.75), 2.0));
            let a = i.color.a * clamp(max(max(core, body * 0.95), halo * 0.8), 0.0, 1.0);
            let c = mix(i.color.rgb, vec3f(1.0, 0.95, 0.96), core * 0.9);
            return out_srgb(c, a);
        }
        case 2: { // SHADOW: a soft band
            let g = exp(-(dist * dist) / (r * r) * 2.6);
            return vec4f(0.0, 0.0, 0.0, i.color.a * g);
        }
        case 3: { // CHROME
            let side = normalize(i.aux.zw);
            let n = cylinder_normal(side, i.ofs.x);
            var col = chrome_env(n);
            col *= mix(0.5, 1.0, smoothstep(0.0, 0.45, n.z));
            return out_lin(col, aa(dist - r));
        }
        case 4: { // RUBBER
            let side = normalize(i.aux.zw);
            let n = cylinder_normal(side, i.ofs.x);
            return out_lin(rubber(n, i.aux.x), aa(dist - r));
        }
        case 5: { // GLOSS: glossy lacquer in the vertex colour (the wand)
            let side = normalize(i.aux.zw);
            let n = cylinder_normal(side, i.ofs.x);
            let key = normalize(vec3f(-0.62, -0.5, 0.6));
            let h = normalize(key + vec3f(0.0, 0.0, 1.0));
            let diff = 0.3 + 0.7 * max(dot(n, key), 0.0);
            let spec = pow(max(dot(n, h), 0.0), 70.0) * 0.9 + pow(max(dot(n, h), 0.0), 8.0) * 0.08;
            var col = srgb_to_lin(i.color.rgb) * diff + vec3f(spec);
            col *= mix(0.55, 1.0, smoothstep(0.0, 0.4, n.z));
            return out_lin(col, i.color.a * aa(dist - r));
        }
        case 6: { // WOOD: a turned wooden handle
            let side = normalize(i.aux.zw);
            let n = cylinder_normal(side, i.ofs.x);
            let along = i.aux.x;
            let grain = 0.5 + 0.5 * sin(i.ofs.x * 7.0 + along * 2.2 + sin(along * 5.0 + i.ofs.x * 2.5) * 1.6);
            let base = srgb_to_lin(mix(vec3f(0.56, 0.34, 0.17), vec3f(0.80, 0.55, 0.30), grain));
            let key = normalize(vec3f(-0.62, -0.5, 0.6));
            let h = normalize(key + vec3f(0.0, 0.0, 1.0));
            var col = base * (0.35 + 0.75 * max(dot(n, key), 0.0)) + vec3f(0.12) * pow(max(dot(n, h), 0.0), 24.0);
            col *= mix(0.5, 1.0, smoothstep(0.0, 0.45, n.z));
            return out_lin(col, aa(dist - r));
        }
        default: {
            return vec4f(0.0);
        }
    }
}
