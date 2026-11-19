//! Cached JPEG thumbnails for RAF files (from the embedded preview) and JPEGs.

use anyhow::{Context, Result};
use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::{DynamicImage, ImageDecoder};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn cache_key(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    meta.len().hash(&mut h);
    if let Ok(m) = meta.modified() {
        m.hash(&mut h);
    }
    Ok(format!("{:016x}", h.finish()))
}

/// Decode a JPEG and rotate it according to its EXIF orientation.
pub fn decode_oriented(bytes: &[u8]) -> Result<DynamicImage> {
    let mut dec = JpegDecoder::new(Cursor::new(bytes))?;
    let orientation = dec.orientation().ok();
    let mut img = DynamicImage::from_decoder(dec)?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img)
}

/// Return the path of a cached thumbnail (longest side `max` px), creating it if needed.
pub fn thumbnail(cache_dir: &Path, path: &Path, max: u32) -> Result<PathBuf> {
    let out = cache_dir.join(format!("{}_{max}.jpg", cache_key(path)?));
    if out.exists() {
        return Ok(out);
    }
    let jpg = if raf::has_raf_extension(path) {
        raf::read_embedded_jpeg(path)?
    } else {
        std::fs::read(path)?
    };
    let img = decode_oriented(&jpg)?.thumbnail(max, max).to_rgb8();
    std::fs::create_dir_all(cache_dir)?;
    let tmp = out.with_extension("tmp");
    {
        let f = std::fs::File::create(&tmp)?;
        let mut enc = JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 82);
        enc.encode_image(&img)?;
    }
    std::fs::rename(&tmp, &out)?;
    Ok(out)
}
