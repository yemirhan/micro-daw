//! Automation lane helpers. Port of `src/utils/automationHelpers.ts`.

use std::cmp::Ordering;

use crate::js::{max as js_max, min as js_min, round as js_round};
use crate::types::{AutomationParameter, AutomationPoint};

/// `getAutomationValue`: linear interpolation between points (0.5 for an empty lane).
pub fn get_automation_value(points: &[AutomationPoint], beat: f64) -> f64 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 0.5;
    };
    if points.len() == 1 {
        return first.value;
    }
    if beat <= first.beat {
        return first.value;
    }
    if beat >= last.beat {
        return last.value;
    }
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        if beat >= a.beat && beat <= b.beat {
            let t = (beat - a.beat) / (b.beat - a.beat);
            return a.value + t * (b.value - a.value);
        }
    }
    last.value
}

/// `mapAutomationValue`: normalized 0..1 → the parameter's real range.
pub fn map_automation_value(param: AutomationParameter, normalized: f64) -> f64 {
    use AutomationParameter::*;
    match param {
        Volume => -40.0 + normalized * 40.0,
        Pan => -1.0 + normalized * 2.0,
        FilterCutoff => 60.0 + normalized * (18000.0 - 60.0),
        EqLow | EqMid | EqHigh => -12.0 + normalized * 24.0,
        ReverbWet | DelayWet | ChorusDepth | DistortionWet => normalized,
    }
}

/// `setAutomationPoint`: insert or update the point at `beat` (snapped), keeping beat order.
pub fn set_automation_point(
    points: &[AutomationPoint],
    beat: f64,
    value: f64,
    snap_value: f64,
) -> Vec<AutomationPoint> {
    let snapped = if snap_value > 0.0 {
        js_round(beat / snap_value) * snap_value
    } else {
        beat
    };
    let clamped = js_max(0.0, js_min(1.0, value));
    let mut out = points.to_vec();
    let point = AutomationPoint {
        beat: snapped,
        value: clamped,
    };
    if let Some(i) = out.iter().position(|p| (p.beat - snapped).abs() < 0.001) {
        out[i] = point;
    } else {
        out.push(point);
        // Stable sort, comparator `a.beat - b.beat` (NaN compares equal, as in JS).
        out.sort_by(|a, b| {
            (a.beat - b.beat)
                .partial_cmp(&0.0)
                .unwrap_or(Ordering::Equal)
        });
    }
    out
}

/// `deleteAutomationPoint`.
pub fn delete_automation_point(points: &[AutomationPoint], index: usize) -> Vec<AutomationPoint> {
    points
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != index)
        .map(|(_, p)| *p)
        .collect()
}

/// `getAutomationParameterLabel`.
pub fn get_automation_parameter_label(param: AutomationParameter) -> &'static str {
    use AutomationParameter::*;
    match param {
        Volume => "Volume",
        Pan => "Pan",
        ReverbWet => "Reverb",
        DelayWet => "Delay",
        ChorusDepth => "Chorus",
        DistortionWet => "Distortion",
        FilterCutoff => "Filter",
        EqLow => "EQ Low",
        EqMid => "EQ Mid",
        EqHigh => "EQ High",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(beat: f64, value: f64) -> AutomationPoint {
        AutomationPoint { beat, value }
    }

    #[test]
    fn interpolates() {
        assert_eq!(get_automation_value(&[], 3.0), 0.5);
        assert_eq!(get_automation_value(&[p(1.0, 0.2)], 3.0), 0.2);
        let pts = [p(0.0, 0.0), p(4.0, 1.0), p(8.0, 0.5)];
        assert_eq!(get_automation_value(&pts, -1.0), 0.0);
        assert_eq!(get_automation_value(&pts, 2.0), 0.5);
        assert_eq!(get_automation_value(&pts, 6.0), 0.75);
        assert_eq!(get_automation_value(&pts, 9.0), 0.5);
    }

    #[test]
    fn sets_and_deletes_points() {
        let pts = set_automation_point(&[p(0.0, 0.0), p(4.0, 1.0)], 1.9, 2.0, 1.0);
        assert_eq!(pts, vec![p(0.0, 0.0), p(2.0, 1.0), p(4.0, 1.0)]);
        let pts = set_automation_point(&pts, 2.0004, -1.0, 0.0);
        assert_eq!(pts[1], p(2.0004, 0.0));
        assert_eq!(
            delete_automation_point(&pts, 0),
            vec![p(2.0004, 0.0), p(4.0, 1.0)]
        );
        assert!(
            set_automation_point(&[], 0.0, f64::NAN, 0.0)[0]
                .value
                .is_nan()
        );
        assert_eq!(
            map_automation_value(AutomationParameter::Volume, 0.5),
            -20.0
        );
        assert_eq!(
            get_automation_parameter_label(AutomationParameter::EqMid),
            "EQ Mid"
        );
    }

    /// Golden: rune/parity/fixtures/automation.json (rune/parity/ts/automation.golden.ts).
    #[test]
    fn golden_automation_vectors() {
        use crate::test_support::assert_json_eq;
        use serde_json::{Value, json};
        let fx: Value =
            serde_json::from_str(include_str!("../../../parity/fixtures/automation.json")).unwrap();
        let lanes: Vec<Vec<AutomationPoint>> = serde_json::from_value(fx["lanes"].clone()).unwrap();
        let param =
            |v: &Value| -> AutomationParameter { serde_json::from_value(v.clone()).unwrap() };
        let mut n = 0;
        for r in fx["values"].as_array().unwrap() {
            let got = get_automation_value(
                &lanes[r[0].as_u64().unwrap() as usize],
                r[1].as_f64().unwrap(),
            );
            assert_json_eq(&json!(got), &r[2], &format!("getAutomationValue {r}"));
            n += 1;
        }
        for r in fx["mapped"].as_array().unwrap() {
            assert_json_eq(
                &json!(map_automation_value(param(&r[0]), r[1].as_f64().unwrap())),
                &r[2],
                &r.to_string(),
            );
            n += 1;
        }
        for r in fx["labels"].as_array().unwrap() {
            assert_eq!(get_automation_parameter_label(param(&r[0])), r[1]);
            n += 1;
        }
        let mut current: Option<(u64, Vec<AutomationPoint>)> = None;
        for r in fx["edits"].as_array().unwrap() {
            let lane = r[0].as_u64().unwrap();
            let pts = match &current {
                Some((l, pts)) if *l == lane => pts.clone(),
                _ => lanes[lane as usize].clone(),
            };
            let next = if r[1] == "delete" {
                delete_automation_point(&pts, r[2].as_u64().unwrap() as usize)
            } else {
                set_automation_point(
                    &pts,
                    r[1].as_f64().unwrap(),
                    r[2].as_f64().unwrap(),
                    r[3].as_f64().unwrap(),
                )
            };
            assert_json_eq(
                &serde_json::to_value(&next).unwrap(),
                &r[4],
                &format!("edit {}", &r.to_string()[..40.min(r.to_string().len())]),
            );
            current = Some((lane, next));
            n += 1;
        }
        assert!(n > 1000, "{n}");
    }
}
