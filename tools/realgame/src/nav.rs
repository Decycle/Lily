//! Scripted navigation (doc 07 §2): operator-button presses and ring touches through the shared buffer, each
//! followed by a wait, and screen checks on captured frames, so a run only continues once the expected screen is
//! shown.
//!
//! File format (`*.nav.json`):
//! `{"steps":[{"do":"press","buttons":["test"],"hold_ms":150,"wait_ms":2000},
//!            {"do":"touch","lane":15,"width":3,"depths":15,"hold_ms":100,"wait_ms":1000},
//!            {"do":"expect","name":"test menu","timeout_ms":5000,"all":[{"region":[x,y,w,h],"color":[r,g,b],
//!             "tol":60,"min_frac":0.05,"max_frac":1.0}]}]}`
//! Regions are fractions of the captured frame (x, y, width, height), so a script does not depend on the window
//! size as long as the aspect ratio is the game's. A colour rule passes when the fraction of region pixels within
//! `tol` (per channel) of `color` lies in `[min_frac, max_frac]`; an expect step without rules just saves a frame.
//! Touch lanes/depths follow [`crate::timeline::Tap`] (lane 0 just above 3 o'clock, counter-clockwise; depth bit
//! 0 = innermost ring).

use crate::cells::CELLS;
use crate::frame::Rgb;
use crate::inject::{self, Kind, Publish, RENEW_MS};
use crate::timeline::{self, Tap};
use crate::wcon::{OPBTN_COIN, OPBTN_SERVICE, OPBTN_TEST};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Test,
    Service,
    Coin,
}

impl Button {
    pub fn bit(self) -> u8 {
        match self {
            Self::Test => OPBTN_TEST,
            Self::Service => OPBTN_SERVICE,
            Self::Coin => OPBTN_COIN,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct ColorRule {
    pub region: [f64; 4],
    pub color: [u8; 3],
    #[serde(default = "default_tol")]
    pub tol: u8,
    #[serde(default)]
    pub min_frac: f64,
    #[serde(default = "one")]
    pub max_frac: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "do", rename_all = "lowercase")]
pub enum Step {
    /// Hold `buttons` together for `hold_ms`, then wait `wait_ms`.
    Press {
        buttons: Vec<Button>,
        #[serde(default = "default_hold")]
        hold_ms: f64,
        #[serde(default)]
        wait_ms: f64,
    },
    /// Touch `width` lanes from `lane` on the rings in `depths` for `hold_ms`, then wait `wait_ms`.
    Touch {
        lane: u8,
        #[serde(default = "one_lane")]
        width: u8,
        #[serde(default = "all_depths")]
        depths: u8,
        #[serde(default = "default_hold")]
        hold_ms: f64,
        #[serde(default)]
        wait_ms: f64,
    },
    /// Grab frames until every rule passes, failing after `timeout_ms`.
    Expect {
        name: String,
        all: Vec<ColorRule>,
        #[serde(default = "default_timeout")]
        timeout_ms: f64,
    },
    /// Until `region` of the frame looks like the PNG `image` ([`region_diff`] <= `max_diff`), run the press or
    /// touch step `then`; fails after `max_steps` of them. E.g. scroll the song list until a jacket is centred, or
    /// wait for a screen (`then` = a press of no buttons with a wait). With `back`, a search that `then` did not
    /// finish continues with up to `2 * max_steps` of `back` (the other scroll direction).
    Seek {
        name: String,
        image: String,
        region: [f64; 4],
        #[serde(default = "default_diff")]
        max_diff: f64,
        then: Box<Step>,
        max_steps: usize,
        #[serde(default)]
        back: Option<Box<Step>>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NavScript {
    pub steps: Vec<Step>,
}

fn default_tol() -> u8 {
    48
}
fn one() -> f64 {
    1.0
}
fn one_lane() -> u8 {
    1
}
fn all_depths() -> u8 {
    0b1111
}
fn default_hold() -> f64 {
    150.0
}
fn default_timeout() -> f64 {
    5000.0
}
fn default_diff() -> f64 {
    30.0
}

/// Plan for touching `width` lanes from `lane` on `depths` for `hold_ms`.
pub fn touch_plan(lane: u8, width: u8, depths: u8, hold_ms: f64) -> Vec<Publish> {
    let tap = Tap {
        at_ms: 0.0,
        dur_ms: hold_ms,
        lane,
        width,
        depths,
    };
    inject::schedule(&timeline::expand(&[tap]))
}

/// Mean absolute difference (0..255) between `region` of `f` (fractions of the frame) and `reference`, both
/// reduced to a `grid` x `grid` raster of cell means, so small misalignment and scaling do not matter.
pub fn region_diff(f: &Rgb, region: [f64; 4], reference: &Rgb, grid: usize) -> f64 {
    let [x, y, w, h] = region;
    let cells = |img: &Rgb, x0: f64, y0: f64, cw: f64, ch: f64| -> Vec<f64> {
        let mut out = Vec::with_capacity(grid * grid * 3);
        for gy in 0..grid {
            for gx in 0..grid {
                let px = |v: f64, max: usize| (v.max(0.0) as usize).min(max);
                let (xa, xb) = (
                    px(x0 + cw * gx as f64, img.width),
                    px(x0 + cw * (gx + 1) as f64, img.width),
                );
                let (ya, yb) = (
                    px(y0 + ch * gy as f64, img.height),
                    px(y0 + ch * (gy + 1) as f64, img.height),
                );
                let n = ((xb - xa) * (yb - ya)).max(1) as f64;
                let mut sum = [0.0; 3];
                for yy in ya..yb {
                    for xx in xa..xb {
                        let p = img.pixel(xx, yy);
                        (0..3).for_each(|c| sum[c] += f64::from(p[c]));
                    }
                }
                out.extend(sum.map(|s| s / n));
            }
        }
        out
    };
    let (fw, fh) = (f.width as f64, f.height as f64);
    let a = cells(
        f,
        x * fw,
        y * fh,
        w * fw / grid as f64,
        h * fh / grid as f64,
    );
    let (rw, rh) = (reference.width as f64, reference.height as f64);
    let b = cells(reference, 0.0, 0.0, rw / grid as f64, rh / grid as f64);
    a.iter().zip(&b).map(|(p, q)| (p - q).abs()).sum::<f64>() / a.len().max(1) as f64
}

pub fn parse(json: &str) -> serde_json::Result<NavScript> {
    serde_json::from_str(json)
}

impl Step {
    /// Input of a press or touch step and the wait after it; `None` for screen checks.
    pub fn input(&self) -> Option<(Vec<Publish>, f64)> {
        match *self {
            Self::Press {
                ref buttons,
                hold_ms,
                wait_ms,
            } => Some((press_plan(buttons, hold_ms), wait_ms)),
            Self::Touch {
                lane,
                width,
                depths,
                hold_ms,
                wait_ms,
            } => Some((touch_plan(lane, width, depths, hold_ms), wait_ms)),
            Self::Expect { .. } | Self::Seek { .. } => None,
        }
    }
}

/// Publishes for holding `buttons` from 0 to `hold_ms` (lease renewals in between), no cells touched.
pub fn press_plan(buttons: &[Button], hold_ms: f64) -> Vec<Publish> {
    let bits = buttons.iter().fold(0, |b, x| b | x.bit());
    let publish = |t_ms, kind, opbtn| Publish {
        t_ms,
        kind,
        opbtn,
        cells: [0; CELLS],
    };
    let mut out = vec![publish(0.0, Kind::Edge, bits)];
    let mut t = RENEW_MS;
    while t < hold_ms {
        out.push(publish(t, Kind::Renew, bits));
        t += RENEW_MS;
    }
    out.push(publish(hold_ms, Kind::Edge, 0));
    out
}

impl ColorRule {
    /// Fraction of matching pixels in the region (0 for an empty region).
    pub fn fraction(&self, f: &Rgb) -> f64 {
        let [x, y, w, h] = self.region;
        let px = |v: f64, size: usize| ((v * size as f64).round().max(0.0) as usize).min(size);
        let (x0, x1) = (px(x, f.width), px(x + w, f.width));
        let (y0, y1) = (px(y, f.height), px(y + h, f.height));
        let total = (x1 - x0) * (y1 - y0);
        if total == 0 {
            return 0.0;
        }
        let near = |p: [u8; 3]| {
            p.iter()
                .zip(self.color)
                .all(|(&a, b)| a.abs_diff(b) <= self.tol)
        };
        let hits = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| near(f.pixel(x, y)))
            .count();
        hits as f64 / total as f64
    }

    pub fn passes(&self, f: &Rgb) -> bool {
        (self.min_frac..=self.max_frac).contains(&self.fraction(f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steps_with_defaults() {
        let s = parse(
            r#"{"steps":[
                {"do":"press","buttons":["test","service"]},
                {"do":"expect","name":"menu","all":[{"region":[0,0,1,1],"color":[255,0,0],"min_frac":0.1}]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            s.steps[0],
            Step::Press {
                buttons: vec![Button::Test, Button::Service],
                hold_ms: 150.0,
                wait_ms: 0.0
            }
        );
        let Step::Expect {
            name,
            all,
            timeout_ms,
        } = &s.steps[1]
        else {
            panic!("not an expect step");
        };
        assert_eq!((name.as_str(), *timeout_ms), ("menu", 5000.0));
        assert_eq!((all[0].tol, all[0].max_frac), (48, 1.0));
        assert!(parse(r#"{"steps":[{"do":"jump"}]}"#).is_err());
        assert!(parse(r#"{"steps":[{"do":"press","buttons":["start"]}]}"#).is_err());
    }

    #[test]
    fn shipped_nav_script_parses() {
        let s = parse(include_str!("../scripts/nav-input-test.nav.json")).unwrap();
        assert!(matches!(s.steps.last(), Some(Step::Expect { .. })));
        let s = parse(include_str!("../scripts/e2-guest-fragile.nav.json")).unwrap();
        assert!(matches!(s.steps.last(), Some(Step::Seek { .. })));
        assert!(parse(include_str!("../scripts/e2-start.nav.json")).is_ok());
        assert!(parse(include_str!("../scripts/e2-guest-king.nav.json")).is_ok());
    }

    #[test]
    fn press_holds_with_renewals_then_releases() {
        let plan = press_plan(&[Button::Test, Button::Service], 120.0);
        let got: Vec<(f64, Kind, u8)> = plan.iter().map(|p| (p.t_ms, p.kind, p.opbtn)).collect();
        assert_eq!(
            got,
            [
                (0.0, Kind::Edge, 0x03),
                (50.0, Kind::Renew, 0x03),
                (100.0, Kind::Renew, 0x03),
                (120.0, Kind::Edge, 0),
            ]
        );
        assert!(plan.iter().all(|p| p.cells.iter().all(|&c| c == 0)));
        assert_eq!(Button::Coin.bit(), 0x04);
    }

    #[test]
    fn touch_step_plans_cells_with_defaults() {
        let s =
            parse(r#"{"steps":[{"do":"touch","lane":30,"hold_ms":120,"wait_ms":500}]}"#).unwrap();
        let (plan, wait) = s.steps[0].input().unwrap();
        assert_eq!(wait, 500.0);
        let kinds: Vec<(f64, Kind)> = plan.iter().map(|p| (p.t_ms, p.kind)).collect();
        assert_eq!(
            kinds,
            [
                (0.0, Kind::Edge),
                (50.0, Kind::Renew),
                (100.0, Kind::Renew),
                (120.0, Kind::Edge)
            ]
        );
        let on = crate::cells::from_cells(&plan[0].cells);
        assert!((0..4).all(|d| on.get(30, d)) && !on.lane_touched(31));
        assert!(plan[3].cells.iter().all(|&c| c == 0));
        assert!(plan.iter().all(|p| p.opbtn == 0));
        let press = parse(r#"{"steps":[{"do":"press","buttons":["coin"]}]}"#).unwrap();
        assert_eq!(press.steps[0].input().unwrap().0[0].opbtn, 0x04);
        let expect = parse(r#"{"steps":[{"do":"expect","name":"x","all":[]}]}"#).unwrap();
        assert!(expect.steps[0].input().is_none());
        let seek = parse(
            r#"{"steps":[{"do":"seek","name":"j","image":"a.png","region":[0,0,1,1],"max_steps":9,
                "then":{"do":"touch","lane":49,"wait_ms":400}}]}"#,
        )
        .unwrap();
        let Step::Seek {
            max_diff,
            then,
            max_steps: 9,
            back: None,
            ..
        } = &seek.steps[0]
        else {
            panic!("not a seek step");
        };
        assert_eq!(*max_diff, 30.0);
        assert_eq!(then.input().unwrap().1, 400.0);
        assert!(seek.steps[0].input().is_none());
    }

    #[test]
    fn region_difference_ignores_scale() {
        // 4x4 reference: left half red, right half blue; frame 8x8 with the same picture in its top-left 4x4.
        let px = |x: usize| if x < 2 { [200, 0, 0] } else { [0, 0, 200] };
        let reference = Rgb::new(4, 4, (0..16).flat_map(|i| px(i % 4)).collect()).unwrap();
        let big = Rgb::new(
            8,
            8,
            (0..64)
                .flat_map(|i| {
                    if i % 8 < 4 && i / 8 < 4 {
                        px(i % 8)
                    } else {
                        [0; 3]
                    }
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(region_diff(&big, [0.0, 0.0, 0.5, 0.5], &reference, 2), 0.0);
        let d = region_diff(&big, [0.5, 0.5, 0.5, 0.5], &reference, 2);
        assert!((d - 200.0 / 3.0).abs() < 1e-9, "{d}");
    }

    #[test]
    fn colour_fraction_in_region() {
        // 4x2 frame: left half red, right half black.
        let mut data = Vec::new();
        for _ in 0..2 {
            data.extend([250, 10, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        let f = Rgb::new(4, 2, data).unwrap();
        let rule = |region, min_frac, max_frac| ColorRule {
            region,
            color: [255, 0, 0],
            tol: 20,
            min_frac,
            max_frac,
        };
        assert_eq!(rule([0.0, 0.0, 1.0, 1.0], 0.0, 1.0).fraction(&f), 0.5);
        assert_eq!(rule([0.0, 0.0, 0.5, 0.5], 0.0, 1.0).fraction(&f), 1.0);
        assert_eq!(rule([0.5, 0.0, 0.5, 1.0], 0.0, 1.0).fraction(&f), 0.0);
        assert_eq!(rule([0.9, 0.0, 0.0, 1.0], 0.0, 1.0).fraction(&f), 0.0);
        assert!(rule([0.0, 0.0, 1.0, 1.0], 0.4, 0.6).passes(&f));
        assert!(!rule([0.0, 0.0, 1.0, 1.0], 0.6, 1.0).passes(&f));
        assert!(rule([0.5, 0.0, 0.5, 1.0], 0.0, 0.01).passes(&f));
    }
}
