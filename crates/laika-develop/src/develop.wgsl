// Laika develop pipeline (plan.md Phase 3). One fullscreen pass over the
// linear camera-RGB source texture. `develop()` runs the eight prototype
// stages; `fs` composites before/after at the split fraction.

struct U {
  a: vec4<f32>, // temperature, tint, exposure, contrast
  b: vec4<f32>, // highlights, shadows, whites, blacks
  c: vec4<f32>, // texture, clarity, vibrance, saturation
  d: vec4<f32>, // split, angle_rad, target_w, target_h
  e: vec4<f32>, // src_w, src_h, flip_h, flip_v
  f: vec4<f32>, // U08 crop rect x, y, w, h (normalized source space)
  g: vec4<f32>, // U18 tone-curve outputs at 20/40/60/80%
  h: vec4<f32>, // U18 hue shifts R,O,Y,G
  i: vec4<f32>, // U18 hue shifts A,B,P,M
  j: vec4<f32>, // U18 saturation R,O,Y,G
  k: vec4<f32>, // U18 saturation A,B,P,M
  l: vec4<f32>, // U18 luminance R,O,Y,G
  m: vec4<f32>, // U18 luminance A,B,P,M
  n: vec4<f32>, // U18 sharpen, radius, lum NR, color NR
  o: vec4<f32>, // U18 distortion, CA, V15 rotation 0-3, crop frame view
  p: vec4<f32>, // effects: dehaze, vignette, grain; .w = display-referred source
  cg0: vec4<f32>, // grading: shadow hue, sat, lum, midtone hue
  cg1: vec4<f32>, // midtone sat, lum, highlight hue, sat
  cg2: vec4<f32>, // highlight lum, global hue, sat, lum
  cg3: vec4<f32>, // blending, balance, unused, unused
  wb: vec4<f32>, // as-shot rgb multipliers + pad
  cam0: vec4<f32>, // cam_to_xyz rows (xyz in .xyz)
  cam1: vec4<f32>,
  cam2: vec4<f32>,
  warp0: vec4<f32>, // perspective warp G row 0 (.xyz), .w = active flag
  warp1: vec4<f32>, // G row 1 (.xyz)
  warp2: vec4<f32>, // G row 2 (.xyz)
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VOut {
  let t = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  var o: VOut;
  o.pos = vec4<f32>(t * 2.0 - 1.0, 0.0, 1.0);
  o.uv = vec2<f32>(t.x, 1.0 - t.y);
  return o;
}

fn lum(c: vec3<f32>) -> f32 {
  return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
  return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

// U18: manual optics — radial distortion about the frame center, then
// the crop mapping. Positive pulls edges inward (fixes barrel bulge)
// and always samples in-bounds; negative may smear edges at extremes
// (crop in to hide). k1 = 0 is the exact identity.
fn optics_uv(uv: vec2<f32>) -> vec2<f32> {
  let k = u.o.x / 100.0 * 0.3;
  if (abs(k) < 0.00001) {
    return uv;
  }
  let d = uv - vec2<f32>(0.5, 0.5);
  let r2 = dot(d, d);
  return vec2<f32>(0.5, 0.5) + d * (1.0 - k * r2 * 4.0);
}

// U18: lateral CA — R/B radial scale about the center (G is reference).
fn ca_uv(uv: vec2<f32>, channel: f32) -> vec2<f32> {
  let c = u.o.y / 100.0 * 0.005;
  let s = 1.0 + c * channel;
  return vec2<f32>(0.5, 0.5) + (uv - vec2<f32>(0.5, 0.5)) * s;
}

// U18: point tone curve through (0,0), four points at fixed
// 20/40/60/80%, and (1,1). Identity at panel defaults.
fn apply_curve_p(x: f32, ys: vec4<f32>) -> f32 {
  if (x <= 0.2) {
    return mix(0.0, ys.x, clamp(x / 0.2, 0.0, 1.0));
  }
  if (x <= 0.4) {
    return mix(ys.x, ys.y, clamp((x - 0.2) / 0.2, 0.0, 1.0));
  }
  if (x <= 0.6) {
    return mix(ys.y, ys.z, clamp((x - 0.4) / 0.2, 0.0, 1.0));
  }
  if (x <= 0.8) {
    return mix(ys.z, ys.w, clamp((x - 0.6) / 0.2, 0.0, 1.0));
  }
  return mix(ys.w, 1.0, clamp((x - 0.8) / 0.2, 0.0, 1.0));
}

fn rgb_to_hsv(c: vec3<f32>) -> vec3<f32> {
  let mx = max(c.r, max(c.g, c.b));
  let mn = min(c.r, min(c.g, c.b));
  let d = mx - mn;
  var h = 0.0;
  if (d > 0.00001) {
    if (mx == c.r) {
      h = fract((c.g - c.b) / d / 6.0 + 1.0) * 360.0;
    } else if (mx == c.g) {
      h = ((c.b - c.r) / d + 2.0) * 60.0;
    } else {
      h = ((c.r - c.g) / d + 4.0) * 60.0;
    }
  }
  let s = select(d / max(mx, 1e-5), 0.0, mx < 0.00001);
  return vec3<f32>(h, clamp(s, 0.0, 1.0), mx);
}

fn hsv_to_rgb(c: vec3<f32>) -> vec3<f32> {
  let h = c.x / 60.0;
  let s = clamp(c.y, 0.0, 1.0);
  let v = c.z;
  let i = floor(h);
  let f = h - i;
  let p = v * (1.0 - s);
  let q = v * (1.0 - s * f);
  let t = v * (1.0 - s * (1.0 - f));
  let m = i32(u32(i) % 6u);
  if (m == 0) { return vec3<f32>(v, t, p); }
  if (m == 1) { return vec3<f32>(q, v, p); }
  if (m == 2) { return vec3<f32>(p, v, t); }
  if (m == 3) { return vec3<f32>(p, q, v); }
  if (m == 4) { return vec3<f32>(t, p, v); }
  return vec3<f32>(v, p, q);
}

// U18: Color Mix — per-sector hue rotation (±30°), saturation and
// luminance scales with triangular sector weights. Identity at zeros.
// Rows ride per call so before/after can differ.
// Triangular sector weight (60° wide) with hue wraparound.
fn sector_w(h: f32, center: f32) -> f32 {
  var dh = abs(h - center);
  dh = min(dh, 360.0 - dh);
  return clamp(1.0 - dh / 60.0, 0.0, 1.0);
}

fn apply_hsl_p(srgb: vec3<f32>, h: vec4<f32>, i: vec4<f32>, j: vec4<f32>, k: vec4<f32>, l: vec4<f32>, m: vec4<f32>) -> vec3<f32> {
  let hsv = rgb_to_hsv(clamp(srgb, vec3<f32>(0.0), vec3<f32>(1.0)));
  let w_r = sector_w(hsv.x, 0.0);
  let w_o = sector_w(hsv.x, 30.0);
  let w_y = sector_w(hsv.x, 60.0);
  let w_g = sector_w(hsv.x, 120.0);
  let w_a = sector_w(hsv.x, 180.0);
  let w_b = sector_w(hsv.x, 240.0);
  let w_p = sector_w(hsv.x, 270.0);
  let w_m = sector_w(hsv.x, 300.0);
  let h_rot = (w_r * h.x + w_o * h.y + w_y * h.z + w_g * h.w
    + w_a * i.x + w_b * i.y + w_p * i.z + w_m * i.w) / 100.0 * 30.0;
  let s_mul = (1.0 + w_r * j.x / 100.0 * 0.9) * (1.0 + w_o * j.y / 100.0 * 0.9)
    * (1.0 + w_y * j.z / 100.0 * 0.9) * (1.0 + w_g * j.w / 100.0 * 0.9)
    * (1.0 + w_a * k.x / 100.0 * 0.9) * (1.0 + w_b * k.y / 100.0 * 0.9)
    * (1.0 + w_p * k.z / 100.0 * 0.9) * (1.0 + w_m * k.w / 100.0 * 0.9);
  let v_mul = (1.0 + w_r * l.x / 100.0 * 0.9) * (1.0 + w_o * l.y / 100.0 * 0.9)
    * (1.0 + w_y * l.z / 100.0 * 0.9) * (1.0 + w_g * l.w / 100.0 * 0.9)
    * (1.0 + w_a * m.x / 100.0 * 0.9) * (1.0 + w_b * m.y / 100.0 * 0.9)
    * (1.0 + w_p * m.z / 100.0 * 0.9) * (1.0 + w_m * m.w / 100.0 * 0.9);
  let out = hsv_to_rgb(vec3<f32>(hsv.x + h_rot, hsv.y * s_mul, hsv.z * v_mul));
  return out;
}

// U08: output uv -> source uv for crop + straighten + flip. Mirrors
// `laika_core::edit::crop_sample` exactly: flips in output space, then
// the crop rect, then content derotation about the frame center.
// Positive angles turn the picture clockwise as viewed.
// V15: the rect lives in display (post-rotation) space; the tail
// unrotates into source pixels. Output dims swap on odd rotations
// (crop_target), so the frame aspect below is the display aspect.
fn geom_uv(uv: vec2<f32>) -> vec2<f32> {
  var q = uv;
  if (u.e.z > 0.5) { q.x = 1.0 - q.x; }
  if (u.e.w > 0.5) { q.y = 1.0 - q.y; }
  var src = u.f.xy + q * u.f.zw;
  let rot = u.o.z;
  let odd = (rot > 0.5 && rot < 1.5) || rot > 2.5;
  let a = u.d.y;
  if (abs(a) > 0.0001) {
    var asp = u.e.x / max(u.e.y, 1.0);
    if (odd) { asp = 1.0 / max(asp, 0.0001); }
    var d = (src - vec2<f32>(0.5, 0.5)) * vec2<f32>(asp, 1.0);
    let s = sin(a);
    let c = cos(a);
    d = vec2<f32>(d.x * c + d.y * s, -d.x * s + d.y * c);
    src = vec2<f32>(0.5, 0.5) + d / vec2<f32>(asp, 1.0);
  }
  // Upright/Transform perspective warp in display space (mirrors
  // `laika_develop::warp_display_uv`): corrected centered coords through
  // G to homogeneous source centered coords. Skipped entirely when
  // inactive so unwarped renders stay bit-identical.
  if (u.warp0.w > 0.5) {
    var wa = u.e.x / max(u.e.y, 1.0);
    if (odd) { wa = u.e.y / max(u.e.x, 1.0); }
    let p = vec3<f32>((src.x - 0.5) * wa, src.y - 0.5, 1.0);
    let hw = dot(u.warp2.xyz, p);
    if (abs(hw) < 1e-6) {
      src = vec2<f32>(-1.0, -1.0);
    } else {
      src = vec2<f32>(dot(u.warp0.xyz, p) / hw / wa + 0.5, dot(u.warp1.xyz, p) / hw + 0.5);
    }
  }
  if (rot > 0.5 && rot < 1.5) { src = vec2<f32>(src.y, 1.0 - src.x); }
  else if (rot > 1.5 && rot < 2.5) { src = vec2<f32>(1.0 - src.x, 1.0 - src.y); }
  else if (rot > 2.5) { src = vec2<f32>(1.0 - src.y, src.x); }
  return src;
}

// Default rendering look: baseline exposure + ACES filmic fit (Narkowicz).
// Measured against in-camera JPEGs of the fixtures, this tracks their
// brightness closely (per-pixel luma error 5.9 vs 8.8 for plain Reinhard,
// which mapped linear white to 0.5 — Develop visibly darkened when its
// render replaced the bright embedded preview). Shoulder rolls off to 1.
const BASE_GAIN = 2.3784142; // +1.25 EV

// Display-referred sources (rasters through the sRGB bridge) are already a
// finished rendering: their default look is the identity, and the stages
// pivot at gain 1 instead of BASE_GAIN.
fn display_referred() -> bool {
  return u.p.w > 0.5;
}

fn look_gain() -> f32 {
  return select(BASE_GAIN, 1.0, display_referred());
}

fn base_look(c: vec3<f32>) -> vec3<f32> {
  let x = max(c, vec3<f32>(0.0)) * BASE_GAIN;
  let y = (x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14);
  return clamp(y, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Default (unedited) params: Temp 5480, Tint +6, rest 0.
const DEF_A = vec4<f32>(5480.0, 6.0, 0.0, 0.0);
const DEF_B = vec4<f32>(0.0, 0.0, 0.0, 0.0);
const DEF_C = vec4<f32>(0.0, 0.0, 0.0, 0.0);

const IDENT_CURVE = vec4<f32>(0.2, 0.4, 0.6, 0.8);

// U18: `curve`/`detail` ride per call so before/after can differ while
// sharing geometry (crop, distortion) for alignment.
fn develop(uv: vec2<f32>, a: vec4<f32>, b: vec4<f32>, c: vec4<f32>, curve: vec4<f32>, detail: vec4<f32>, fx: vec4<f32>) -> vec3<f32> {
  let texel = 1.0 / u.e.xy;
  var col = max(tap(uv), vec3<f32>(0.0));

  // U18: noise reduction first (linear): luminance blends toward a wide
  // blur, chroma harder toward an even wider one.
  if (detail.z > 0.5 || detail.w > 0.5) {
    let o2 = texel * 2.0;
    let blur2 = 0.25 * (tap(uv + vec2<f32>(o2.x, 0.0)) + tap(uv - vec2<f32>(o2.x, 0.0))
      + tap(uv + vec2<f32>(0.0, o2.y)) + tap(uv - vec2<f32>(0.0, o2.y)));
    let l0 = lum(col);
    let l1 = lum(blur2);
    col = mix(col, vec3<f32>(l1) * (col / max(l0, 1e-4)), detail.z / 100.0 * 0.85);
    let o4 = texel * 4.0;
    let blur4 = 0.25 * (tap(uv + vec2<f32>(o4.x, 0.0)) + tap(uv - vec2<f32>(o4.x, 0.0))
      + tap(uv + vec2<f32>(0.0, o4.y)) + tap(uv - vec2<f32>(0.0, o4.y)));
    col = mix(col, blur4, detail.w / 100.0 * 0.9);
  }

  // 6. Texture and clarity: local contrast from 4-tap cross blurs.
  // Clarity samples 12 px out with a midtone mask, texture 2 px.
  if (abs(c.y) > 0.5) {
    let o = texel * 12.0;
    let blur = 0.25 * (tap(uv + vec2<f32>(o.x, 0.0)) + tap(uv - vec2<f32>(o.x, 0.0))
      + tap(uv + vec2<f32>(0.0, o.y)) + tap(uv - vec2<f32>(0.0, o.y)));
    let mid = 1.0 - abs(lum(col) - 0.4) * 1.2;
    col = max(col + (col - blur) * (c.y / 100.0) * clamp(mid, 0.0, 1.0), vec3<f32>(0.0));
  }
  if (abs(c.x) > 0.5) {
    let o = texel * 2.0;
    let blur = 0.25 * (tap(uv + vec2<f32>(o.x, 0.0)) + tap(uv - vec2<f32>(o.x, 0.0))
      + tap(uv + vec2<f32>(0.0, o.y)) + tap(uv - vec2<f32>(0.0, o.y)));
    col = max(col + (col - blur) * (c.x / 50.0), vec3<f32>(0.0));
  }

  // 1. White balance: as-shot multipliers, then Temperature as a shift
  // on the mired scale (perceptually even: 5480→3600 K and 5480→8960 K
  // are no longer 2× apart in strength), ~0.9 stop of R/B per 100 mired
  // like a real illuminant change; tint scales G. Display-referred sources
  // are already balanced, so their tint counts from the default.
  let mired = 1.0e6 / 5480.0 - 1.0e6 / max(a.x, 1000.0);
  let stops = mired / 100.0 * 0.9;
  let rb = pow(2.0, stops * 0.5);
  let tint = select(a.y, a.y - DEF_A.y, display_referred());
  col = col * u.wb.rgb * vec3<f32>(rb, 1.0 - tint / 300.0, 1.0 / rb);

  // 2. Exposure.
  col *= pow(2.0, a.z);

  // Dehaze. Positive removes a veil estimated from the dark channel (the
  // smallest channel carries the added airlight) and gives back part of
  // the lost brightness; negative lays a flat veil over the scene.
  if (abs(fx.x) > 0.5) {
    let k = fx.x / 100.0;
    if (k > 0.0) {
      let before_l = lum(col);
      let dark = min(col.r, min(col.g, col.b));
      col = max(col - vec3<f32>(dark * k * 0.7), vec3<f32>(0.0));
      let after_l = max(lum(col), 1e-6);
      col = col * clamp(mix(1.0, before_l / after_l, 0.5), 1.0, 2.0);
    } else {
      let veil = 0.06 / look_gain();
      col = mix(col, vec3<f32>(lum(col) * 0.6 + veil), -k * 0.45);
    }
  }

  // 3. Whites and blacks, endpoint remap.
  col = col * (1.0 + b.z * 0.005) + b.w * 0.001;

  // 4. Highlights and shadows, luminance-masked soft compression and lift.
  let l = lum(col);
  let hi = smoothstep(0.5, 1.0, l);
  col = mix(col, col * (1.0 + b.x * 0.008), hi);
  let sh = 1.0 - smoothstep(0.0, 0.5, l);
  col = mix(col, col * (1.0 + b.y * 0.008) + b.y * 0.0005, sh);

  // 5. Contrast: a power curve on luminance about display middle gray
  // (hue-preserving). Multiplicative, so shadows darken proportionally
  // instead of being pushed below zero (the old additive form sent ~85%
  // of a night frame to black at +12 and fogged blacks at -10).
  if (abs(a.w) > 0.5) {
    let lc = max(lum(col), 1e-6);
    let pivot = 0.18 / look_gain();
    let k = a.w / 100.0 * 0.5;
    col = col * clamp(pow(lc / pivot, k), 0.02, 50.0);
  }

  // 7. Vibrance and saturation about luminance. Vibrance favors muted
  // colors (relative chroma); the gain is capped so no channel is pushed
  // below zero (clipping shifts hue and reads as harsh).
  let l2 = lum(col);
  let mx = max(col.r, max(col.g, col.b));
  let mn = min(col.r, min(col.g, col.b));
  let rel_sat = clamp((mx - mn) / max(mx, 1e-6), 0.0, 1.0);
  let vib = c.z / 100.0 * 0.6 * (1.0 - rel_sat);
  var gain = max(1.0 + vib + c.w / 100.0, 0.0);
  if (gain > 1.0 && mn < l2) {
    gain = min(gain, l2 / max(l2 - mn, 1e-6));
  }
  col = max(vec3<f32>(l2) + (col - vec3<f32>(l2)) * gain, vec3<f32>(0.0));

  // U18: sharpening (unsharp mask at the Radius scale, fixed small
  // threshold so flat skies don't grit).
  if (detail.x > 0.5) {
    let o = texel * max(detail.y, 0.25);
    let blur = 0.25 * (tap(uv + vec2<f32>(o.x, 0.0)) + tap(uv - vec2<f32>(o.x, 0.0))
      + tap(uv + vec2<f32>(0.0, o.y)) + tap(uv - vec2<f32>(0.0, o.y)));
    let edge = col - blur;
    let w = smoothstep(0.002, 0.03, abs(lum(edge)));
    col = max(col + edge * (detail.x / 100.0) * w, vec3<f32>(0.0));
  }

  // 8. Camera to display transform, tone curve, sRGB encode.
  // u.cam holds camera → linear sRGB rows (built in decode.rs).
  let srgb_lin = mat3x3f(
    vec3<f32>(u.cam0.x, u.cam1.x, u.cam2.x),
    vec3<f32>(u.cam0.y, u.cam1.y, u.cam2.y),
    vec3<f32>(u.cam0.z, u.cam1.z, u.cam2.z)) * col;
  var tm = clamp(srgb_lin, vec3<f32>(0.0), vec3<f32>(1.0));
  if (!display_referred()) {
    tm = base_look(srgb_lin);
  }
  // U18: point tone curve on the tonemapped value, then sRGB encode.
  let curved = vec3<f32>(apply_curve_p(tm.r, curve), apply_curve_p(tm.g, curve), apply_curve_p(tm.b, curve));
  let lo = curved * 12.92;
  let hi3 = 1.055 * pow(clamp(curved, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
  return select(hi3, lo, curved < vec3<f32>(0.0031308));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
  // One shared geometric mapping: before/after can never misalign.
  // Optics (lens) acts first, then the crop window.
  let luv = optics_uv(in.uv);
  let suv = geom_uv(luv);
  // Crop tool: the full straightened frame is shown under the box; area
  // outside the source reads as empty canvas, not clamped edge pixels.
  if (u.o.w > 0.5 && (any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0)))) {
    return vec4<f32>(0.07, 0.07, 0.07, 1.0);
  }
  // Perspective warp without constrain: uncovered area reads white
  // (Lightroom). Unwarped renders keep clamping edge texels.
  if (u.warp0.w > 0.5 && u.o.w < 0.5
    && (any(suv < vec2<f32>(-1e-4)) || any(suv > vec2<f32>(1.0 + 1e-4)))) {
    return vec4<f32>(1.0, 1.0, 1.0, 1.0);
  }
  // Zero rows = the defined BEFORE reference (pipeline defaults).
  // Only the side of the split this pixel lies on is developed — each
  // `develop` call is up to ~25 texture taps, so computing both and
  // mixing doubled the per-pixel cost (and After-only renders paid for a
  // Before they never showed).
  let zh = vec4<f32>(0.0);
  if (in.uv.x < u.d.x) {
    // (CA is color: before samples straight, like the crop rule that
    // only geometry is shared.)
    let before = develop(suv, DEF_A, DEF_B, DEF_C, IDENT_CURVE, zh, zh);
    return vec4<f32>(apply_hsl_p(before, zh, zh, zh, zh, zh, zh), 1.0);
  }
  var after: vec3<f32>;
  if (abs(u.o.y) > 0.5) {
    // U18: lateral CA — R/B sampled at radially scaled positions.
    let ruv = ca_uv(suv, 1.0);
    let buv = ca_uv(suv, -1.0);
    after = vec3<f32>(develop(ruv, u.a, u.b, u.c, u.g, u.n, u.p).r, develop(suv, u.a, u.b, u.c, u.g, u.n, u.p).g, develop(buv, u.a, u.b, u.c, u.g, u.n, u.p).b);
  } else {
    after = develop(suv, u.a, u.b, u.c, u.g, u.n, u.p);
  }
  // U18: Color Mix grades the display-referred result; before is bare.
  var out = apply_hsl_p(after, u.h, u.i, u.j, u.k, u.l, u.m);
  out = color_grade(out);
  // Post-crop effects act on the output frame (skipped in the crop tool's
  // full-frame view, where the frame is not the final one).
  if (u.o.w < 0.5) {
    out = post_effects(out, in.uv);
  }
  return vec4<f32>(out, 1.0);
}

// Zero-luminance tint direction for a grading wheel (hue deg, sat 0..100).
fn grade_tint(hue: f32, sat: f32) -> vec3<f32> {
  let c = hsv_to_rgb(vec3<f32>(fract(hue / 360.0) * 360.0, 1.0, 1.0));
  return (c - vec3<f32>(lum(c))) * (sat / 100.0);
}

// Color Grading on the display-referred result (Lightroom-style): tints
// and luminance offsets for shadows / midtones / highlights weighted by
// luminance, plus a global tint. Balance moves the shadow/highlight split;
// Blending widens the overlap between ranges. The three weights always
// sum to one, so a flat wheel set is exactly the identity.
fn color_grade(c: vec3<f32>) -> vec3<f32> {
  let amount = u.cg0.y + abs(u.cg0.z) + u.cg1.x + abs(u.cg1.y) + u.cg1.w
    + abs(u.cg2.x) + u.cg2.z + abs(u.cg2.w);
  if (amount < 0.5) {
    return c;
  }
  let l = clamp(lum(c), 0.0, 1.0);
  let pivot = clamp(0.5 - u.cg3.y / 100.0 * 0.3, 0.15, 0.85);
  let width = 0.08 + u.cg3.x / 100.0 * 0.42;
  let ws = 1.0 - smoothstep(pivot - width, pivot, l);
  let wh = smoothstep(pivot, pivot + width, l);
  let wm = max(1.0 - ws - wh, 0.0);
  let tint = ws * grade_tint(u.cg0.x, u.cg0.y)
    + wm * grade_tint(u.cg0.w, u.cg1.x)
    + wh * grade_tint(u.cg1.z, u.cg1.w)
    + grade_tint(u.cg2.y, u.cg2.z);
  // Tints scale gently with brightness, and are capped so no channel is
  // pushed below zero (clipping shifts hue and lifts blacks); a small
  // allowance still lets deep shadows take on some color.
  var strength = 0.24 * (0.25 + 0.75 * sqrt(l));
  for (var ch = 0; ch < 3; ch++) {
    if (tint[ch] < -1e-4) {
      strength = min(strength, c[ch] / -tint[ch] + 0.02);
    }
  }
  let lum_shift = (ws * u.cg0.z + wm * u.cg1.y + wh * u.cg2.x + u.cg2.w) / 100.0 * 0.11;
  return clamp(c + tint * strength + vec3<f32>(lum_shift), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn hash21(p: vec2<f32>) -> f32 {
  var q = fract(p * vec2<f32>(123.34, 456.21));
  q += dot(q, q + 45.32);
  return fract(q.x * q.y);
}

// Smooth value noise in [-0.5, 0.5].
fn value_noise(p: vec2<f32>) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let w = f * f * (3.0 - 2.0 * f);
  let a = hash21(i);
  let b = hash21(i + vec2<f32>(1.0, 0.0));
  let c = hash21(i + vec2<f32>(0.0, 1.0));
  let d = hash21(i + vec2<f32>(1.0, 1.0));
  return mix(mix(a, b, w.x), mix(c, d, w.x), w.y) - 0.5;
}

// Vignette and grain on the display-referred output frame. Both scale
// with the frame, not with pixels, so the 720/1280 px previews and a
// full-resolution export look the same.
fn post_effects(c: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
  var col = c;
  let tw = max(u.d.z, 1.0);
  let th = max(u.d.w, 1.0);
  // Vignette: elliptical falloff reaching the corners; negative darkens,
  // positive lifts toward white.
  if (abs(u.p.y) > 0.5) {
    let amt = u.p.y / 100.0;
    let asp = tw / th;
    let d = (uv - vec2<f32>(0.5)) * vec2<f32>(asp, 1.0);
    let r = length(d) / (0.5 * sqrt(asp * asp + 1.0));
    let fall = smoothstep(0.35, 1.05, r);
    if (amt < 0.0) {
      col = col * (1.0 + amt * 0.85 * fall);
    } else {
      col = mix(col, vec3<f32>(1.0), amt * 0.55 * fall);
    }
  }
  // Grain: two octaves of value noise on a frame-relative grid (~1000
  // cells across the long edge), strongest in the midtones.
  if (u.p.z > 0.5) {
    let amt = u.p.z / 100.0;
    let scale = 1000.0 / max(tw, th);
    let gp = uv * vec2<f32>(tw, th) * scale;
    let n = value_noise(gp) * 0.7 + value_noise(gp * 2.3 + vec2<f32>(17.0, 3.0)) * 0.3;
    let l = clamp(dot(col, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0, 1.0);
    let mid = 0.55 + 0.45 * (4.0 * l * (1.0 - l));
    // Mostly multiplicative so saturated colors keep their hue (equal
    // additive noise pushed clipped blues toward white); a small additive
    // part keeps grain visible in the shadows.
    col = col * (1.0 + n * amt * 0.45 * mid) + vec3<f32>(n * amt * 0.04);
  }
  return clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
}
