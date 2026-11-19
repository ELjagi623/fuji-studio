//! Little-endian PTP dataset encoding / decoding.

use anyhow::{bail, Result};

pub struct Reader<'a> {
    b: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.b.len() {
            bail!("PTP dataset truncated at {} (+{n}, len {})", self.pos, self.b.len());
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    pub fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    /// PTP string: u8 char count (incl. NUL) followed by UCS-2LE chars.
    pub fn string(&mut self) -> Result<String> {
        let n = self.u8()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        let raw = self.take(n * 2)?;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        Ok(String::from_utf16_lossy(&units))
    }
    pub fn u16_array(&mut self) -> Result<Vec<u16>> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u16()).collect()
    }
    pub fn u32_array(&mut self) -> Result<Vec<u32>> {
        let n = self.u32()? as usize;
        (0..n).map(|_| self.u32()).collect()
    }
}

#[derive(Default)]
pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn string(&mut self, s: &str) -> &mut Self {
        if s.is_empty() {
            return self.u8(0);
        }
        let units: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        self.u8(units.len() as u8);
        for u in units {
            self.u16(u);
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub standard_version: u16,
    pub vendor_extension_id: u32,
    pub vendor_extension_version: u16,
    pub vendor_extension_desc: String,
    pub functional_mode: u16,
    pub operations: Vec<u16>,
    pub events: Vec<u16>,
    pub properties: Vec<u16>,
    pub capture_formats: Vec<u16>,
    pub image_formats: Vec<u16>,
    pub manufacturer: String,
    pub model: String,
    pub device_version: String,
    pub serial_number: String,
}

impl DeviceInfo {
    pub fn parse(b: &[u8]) -> Result<Self> {
        let mut r = Reader::new(b);
        Ok(Self {
            standard_version: r.u16()?,
            vendor_extension_id: r.u32()?,
            vendor_extension_version: r.u16()?,
            vendor_extension_desc: r.string()?,
            functional_mode: r.u16()?,
            operations: r.u16_array()?,
            events: r.u16_array()?,
            properties: r.u16_array()?,
            capture_formats: r.u16_array()?,
            image_formats: r.u16_array()?,
            manufacturer: r.string()?,
            model: r.string()?,
            device_version: r.string()?,
            serial_number: r.string()?,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ObjectInfo {
    pub storage_id: u32,
    pub format: u16,
    pub protection: u16,
    pub compressed_size: u32,
    pub thumb_format: u16,
    pub thumb_size: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub bit_depth: u32,
    pub parent: u32,
    pub association_type: u16,
    pub association_desc: u32,
    pub sequence_number: u32,
    pub filename: String,
    pub capture_date: String,
    pub modification_date: String,
    pub keywords: String,
}

impl ObjectInfo {
    pub fn parse(b: &[u8]) -> Result<Self> {
        let mut r = Reader::new(b);
        Ok(Self {
            storage_id: r.u32()?,
            format: r.u16()?,
            protection: r.u16()?,
            compressed_size: r.u32()?,
            thumb_format: r.u16()?,
            thumb_size: r.u32()?,
            thumb_width: r.u32()?,
            thumb_height: r.u32()?,
            image_width: r.u32()?,
            image_height: r.u32()?,
            bit_depth: r.u32()?,
            parent: r.u32()?,
            association_type: r.u16()?,
            association_desc: r.u32()?,
            sequence_number: r.u32()?,
            filename: r.string()?,
            capture_date: r.string().unwrap_or_default(),
            modification_date: r.string().unwrap_or_default(),
            keywords: r.string().unwrap_or_default(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.storage_id)
            .u16(self.format)
            .u16(self.protection)
            .u32(self.compressed_size)
            .u16(self.thumb_format)
            .u32(self.thumb_size)
            .u32(self.thumb_width)
            .u32(self.thumb_height)
            .u32(self.image_width)
            .u32(self.image_height)
            .u32(self.bit_depth)
            .u32(self.parent)
            .u16(self.association_type)
            .u32(self.association_desc)
            .u32(self.sequence_number)
            .string(&self.filename)
            .string(&self.capture_date)
            .string(&self.modification_date)
            .string(&self.keywords);
        w.0
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_roundtrip() {
        let mut w = Writer::default();
        w.string("FUP_FILE.dat").string("");
        let mut r = Reader::new(&w.0);
        assert_eq!(r.string().unwrap(), "FUP_FILE.dat");
        assert_eq!(r.string().unwrap(), "");
    }

    #[test]
    fn object_info_roundtrip() {
        let oi = ObjectInfo {
            format: 0xF802,
            compressed_size: 123,
            filename: "a.RAF".into(),
            ..Default::default()
        };
        let back = ObjectInfo::parse(&oi.encode()).unwrap();
        assert_eq!(back.format, 0xF802);
        assert_eq!(back.compressed_size, 123);
        assert_eq!(back.filename, "a.RAF");
    }
}
