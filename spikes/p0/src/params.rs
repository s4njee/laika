//! The twelve Basic parameters. `default` is the value that reads as untouched (white); the mock's
//! green values start modified.

#[derive(Clone, Copy)]
pub enum Fmt {
    Kelvin,
    Int,
    Ev,
}

#[derive(Clone, Copy)]
pub struct ParamDef {
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f32,
    pub fmt: Fmt,
}

const fn p(label: &'static str, min: f32, max: f32, default: f32, step: f32, fmt: Fmt) -> ParamDef {
    ParamDef { label, min, max, default, step, fmt }
}

pub const PARAMS: [ParamDef; 12] = [
    p("Temperature", 2000., 8960., 5480., 20., Fmt::Kelvin),
    p("Tint", -150., 150., 6., 1., Fmt::Int),
    p("Exposure", -5., 5., 0., 0.05, Fmt::Ev),
    p("Contrast", -100., 100., 12., 1., Fmt::Int),
    p("Highlights", -100., 100., 0., 1., Fmt::Int),
    p("Shadows", -100., 100., 0., 1., Fmt::Int),
    p("Whites", -100., 100., 8., 1., Fmt::Int),
    p("Blacks", -100., 100., -14., 1., Fmt::Int),
    p("Texture", -100., 100., 10., 1., Fmt::Int),
    p("Clarity", -100., 100., 4., 1., Fmt::Int),
    p("Vibrance", -100., 100., 18., 1., Fmt::Int),
    p("Saturation", -100., 100., 0., 1., Fmt::Int),
];

/// The mock's starting state: Exposure, Highlights and Shadows modified.
pub fn initial() -> [f32; 12] {
    let mut v = PARAMS.map(|d| d.default);
    v[2] = 0.35;
    v[4] = -40.;
    v[5] = 28.;
    v
}

pub fn is_modified(i: usize, v: f32) -> bool {
    (v - PARAMS[i].default).abs() > PARAMS[i].step * 0.25
}

pub fn snap(i: usize, v: f32) -> f32 {
    let d = PARAMS[i];
    ((v / d.step).round() * d.step).clamp(d.min, d.max)
}

pub fn format(i: usize, v: f32) -> String {
    let sign = |x: f32| if x > 0. { "+" } else if x < 0. { "\u{2212}" } else { "" };
    match PARAMS[i].fmt {
        Fmt::Kelvin => format!("{} K", v.round() as i32),
        Fmt::Int => format!("{}{}", sign(v.round()), v.round().abs() as i32),
        Fmt::Ev => {
            let r = (v * 100.).round() / 100.;
            format!("{}{:.2}", sign(r), r.abs())
        }
    }
}
