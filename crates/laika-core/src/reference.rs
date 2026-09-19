//! Stable, UI-independent interpretation of persisted develop edits.
//!
//! This is the small compatibility layer used by `laika-render`. Keep it
//! additive: old JSON widths and absent fields must continue to decode.

use crate::edit::{self, CameraProfile, CropGeom, LocalEdits, PARAM_COUNT};

/// The parts of a catalog edit that affect pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct DevelopState {
    pub params: [f32; PARAM_COUNT],
    pub geom: CropGeom,
    pub curve_on: bool,
    pub hsl_on: bool,
    pub detail_on: bool,
    pub optics_on: bool,
    pub effects_on: bool,
    pub grading_on: bool,
    pub locals: LocalEdits,
    pub camera_profile: CameraProfile,
}

impl Default for DevelopState {
    fn default() -> Self {
        Self {
            params: edit::defaults(),
            geom: CropGeom::default(),
            curve_on: true,
            hsl_on: true,
            detail_on: true,
            optics_on: true,
            effects_on: true,
            grading_on: true,
            locals: LocalEdits::default(),
            camera_profile: CameraProfile::default(),
        }
    }
}

/// Public representation of `edits.params_json`.
///
/// Schema-era rows may instead be a bare JSON array; see
/// [`decode_catalog_edit`]. Unknown object fields are ignored by serde.
#[derive(serde::Deserialize)]
struct EditJson {
    #[serde(default)]
    params: Vec<f32>,
    #[serde(default)]
    geom: CropGeom,
    #[serde(default = "edit::flag_on")]
    curve_on: bool,
    #[serde(default = "edit::flag_on")]
    hsl_on: bool,
    #[serde(default = "edit::flag_on")]
    detail_on: bool,
    #[serde(default = "edit::flag_on")]
    optics_on: bool,
    #[serde(default = "edit::flag_on")]
    effects_on: bool,
    #[serde(default = "edit::flag_on")]
    grading_on: bool,
    #[serde(default)]
    locals: LocalEdits,
    #[serde(default)]
    camera_profile: CameraProfile,
}

/// Decode the pixel-affecting state from any released catalog edit shape.
///
/// `history_json` and `cursor` matter because older rows kept live geometry
/// and panel switches on the selected history step. Corrupt history entries
/// are ignored just as they are in the desktop app.
pub fn decode_catalog_edit(
    params_json: Option<&str>,
    history_json: Option<&str>,
    cursor: i64,
) -> Result<DevelopState, String> {
    let Some(json) = params_json else {
        return Ok(DevelopState::default());
    };
    let (params, mut state) = if let Ok(row) = serde_json::from_str::<EditJson>(json) {
        let params = edit::pad_params(&row.params)
            .ok_or_else(|| format!("unsupported edit parameter count {}", row.params.len()))?;
        (
            params,
            DevelopState {
                params,
                geom: row.geom,
                curve_on: row.curve_on,
                hsl_on: row.hsl_on,
                detail_on: row.detail_on,
                optics_on: row.optics_on,
                effects_on: row.effects_on,
                grading_on: row.grading_on,
                locals: row.locals,
                camera_profile: row.camera_profile,
            },
        )
    } else if let Ok(values) = serde_json::from_str::<Vec<f32>>(json) {
        let params = edit::pad_params(&values)
            .ok_or_else(|| format!("unsupported edit parameter count {}", values.len()))?;
        (
            params,
            DevelopState {
                params,
                ..Default::default()
            },
        )
    } else {
        return Err("invalid edits.params_json".to_string());
    };
    state.params = params;

    let history = edit::decode_history(history_json.unwrap_or(""));
    let cursor = (cursor.max(0) as usize).min(history.len());
    if let Some(step) = history.get(cursor.saturating_sub(1)) {
        state.geom = step.geom;
        state.curve_on = step.curve_on;
        state.hsl_on = step.hsl_on;
        state.detail_on = step.detail_on;
        state.optics_on = step.optics_on;
        state.effects_on = step.effects_on;
        state.grading_on = step.grading_on;
        state.locals = step.locals.clone();
        state.camera_profile = step.camera_profile;
    }
    Ok(state)
}

/// Resolve disabled develop panels to their defaults, exactly as preview and
/// export do in the desktop application.
pub fn effective_params(state: &DevelopState) -> [f32; PARAM_COUNT] {
    let mut values = state.params;
    let defaults = edit::defaults();
    if !state.curve_on {
        values[edit::CURVE_RANGE].copy_from_slice(&defaults[edit::CURVE_RANGE]);
    }
    if !state.hsl_on {
        values[edit::HSL_RANGE].copy_from_slice(&defaults[edit::HSL_RANGE]);
    }
    if !state.detail_on {
        values[edit::DETAIL_RANGE].copy_from_slice(&defaults[edit::DETAIL_RANGE]);
    }
    if !state.optics_on {
        values[edit::OPTICS_RANGE].copy_from_slice(&defaults[edit::OPTICS_RANGE]);
    }
    if !state.effects_on {
        for i in edit::EFFECTS_PARAMS {
            values[i] = defaults[i];
        }
    }
    if !state.grading_on {
        values[edit::GRADING_RANGE].copy_from_slice(&defaults[edit::GRADING_RANGE]);
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_arrays_pad_and_future_fields_are_ignored() {
        let old = serde_json::to_string(&edit::defaults()[..12]).unwrap();
        let decoded = decode_catalog_edit(Some(&old), None, 0).unwrap();
        assert_eq!(decoded.params, edit::defaults());

        let current = format!(
            r#"{{"params":{},"curve_on":false,"future_field":{{"x":1}}}}"#,
            serde_json::to_string(edit::defaults().as_slice()).unwrap()
        );
        let decoded = decode_catalog_edit(Some(&current), None, 0).unwrap();
        assert!(!decoded.curve_on);
    }

    #[test]
    fn bypassed_panels_render_at_defaults() {
        let mut state = DevelopState::default();
        state.params[12] = 0.9;
        state.params[16] = 25.;
        state.params[40] = 80.;
        state.params[44] = 30.;
        state.params[8] = 40.;
        state.params[49] = 180.;
        state.curve_on = false;
        state.hsl_on = false;
        state.detail_on = false;
        state.optics_on = false;
        state.effects_on = false;
        state.grading_on = false;
        assert_eq!(effective_params(&state), edit::defaults());
    }
}
