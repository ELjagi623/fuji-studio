//! D185 RAW-conversion profile ("native" format returned by the camera).
//!
//! Layout (per FilmKit, confirmed on X100VI; X-Pro3 is UNVERIFIED):
//!   [0..2]  u16 LE  number of int32 parameters (N)
//!   ...     opaque header (kept byte-for-byte)
//!   [len-N*4 ..]  N × i32 LE parameters
//! We only patch the parameters the user changes; everything else is preserved
//! so the camera falls back to the RAF's EXIF values.

use anyhow::{bail, Result};

/// Indices verified on X-Pro3 fw 2.00 (621-byte profile, 28 params):
/// the layout is rawji's PARAM_INDEX shifted by +1.
pub mod idx {
    pub const SHOOTING_CONDITION: usize = 1;
    pub const FILE_TYPE: usize = 2; // 7 = JPEG
    pub const IMAGE_SIZE: usize = 3; // 7 = L 3:2 (output size is really chosen by the D183 trigger value)
    pub const IMAGE_QUALITY: usize = 4; // 2 Fine, 3 Normal
    pub const EXPOSURE_BIAS: usize = 5; // millistops (1000 = +1 EV) — unverified
    pub const DYNAMIC_RANGE: usize = 6; // 100 / 200 / 400
    pub const WIDE_D_RANGE: usize = 7; // D-Range priority
    pub const FILM_SIMULATION: usize = 8; // verified
    pub const GRAIN_EFFECT: usize = 9; // 1 Off 2 Weak 3 Strong (4/5 = large on newer bodies)
    pub const COLOR_CHROME: usize = 10; // 1 Off 2 Weak 3 Strong
    pub const WB_SHOOT_COND: usize = 11;
    pub const WHITE_BALANCE: usize = 12; // 0 = as shot
    pub const WB_SHIFT_R: usize = 13;
    pub const WB_SHIFT_B: usize = 14;
    pub const WB_COLOR_TEMP: usize = 15; // Kelvin
    pub const HIGHLIGHT_TONE: usize = 16; // ×10
    pub const SHADOW_TONE: usize = 17; // ×10
    pub const COLOR: usize = 18; // ×10
    pub const SHARPNESS: usize = 19; // ×10
    pub const NOISE_REDUCTION: usize = 20; // proprietary, see nr_encode
    pub const COLOR_SPACE: usize = 22; // 1 sRGB, 2 AdobeRGB
    pub const BLACK_IMAGE_TONE: usize = 23;
    pub const SMOOTH_SKIN: usize = 24; // 1 Off 2 Weak 3 Strong
    pub const COLOR_CHROME_FX_BLUE: usize = 25; // 1 Off 2 Weak 3 Strong
    pub const MONO_COLOR_RG: usize = 26;
    pub const CLARITY: usize = 27; // ×10
}

pub fn field_name(i: usize) -> Option<&'static str> {
    Some(match i {
        idx::SHOOTING_CONDITION => "ShootingCondition",
        idx::FILE_TYPE => "FileType",
        idx::IMAGE_SIZE => "ImageSize",
        idx::IMAGE_QUALITY => "ImageQuality",
        idx::EXPOSURE_BIAS => "ExposureBias(mEV)",
        idx::DYNAMIC_RANGE => "DynamicRange%",
        idx::WIDE_D_RANGE => "DRangePriority",
        idx::FILM_SIMULATION => "FilmSimulation",
        idx::GRAIN_EFFECT => "GrainEffect",
        idx::COLOR_CHROME => "ColorChrome",
        idx::WB_SHOOT_COND => "WBShootCond",
        idx::COLOR_SPACE => "ColorSpace",
        idx::BLACK_IMAGE_TONE => "BlackImageTone",
        idx::MONO_COLOR_RG => "MonoColorRG",
        idx::SMOOTH_SKIN => "SmoothSkin",
        idx::WHITE_BALANCE => "WhiteBalance",
        idx::WB_SHIFT_R => "WBShiftR",
        idx::WB_SHIFT_B => "WBShiftB",
        idx::WB_COLOR_TEMP => "WBColorTemp(K)",
        idx::HIGHLIGHT_TONE => "Highlight×10",
        idx::SHADOW_TONE => "Shadow×10",
        idx::COLOR => "Color×10",
        idx::SHARPNESS => "Sharpness×10",
        idx::NOISE_REDUCTION => "NoiseReduction(enc)",
        idx::COLOR_CHROME_FX_BLUE => "ColorChromeFxBlue",
        idx::CLARITY => "Clarity×10",
        _ => return None,
    })
}

/// Noise reduction -4..+4 → camera encoding.
pub fn nr_encode(v: i32) -> Option<i32> {
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

#[derive(Clone, Debug)]
pub struct Profile {
    pub raw: Vec<u8>,
}

impl Profile {
    pub fn new(raw: Vec<u8>) -> Result<Self> {
        let p = Self { raw };
        p.params_offset()?;
        Ok(p)
    }

    pub fn num_params(&self) -> usize {
        if self.raw.len() < 2 {
            return 0;
        }
        u16::from_le_bytes([self.raw[0], self.raw[1]]) as usize
    }

    pub fn params_offset(&self) -> Result<usize> {
        let n = self.num_params();
        if n == 0 || n * 4 + 2 > self.raw.len() {
            bail!(
                "unexpected D185 layout: len={} num_params={n}",
                self.raw.len()
            );
        }
        Ok(self.raw.len() - n * 4)
    }

    pub fn get(&self, i: usize) -> Option<i32> {
        let off = self.params_offset().ok()? + i * 4;
        let b = self.raw.get(off..off + 4)?;
        Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn set(&mut self, i: usize, v: i32) -> Result<()> {
        if i >= self.num_params() {
            bail!("D185 param index {i} out of range ({})", self.num_params());
        }
        let off = self.params_offset()? + i * 4;
        self.raw[off..off + 4].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    pub fn params(&self) -> Vec<i32> {
        (0..self.num_params()).filter_map(|i| self.get(i)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_param() {
        // 2-byte count (3 params), 6 bytes header filler, 3 params
        let mut raw = vec![3u8, 0, 9, 9, 9, 9, 9, 9];
        for v in [10i32, 20, 30] {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        let mut p = Profile::new(raw).unwrap();
        assert_eq!(p.params(), vec![10, 20, 30]);
        p.set(1, -5).unwrap();
        assert_eq!(p.get(1), Some(-5));
        assert_eq!(&p.raw[2..8], &[9, 9, 9, 9, 9, 9]);
        assert!(p.set(3, 0).is_err());
    }
}
