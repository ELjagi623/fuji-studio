//! In-camera custom settings (C1–C7).
//!
//! Protocol (FilmKit; works on X-Pro3 fw 2.00 even though the properties are not
//! advertised in DeviceInfo):
//!   write 0xD18C = slot (1..7)  -> selects which slot the following props refer to
//!   read  0xD18D               -> slot name (PTP string)
//!   read  0xD18E..=0xD1A5      -> slot parameters (u16/i16, encodings below)
//! The original 0xD18C value is restored afterwards.

use crate::camera::FujiCamera;
use crate::data::{Reader, Writer};
use crate::recipe::{film_name, Recipe};
use crate::transport::Transport;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const SLOT: u16 = 0xD18C;
pub const NAME: u16 = 0xD18D;
pub const FIRST: u16 = 0xD18E;
pub const LAST: u16 = 0xD1A5;

pub const DYNAMIC_RANGE: u16 = 0xD190; // 100/200/400
pub const FILM: u16 = 0xD192;
pub const MONO_WC: u16 = 0xD193; // ×10, B&W only
pub const MONO_MG: u16 = 0xD194; // ×10, B&W only
pub const GRAIN: u16 = 0xD195; // 1 off, 2 weak, 3 strong, 4 weak-large, 5 strong-large
pub const COLOR_CHROME: u16 = 0xD196; // 1 off, 2 weak, 3 strong
pub const CC_FX_BLUE: u16 = 0xD197;
pub const SMOOTH_SKIN: u16 = 0xD198;
pub const WHITE_BALANCE: u16 = 0xD199; // u16 mode
pub const WB_SHIFT_R: u16 = 0xD19A;
pub const WB_SHIFT_B: u16 = 0xD19B;
pub const WB_KELVIN: u16 = 0xD19C;
pub const HIGHLIGHT: u16 = 0xD19D; // ×10
pub const SHADOW: u16 = 0xD19E; // ×10
pub const COLOR: u16 = 0xD19F; // ×10
pub const SHARPNESS: u16 = 0xD1A0; // ×10
pub const HIGH_ISO_NR: u16 = 0xD1A1; // proprietary
pub const CLARITY: u16 = 0xD1A2; // ×10

pub fn prop_label(id: u16) -> &'static str {
    match id {
        0xD18E => "ImageSize",
        0xD18F => "ImageQuality",
        DYNAMIC_RANGE => "DynamicRange%",
        FILM => "FilmSimulation",
        MONO_WC => "MonoWarmCool×10",
        MONO_MG => "MonoMagentaGreen×10",
        GRAIN => "Grain",
        COLOR_CHROME => "ColorChrome",
        CC_FX_BLUE => "ColorChromeFxBlue",
        SMOOTH_SKIN => "SmoothSkin",
        WHITE_BALANCE => "WhiteBalance",
        WB_SHIFT_R => "WBShiftR",
        WB_SHIFT_B => "WBShiftB",
        WB_KELVIN => "WBKelvin",
        HIGHLIGHT => "Highlight×10",
        SHADOW => "Shadow×10",
        COLOR => "Color×10",
        SHARPNESS => "Sharpness×10",
        HIGH_ISO_NR => "HighIsoNR(enc)",
        CLARITY => "Clarity×10",
        0xD1A3 => "LongExpNR",
        0xD1A4 => "ColorSpace",
        _ => "?",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraPreset {
    pub slot: u8,
    pub name: String,
    /// raw property bytes, (prop id, value) — kept verbatim so unknown props round-trip
    pub values: Vec<(u16, Vec<u8>)>,
}

/// Max custom-setting name length we allow (camera UI limit is not documented; keep it short).
pub const MAX_NAME_LEN: usize = 25;

pub fn validate_name(name: &str) -> Result<()> {
    if name.chars().count() > MAX_NAME_LEN {
        bail!("名称最多 {MAX_NAME_LEN} 个字符");
    }
    if !name.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
        bail!("名称只能用英文字母、数字和常见符号（相机不显示中文）");
    }
    Ok(())
}

impl CameraPreset {
    fn raw(&self, id: u16) -> Option<&[u8]> {
        self.values.iter().find(|(i, _)| *i == id).map(|(_, v)| v.as_slice())
    }
    fn u16(&self, id: u16) -> Option<u16> {
        self.raw(id).filter(|b| b.len() >= 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn i16(&self, id: u16) -> Option<i16> {
        self.u16(id).map(|v| v as i16)
    }
}

impl<T: Transport> FujiCamera<T> {
    fn select_slot(&mut self, slot: u8) -> Result<()> {
        if !(1..=7).contains(&slot) {
            bail!("槽位必须是 C1–C7");
        }
        let mut w = Writer::default();
        w.u16(slot as u16);
        self.set_prop(SLOT, &w.0).context("选择槽位 (0xD18C)")?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        Ok(())
    }

    fn read_selected(&mut self, slot: u8) -> CameraPreset {
        let name = self
            .get_prop(NAME)
            .ok()
            .and_then(|b| Reader::new(&b).string().ok())
            .unwrap_or_default();
        let mut values = Vec::new();
        for id in FIRST..=LAST {
            if let Ok(v) = self.get_prop(id) {
                values.push((id, v));
            }
        }
        CameraPreset { slot, name, values }
    }

    /// Read C1–C7. Restores the camera's slot selector afterwards.
    pub fn read_presets(&mut self) -> Result<Vec<CameraPreset>> {
        let orig = self.get_prop(SLOT)?;
        let mut out = Vec::new();
        let result = (|| -> Result<()> {
            for slot in 1u8..=7 {
                self.select_slot(slot)?;
                out.push(self.read_selected(slot));
            }
            Ok(())
        })();
        let _ = self.set_prop(SLOT, &orig);
        result?;
        Ok(out)
    }

    /// Write a name and/or raw props into slot `slot`, then read it back.
    /// Returns the slot as read back plus a list of props the camera rejected
    /// or did not store. The slot selector is restored afterwards.
    pub fn write_preset(
        &mut self,
        slot: u8,
        name: Option<&str>,
        props: &[(u16, Vec<u8>)],
    ) -> Result<(CameraPreset, Vec<String>)> {
        if let Some(n) = name {
            validate_name(n)?;
        }
        let orig = self.get_prop(SLOT)?;
        let result = (|| -> Result<(CameraPreset, Vec<String>)> {
            self.select_slot(slot)?;
            let mut problems = Vec::new();
            if let Some(n) = name {
                let mut w = Writer::default();
                w.string(n);
                self.set_prop(NAME, &w.0).context("写入名称 (0xD18D)")?;
            }
            for (id, v) in props {
                if let Err(e) = self.set_prop(*id, v) {
                    problems.push(format!("{}：相机拒绝写入（{e:#}）", prop_label(*id)));
                }
            }
            // verify
            let back = self.read_selected(slot);
            if let Some(n) = name {
                if back.name != n {
                    problems.push(format!("名称：写入「{n}」，读回「{}」", back.name));
                }
            }
            for (id, v) in props {
                if let Some(got) = back.raw(*id) {
                    if got != v.as_slice() && !problems.iter().any(|p| p.starts_with(prop_label(*id))) {
                        problems.push(format!("{}：写入后读回的值不一致", prop_label(*id)));
                    }
                }
            }
            Ok((back, problems))
        })();
        let _ = self.set_prop(SLOT, &orig);
        result
    }
}

fn effect_code(s: &str) -> Option<u16> {
    Some(match s {
        "off" => 1,
        "weak" => 2,
        "strong" => 3,
        _ => return None,
    })
}

fn grain_code(s: &str) -> Option<u16> {
    Some(match s {
        "off" => 1,
        "weak" => 2,
        "strong" => 3,
        "weak-large" => 4,
        "strong-large" => 5,
        _ => return None,
    })
}

fn wb_code(s: &str) -> Option<u16> {
    Some(match s {
        "auto" => 0x0002,
        "auto-white" => 0x8020,
        "auto-ambience" => 0x8021,
        "daylight" => 0x0004,
        "shade" => 0x8006,
        "incandescent" => 0x0006,
        "fluorescent1" => 0x8001,
        "fluorescent2" => 0x8002,
        "fluorescent3" => 0x8003,
        "underwater" => 0x0008,
        "kelvin" => 0x8007,
        _ => return None,
    })
}

fn nr_code(v: i32) -> Option<u16> {
    Some(match v {
        -4 => 0x8000,
        -3 => 0x7000,
        -2 => 0x4000,
        -1 => 0x3000,
        0 => 0x2000,
        1 => 0x1000,
        2 => 0x0000,
        3 => 0x6000,
        4 => 0x5000,
        _ => return None,
    })
}

/// Turn a recipe into slot property writes. Fields that are unset (None) keep the
/// camera's current value; only props whose encoded value differs from `base` are returned.
pub fn encode_changes(r: &Recipe, base: &CameraPreset) -> Result<Vec<(u16, Vec<u8>)>> {
    let mut want: Vec<(u16, u16)> = Vec::new();
    if let Some(f) = &r.film {
        want.push((FILM, crate::recipe::film_code(f)? as u16));
    }
    if let Some(dr) = r.dynamic_range {
        if ![100, 200, 400].contains(&dr) {
            bail!("动态范围只能是 100/200/400");
        }
        want.push((DYNAMIC_RANGE, dr as u16));
    }
    let enc = |label: &str, v: Option<u16>| v.ok_or_else(|| anyhow!("{label} 的取值无效"));
    if let Some(g) = &r.grain {
        // "off" is stored as 1 or 6 (off + large size); don't rewrite one as the other
        let already_off = g == "off" && matches!(base.u16(GRAIN), Some(1) | Some(6));
        if !already_off {
            want.push((GRAIN, enc("颗粒效果", grain_code(g))?));
        }
    }
    if let Some(e) = &r.color_chrome {
        want.push((COLOR_CHROME, enc("色彩效果", effect_code(e))?));
    }
    if let Some(e) = &r.color_chrome_blue {
        want.push((CC_FX_BLUE, enc("色彩效果·蓝", effect_code(e))?));
    }
    if let Some(e) = &r.smooth_skin {
        want.push((SMOOTH_SKIN, enc("美肤效果", effect_code(e))?));
    }
    if let Some(wb) = &r.white_balance {
        want.push((WHITE_BALANCE, enc("白平衡", wb_code(wb))?));
        if wb == "kelvin" {
            if let Some(k) = r.wb_kelvin {
                want.push((WB_KELVIN, k.clamp(2500, 10000) as u16));
            }
        }
    }
    if let Some(v) = r.wb_shift_r {
        want.push((WB_SHIFT_R, v.clamp(-9, 9) as i16 as u16));
    }
    if let Some(v) = r.wb_shift_b {
        want.push((WB_SHIFT_B, v.clamp(-9, 9) as i16 as u16));
    }
    let x10 = |v: f32| (v * 10.0).round() as i16 as u16;
    if let Some(v) = r.highlight {
        want.push((HIGHLIGHT, x10(v)));
    }
    if let Some(v) = r.shadow {
        want.push((SHADOW, x10(v)));
    }
    if let Some(v) = r.color {
        want.push((COLOR, x10(v)));
    }
    if let Some(v) = r.sharpness {
        want.push((SHARPNESS, x10(v)));
    }
    if let Some(v) = r.noise_reduction {
        want.push((HIGH_ISO_NR, enc("高 ISO 降噪", nr_code(v))?));
    }
    if let Some(v) = r.clarity {
        want.push((CLARITY, x10(v as f32)));
    }
    Ok(want
        .into_iter()
        .map(|(id, v)| (id, v.to_le_bytes().to_vec()))
        .filter(|(id, bytes)| base.raw(*id) != Some(bytes.as_slice()))
        .collect())
}

fn effect(v: u16) -> Option<String> {
    Some(match v {
        1 => "off",
        2 => "weak",
        3 => "strong",
        _ => return None,
    }
    .into())
}

fn grain(v: u16) -> Option<String> {
    Some(match v {
        // X-Pro3 stores "off" as 1 or 6 (6 = off with size "large", seen on a real slot)
        1 | 6 => "off",
        2 => "weak",
        3 => "strong",
        4 => "weak-large",
        5 => "strong-large",
        _ => return None,
    }
    .into())
}

fn white_balance(v: u16) -> Option<String> {
    Some(match v {
        0x0002 => "auto",
        0x8020 => "auto-white",
        0x8021 => "auto-ambience",
        0x0004 => "daylight",
        0x8006 => "shade",
        0x0006 => "incandescent",
        0x8001 => "fluorescent1",
        0x8002 => "fluorescent2",
        0x8003 => "fluorescent3",
        0x0008 => "underwater",
        0x8007 => "kelvin",
        _ => return None,
    }
    .into())
}

/// ×10 tone value, ignoring the 0x8000 "not set" sentinel.
fn tone(v: Option<i16>) -> Option<f32> {
    v.filter(|x| *x != i16::MIN).map(|x| x as f32 / 10.0)
}

fn nr_decode(v: u16) -> Option<i32> {
    Some(match v {
        0x8000 => -4,
        0x7000 => -3,
        0x4000 => -2,
        0x3000 => -1,
        0x2000 => 0,
        0x1000 => 1,
        0x0000 => 2,
        0x6000 => 3,
        0x5000 => 4,
        _ => return None,
    })
}

/// Translate a camera slot into a Recipe (same encodings as the D185 profile).
pub fn to_recipe(p: &CameraPreset) -> Result<Recipe> {
    let mut r = Recipe {
        name: if p.name.trim().is_empty() { format!("C{}", p.slot) } else { format!("C{} {}", p.slot, p.name.trim()) },
        ..Default::default()
    };
    r.film = p.u16(FILM).and_then(|v| film_name(v as i32)).map(String::from);
    r.dynamic_range = p.u16(DYNAMIC_RANGE).filter(|v| [100, 200, 400].contains(v)).map(|v| v as i32);
    r.grain = p.u16(GRAIN).and_then(grain);
    r.color_chrome = p.u16(COLOR_CHROME).and_then(effect);
    r.color_chrome_blue = p.u16(CC_FX_BLUE).and_then(effect);
    r.smooth_skin = p.u16(SMOOTH_SKIN).and_then(effect);
    r.white_balance = p.u16(WHITE_BALANCE).and_then(white_balance);
    if r.white_balance.as_deref() == Some("kelvin") {
        r.wb_kelvin = p.u16(WB_KELVIN).filter(|k| *k >= 2500).map(|k| k as i32);
    }
    r.wb_shift_r = p.i16(WB_SHIFT_R).map(|v| v as i32);
    r.wb_shift_b = p.i16(WB_SHIFT_B).map(|v| v as i32);
    r.highlight = tone(p.i16(HIGHLIGHT));
    r.shadow = tone(p.i16(SHADOW));
    r.color = tone(p.i16(COLOR));
    r.sharpness = tone(p.i16(SHARPNESS));
    r.noise_reduction = p.u16(HIGH_ISO_NR).and_then(nr_decode);
    r.clarity = tone(p.i16(CLARITY)).map(|v| v.round() as i32);
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(v: i16) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    /// Raw values read from slot C7 "SUPERIA" on a real X-Pro3 (fw 2.00).
    #[test]
    fn decodes_real_xpro3_slot() {
        let p = CameraPreset {
            slot: 7,
            name: "SUPERIA".into(),
            values: vec![
                (DYNAMIC_RANGE, le(400)),
                (FILM, le(0x11)),
                (GRAIN, le(3)),
                (COLOR_CHROME, le(3)),
                (CC_FX_BLUE, le(3)),
                (SMOOTH_SKIN, le(1)),
                (WHITE_BALANCE, le(2)),
                (WB_SHIFT_R, le(3)),
                (WB_SHIFT_B, le(-5)),
                (HIGHLIGHT, le(0)),
                (SHADOW, le(-10)),
                (COLOR, le(40)),
                (SHARPNESS, le(-10)),
                (HIGH_ISO_NR, vec![0x00, 0x80]),
                (CLARITY, le(-20)),
            ],
        };
        let r = to_recipe(&p).unwrap();
        assert_eq!(r.name, "C7 SUPERIA");
        assert_eq!(r.film.as_deref(), Some("classic-neg"));
        assert_eq!(r.dynamic_range, Some(400));
        assert_eq!(r.grain.as_deref(), Some("strong"));
        assert_eq!(r.color_chrome_blue.as_deref(), Some("strong"));
        assert_eq!(r.white_balance.as_deref(), Some("auto"));
        assert_eq!((r.wb_shift_r, r.wb_shift_b), (Some(3), Some(-5)));
        assert_eq!((r.highlight, r.shadow, r.color), (Some(0.0), Some(-1.0), Some(4.0)));
        assert_eq!(r.noise_reduction, Some(-4));
        assert_eq!(r.clarity, Some(-2));
    }

    /// Reading a slot and encoding it back unchanged must produce zero writes.
    #[test]
    fn roundtrip_is_a_noop() {
        for (dr, grain) in [(le(400), le(3)), (le(-1), le(6)), (le(200), le(4))] {
            let p = CameraPreset {
                slot: 2,
                name: "X".into(),
                values: vec![
                    (DYNAMIC_RANGE, dr),
                    (FILM, le(0x0B)),
                    (GRAIN, grain),
                    (COLOR_CHROME, le(3)),
                    (CC_FX_BLUE, le(2)),
                    (SMOOTH_SKIN, le(1)),
                    (WHITE_BALANCE, le(2)),
                    (WB_SHIFT_R, le(2)),
                    (WB_SHIFT_B, le(-4)),
                    (WB_KELVIN, le(10000)),
                    (HIGHLIGHT, le(-10)),
                    (SHADOW, le(-10)),
                    (COLOR, le(10)),
                    (SHARPNESS, le(-20)),
                    (HIGH_ISO_NR, vec![0x00, 0x80]),
                    (CLARITY, le(0)),
                ],
            };
            let r = to_recipe(&p).unwrap();
            assert!(encode_changes(&r, &p).unwrap().is_empty(), "dr/grain {:?}", (p.u16(DYNAMIC_RANGE), p.u16(GRAIN)));
        }
    }

    #[test]
    fn encodes_only_changed_fields() {
        let p = CameraPreset { slot: 1, name: "X".into(), values: vec![(FILM, le(2)), (SHADOW, le(-10)), (CLARITY, le(0))] };
        let mut r = to_recipe(&p).unwrap();
        r.film = Some("classic-chrome".into());
        r.shadow = Some(1.5);
        let ch = encode_changes(&r, &p).unwrap();
        assert_eq!(ch, vec![(FILM, le(0x0B)), (SHADOW, le(15))]);
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("SUPERIA 400").is_ok());
        assert!(validate_name("胶片").is_err());
        assert!(validate_name(&"A".repeat(26)).is_err());
    }

    #[test]
    fn dr_auto_and_grain_off_large() {
        let p = CameraPreset { slot: 3, name: String::new(), values: vec![(DYNAMIC_RANGE, le(-1)), (GRAIN, le(6))] };
        let r = to_recipe(&p).unwrap();
        assert_eq!(r.name, "C3");
        assert_eq!(r.dynamic_range, None);
        assert_eq!(r.grain.as_deref(), Some("off"));
    }
}
