// Phase 0 develop pass. `scene_fs` runs once into a half-float source texture, standing in for a decoded
// RAW. `fs` is the per-frame develop pass and only samples that texture, as the real pipeline will.

struct U {
    a: vec4<f32>, // temperature, tint, exposure, contrast
    b: vec4<f32>, // highlights, shadows, whites, blacks
    c: vec4<f32>, // texture, clarity, vibrance, saturation
    d: vec4<f32>, // split, time, width, height
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

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var v = 0.0;
    var amp = 0.5;
    for (var k = 0; k < 5; k++) {
        v += amp * vnoise(p);
        p = p * 2.03 + vec2<f32>(17.0, 9.0);
        amp *= 0.5;
    }
    return v;
}

// Linear-light scene, values above 1.0 in the sun and sky glow.
fn scene(uv: vec2<f32>) -> vec3<f32> {
    let aspect = u.d.z / u.d.w;
    let p = vec2<f32>(uv.x * aspect, uv.y);
    let sun = vec2<f32>(0.68 * aspect, 0.44);
    let dist = length(p - sun);

    let horizon = 0.56 + 0.05 * (fbm(vec2<f32>(p.x * 2.0, 3.0)) - 0.5);
    let t = clamp(uv.y / horizon, 0.0, 1.0);
    var sky = mix(vec3<f32>(0.10, 0.20, 0.52), vec3<f32>(1.5, 0.78, 0.36), t * t);
    sky += vec3<f32>(3.2, 1.9, 0.8) * exp(-dist * 7.0);
    sky += vec3<f32>(14.0, 11.0, 7.0) * smoothstep(0.035, 0.03, dist);
    let clouds = smoothstep(0.55, 0.8, fbm(p * vec2<f32>(3.0, 9.0) + vec2<f32>(0.0, 2.0)));
    sky = mix(sky, sky * vec3<f32>(1.25, 0.95, 0.85), clouds * (1.0 - t * 0.5));

    let ridge = 0.64 + 0.07 * (fbm(vec2<f32>(p.x * 3.5 + 5.0, 1.0)) - 0.5);
    let grain = fbm(p * vec2<f32>(9.0, 18.0));
    var ground = vec3<f32>(0.09, 0.07, 0.045) * (0.55 + grain);
    let near = vec3<f32>(0.03, 0.028, 0.02) * (0.4 + fbm(p * 22.0));
    let rim = exp(-abs(uv.y - horizon) * 60.0) * vec3<f32>(1.2, 0.6, 0.25);

    var c = select(sky, ground + rim, uv.y > horizon);
    c = select(c, near, uv.y > ridge);
    return c;
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn finish(c: vec3<f32>) -> vec3<f32> {
    let tm = c * (1.0 + c / 16.0) / (1.0 + c);
    return pow(clamp(tm, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
}

fn tap(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

@fragment
fn scene_fs(in: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(scene(in.uv), 1.0);
}

fn develop(uv: vec2<f32>) -> vec3<f32> {
    let texel = 1.0 / u.d.zw;
    var c = tap(uv);

    // Local contrast. Clarity samples 12 px away, texture 2 px.
    if (abs(u.c.y) > 0.5) {
        let o = texel * 12.0;
        let blur = 0.25 * (tap(uv + vec2<f32>(o.x, 0.0)) + tap(uv - vec2<f32>(o.x, 0.0))
            + tap(uv + vec2<f32>(0.0, o.y)) + tap(uv - vec2<f32>(0.0, o.y)));
        let mid = 1.0 - abs(lum(c) - 0.4) * 1.2;
        c = max(c + (c - blur) * (u.c.y / 100.0) * clamp(mid, 0.0, 1.0), vec3<f32>(0.0));
    }
    if (abs(u.c.x) > 0.5) {
        let o = texel * 2.0;
        let blur = 0.25 * (tap(uv + vec2<f32>(o.x, 0.0)) + tap(uv - vec2<f32>(o.x, 0.0))
            + tap(uv + vec2<f32>(0.0, o.y)) + tap(uv - vec2<f32>(0.0, o.y)));
        c = max(c + (c - blur) * (u.c.x / 50.0), vec3<f32>(0.0));
    }

    let kt = (u.a.x - 5480.0) / 5480.0;
    c *= vec3<f32>(1.0 + 0.8 * kt, 1.0 - u.a.y / 300.0, 1.0 - 0.8 * kt);
    c *= pow(2.0, u.a.z);

    let l = lum(c);
    c *= pow(2.0, (u.b.x / 100.0) * 1.2 * smoothstep(0.3, 1.8, l));
    c *= pow(2.0, (u.b.y / 100.0) * 1.5 * (1.0 - smoothstep(0.0, 0.3, l)));
    c *= 1.0 + (u.b.z / 100.0) * 0.5;
    c = max(c + vec3<f32>(u.b.w / 100.0 * 0.03), vec3<f32>(0.0));

    var x = finish(c);
    x = clamp((x - 0.5) * (1.0 + u.a.w / 100.0) + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));

    let g = lum(x);
    let sat = max(max(x.r, x.g), x.b) - min(min(x.r, x.g), x.b);
    let amount = (1.0 + u.c.w / 100.0) * (1.0 + (u.c.z / 100.0) * (1.0 - sat));
    return clamp(mix(vec3<f32>(g), x, amount), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    if (in.uv.x < u.d.x) {
        return vec4<f32>(finish(tap(in.uv)), 1.0);
    }
    return vec4<f32>(develop(in.uv), 1.0);
}
