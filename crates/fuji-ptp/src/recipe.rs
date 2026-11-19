//! User-facing recipe (film simulation + parameters), stored as JSON.
//! Every field is optional: unset fields keep the RAF's as-shot value.

use crate::profile::{idx, nr_encode, Profile};
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Recipe {
    pub name: String,
    /// e.g. "classic-chrome", "acros-r", or a raw number
    pub film: Option<String>,
    /// exposure compensation in EV (e.g. 0.7)
    pub exposure: Option<f32>,
    /// 100 / 200 / 400
    pub dynamic_range: Option<i32>,
    /// off | weak-small | strong-small | weak-large | strong-large | weak | strong
    pub grain: Option<String>,
    /// off | weak | strong
    pub color_chrome: Option<String>,
    pub color_chrome_blue: Option<String>,
    pub smooth_skin: Option<String>,
    /// as-shot | auto | daylight | shade | incandescent | fluorescent1..3 | underwater | kelvin
    pub white_balance: Option<String>,
    pub wb_kelvin: Option<i32>,
    pub wb_shift_r: Option<i32>,
    pub wb_shift_b: Option<i32>,
    pub highlight: Option<f32>,
    pub shadow: Option<f32>,
    pub color: Option<f32>,
    pub sharpness: Option<f32>,
    pub noise_reduction: Option<i32>,
    pub clarity: Option<i32>,
    /// Raw D185 overrides [(param index, value)], applied last (for research).
    pub raw: Vec<(usize, i32)>,
}

pub const FILM_SIMS: &[(&str, i32)] = &[
    ("provia", 0x01),
    ("velvia", 0x02),
    ("astia", 0x03),
    ("pro-neg-hi", 0x04),
    ("pro-neg-std", 0x05),
    ("monochrome", 0x06),
    ("monochrome-ye", 0x07),
    ("monochrome-r", 0x08),
    ("monochrome-g", 0x09),
    ("sepia", 0x0A),
    ("classic-chrome", 0x0B),
    ("acros", 0x0C),
    ("acros-ye", 0x0D),
    ("acros-r", 0x0E),
    ("acros-g", 0x0F),
    ("eterna", 0x10),
    ("classic-neg", 0x11),
    ("eterna-bleach-bypass", 0x12),
    ("nostalgic-neg", 0x13),
    ("reala-ace", 0x14),
];

fn norm(s: &str) -> String {
    s.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

pub fn film_code(s: &str) -> Result<i32> {
    if let Ok(n) = s.parse::<i32>() {
        return Ok(n);
    }
    if let Some(h) = s.strip_prefix("0x") {
        return Ok(i32::from_str_radix(h, 16)?);
    }
    let n = norm(s);
    FILM_SIMS
        .iter()
        .find(|(k, _)| norm(k) == n)
        .map(|(_, v)| *v)
        .ok_or_else(|| anyhow!("unknown film simulation '{s}'"))
}

pub fn film_name(code: i32) -> Option<&'static str> {
    FILM_SIMS.iter().find(|(_, v)| *v == code).map(|(k, _)| *k)
}

fn effect(s: &str) -> Result<i32> {
    Ok(match norm(s).as_str() {
        "off" => 1,
        "weak" => 2,
        "strong" => 3,
        _ => bail!("effect must be off|weak|strong, got '{s}'"),
    })
}

fn grain(s: &str) -> Result<i32> {
    Ok(match norm(s).as_str() {
        "off" => 1,
        "weak" | "weaksmall" => 2,
        "strong" | "strongsmall" => 3,
        "weaklarge" => 4,
        "stronglarge" => 5,
        _ => bail!("unknown grain '{s}'"),
    })
}

fn white_balance(s: &str) -> Result<i32> {
    Ok(match norm(s).as_str() {
        "asshot" => 0x0000,
        "auto" => 0x0002,
        "autowhite" => 0x8020,
        "autoambience" => 0x8021,
        "daylight" => 0x0004,
        "incandescent" => 0x0006,
        "underwater" => 0x0008,
        "fluorescent1" => 0x8001,
        "fluorescent2" => 0x8002,
        "fluorescent3" => 0x8003,
        "shade" => 0x8006,
        "kelvin" | "colortemp" => 0x8007,
        _ => bail!("unknown white balance '{s}'"),
    })
}

fn x10(v: f32) -> i32 {
    (v * 10.0).round() as i32
}

impl Recipe {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&s)?)
    }

    /// Patch `p` in place; returns human-readable list of changes.
    pub fn apply(&self, p: &mut Profile) -> Result<Vec<String>> {
        let mut changes = Vec::new();
        let mut set = |p: &mut Profile, i: usize, v: i32, label: &str| -> Result<()> {
            let old = p.get(i);
            p.set(i, v)?;
            changes.push(format!("{label}: {old:?} -> {v}"));
            Ok(())
        };
        if let Some(f) = &self.film {
            set(p, idx::FILM_SIMULATION, film_code(f)?, "film")?;
        }
        if let Some(ev) = self.exposure {
            set(p, idx::EXPOSURE_BIAS, (ev * 1000.0).round() as i32, "exposure")?;
        }
        if let Some(dr) = self.dynamic_range {
            // The RAW was exposed for the DR it was shot with: asking for MORE than that
            // (e.g. DR400 on a DR200 shot) makes the camera render garbage (a strong green
            // cast, verified on X-Pro3). X RAW Studio doesn't allow it either, so cap it.
            let dr = match p.get(idx::DYNAMIC_RANGE) {
                Some(shot) if [100, 200, 400].contains(&shot) && dr > shot => shot,
                _ => dr,
            };
            set(p, idx::DYNAMIC_RANGE, dr, "dynamic_range")?;
        }
        if let Some(g) = &self.grain {
            set(p, idx::GRAIN_EFFECT, grain(g)?, "grain")?;
        }
        if let Some(e) = &self.color_chrome {
            set(p, idx::COLOR_CHROME, effect(e)?, "color_chrome")?;
        }
        if let Some(e) = &self.color_chrome_blue {
            set(p, idx::COLOR_CHROME_FX_BLUE, effect(e)?, "color_chrome_blue")?;
        }
        if let Some(e) = &self.smooth_skin {
            set(p, idx::SMOOTH_SKIN, effect(e)?, "smooth_skin")?;
        }
        match (&self.white_balance, self.wb_kelvin) {
            (Some(wb), _) => set(p, idx::WHITE_BALANCE, white_balance(wb)?, "white_balance")?,
            (None, Some(_)) => set(p, idx::WHITE_BALANCE, 0x8007, "white_balance")?,
            _ => {}
        }
        if let Some(k) = self.wb_kelvin {
            set(p, idx::WB_COLOR_TEMP, k, "wb_kelvin")?;
        }
        if let Some(v) = self.wb_shift_r {
            set(p, idx::WB_SHIFT_R, v, "wb_shift_r")?;
        }
        if let Some(v) = self.wb_shift_b {
            set(p, idx::WB_SHIFT_B, v, "wb_shift_b")?;
        }
        if let Some(v) = self.highlight {
            set(p, idx::HIGHLIGHT_TONE, x10(v), "highlight")?;
        }
        if let Some(v) = self.shadow {
            set(p, idx::SHADOW_TONE, x10(v), "shadow")?;
        }
        if let Some(v) = self.color {
            set(p, idx::COLOR, x10(v), "color")?;
        }
        if let Some(v) = self.sharpness {
            set(p, idx::SHARPNESS, x10(v), "sharpness")?;
        }
        if let Some(v) = self.noise_reduction {
            let e = nr_encode(v).ok_or_else(|| anyhow!("noise_reduction must be -4..4"))?;
            set(p, idx::NOISE_REDUCTION, e, "noise_reduction")?;
        }
        if let Some(v) = self.clarity {
            set(p, idx::CLARITY, v * 10, "clarity")?;
        }
        for &(i, v) in &self.raw {
            set(p, i, v, &format!("raw[{i}]"))?;
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn film_lookup() {
        assert_eq!(film_code("Classic Chrome").unwrap(), 0x0B);
        assert_eq!(film_code("acros-r").unwrap(), 0x0E);
        assert_eq!(film_code("17").unwrap(), 17);
        assert!(film_code("kodak").is_err());
    }

    #[test]
    fn recipe_json() {
        let r: Recipe = serde_json::from_str(
            r#"{"name":"t","film":"classic-neg","grain":"weak","highlight":-1,"shadow":1.5}"#,
        )
        .unwrap();
        let mut raw = vec![30u8, 0];
        raw.extend(std::iter::repeat(0u8).take(30 * 4));
        let mut p = Profile::new(raw).unwrap();
        r.apply(&mut p).unwrap();
        assert_eq!(p.get(idx::FILM_SIMULATION), Some(0x11));
        assert_eq!(p.get(idx::GRAIN_EFFECT), Some(2));
        assert_eq!(p.get(idx::HIGHLIGHT_TONE), Some(-10));
        assert_eq!(p.get(idx::SHADOW_TONE), Some(15));
    }
}
