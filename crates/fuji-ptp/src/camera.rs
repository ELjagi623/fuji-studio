//! High-level Fujifilm camera operations over any `Transport`.

use crate::codes::{format, op, prop, resp, resp_name};
use crate::data::{hex, DeviceInfo, ObjectInfo, Reader, Writer};
use crate::profile::Profile;
use crate::recipe::Recipe;
use crate::transport::{DataPhase, Progress, Response, Transport};
use anyhow::{bail, Context, Result};
use std::time::{Duration, Instant};

pub struct FujiCamera<T: Transport> {
    pub t: T,
    pub verbose: bool,
    session_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderQuality {
    Preview = 0,
    Full = 1,
}

#[derive(Debug, Clone, Copy)]
pub enum Stage {
    Uploading { done: u64, total: u64 },
    ReadingProfile,
    WritingProfile,
    Converting,
    Downloading { done: u64, total: u64 },
}

#[derive(Debug, Default, Clone)]
pub struct Timings {
    pub upload: Duration,
    pub profile: Duration,
    pub convert: Duration,
    pub download: Duration,
}

impl Timings {
    pub fn total(&self) -> Duration {
        self.upload + self.profile + self.convert + self.download
    }
}

pub struct ConvertResult {
    pub jpeg: Vec<u8>,
    pub base_profile: Profile,
    pub applied_profile: Profile,
    pub changes: Vec<String>,
    pub timings: Timings,
}

impl<T: Transport> FujiCamera<T> {
    pub fn new(t: T) -> Self {
        Self { t, verbose: false, session_open: false }
    }

    pub fn call(
        &mut self,
        opc: u16,
        params: &[u32],
        data: DataPhase<'_>,
        progress: Progress<'_>,
    ) -> Result<Response> {
        let dir = match &data {
            DataPhase::None => "    ".to_string(),
            DataPhase::In => " <- ".to_string(),
            DataPhase::Out(b) => format!(" -> {}B", b.len()),
        };
        let t0 = Instant::now();
        let r = self.t.execute(opc, params, data, progress);
        if self.verbose {
            let name = crate::codes::op_name(opc).unwrap_or("?");
            match &r {
                Ok(r) => eprintln!(
                    "[ptp] 0x{opc:04X} {name:<22} {params:08X?}{dir} => {} params={:X?} data={}B ({:?})",
                    resp_name(r.code),
                    r.params,
                    r.data.len(),
                    t0.elapsed()
                ),
                Err(e) => eprintln!("[ptp] 0x{opc:04X} {name:<22} {params:08X?}{dir} => ERROR {e:#}"),
            }
        }
        r
    }

    fn ok(&mut self, opc: u16, params: &[u32], data: DataPhase<'_>, progress: Progress<'_>) -> Result<Response> {
        let r = self.call(opc, params, data, progress)?;
        if !r.is_ok() {
            let name = crate::codes::op_name(opc).unwrap_or("op");
            bail!("{name} (0x{opc:04X}) failed: {}", resp_name(r.code));
        }
        Ok(r)
    }

    pub fn open_session(&mut self) -> Result<()> {
        if self.t.driver_manages_session() || self.session_open {
            return Ok(());
        }
        let r = self.call(op::OPEN_SESSION, &[1], DataPhase::None, None)?;
        if r.code == resp::SESSION_ALREADY_OPEN {
            self.call(op::CLOSE_SESSION, &[], DataPhase::None, None)?;
            self.ok(op::OPEN_SESSION, &[1], DataPhase::None, None)?;
        } else if !r.is_ok() {
            bail!("OpenSession failed: {}", resp_name(r.code));
        }
        self.session_open = true;
        Ok(())
    }

    pub fn close_session(&mut self) -> Result<()> {
        if self.session_open {
            self.session_open = false;
            self.call(op::CLOSE_SESSION, &[], DataPhase::None, None)?;
        }
        Ok(())
    }

    pub fn device_info(&mut self) -> Result<DeviceInfo> {
        let r = self.ok(op::GET_DEVICE_INFO, &[], DataPhase::In, None)?;
        DeviceInfo::parse(&r.data).context("parse DeviceInfo")
    }

    pub fn get_prop(&mut self, id: u16) -> Result<Vec<u8>> {
        Ok(self.ok(op::GET_DEVICE_PROP_VALUE, &[id as u32], DataPhase::In, None)?.data)
    }

    pub fn get_prop_desc(&mut self, id: u16) -> Result<Vec<u8>> {
        Ok(self.ok(op::GET_DEVICE_PROP_DESC, &[id as u32], DataPhase::In, None)?.data)
    }

    pub fn set_prop(&mut self, id: u16, value: &[u8]) -> Result<()> {
        self.ok(op::SET_DEVICE_PROP_VALUE, &[id as u32], DataPhase::Out(value), None)?;
        Ok(())
    }

    pub fn storage_ids(&mut self) -> Result<Vec<u32>> {
        let r = self.ok(op::GET_STORAGE_IDS, &[], DataPhase::In, None)?;
        Reader::new(&r.data).u32_array()
    }

    /// storage 0xFFFFFFFF = all, format 0 = any, parent 0 = all / 0xFFFFFFFF = root
    pub fn object_handles(&mut self, storage: u32, fmt: u32, parent: u32) -> Result<Vec<u32>> {
        let r = self.ok(op::GET_OBJECT_HANDLES, &[storage, fmt, parent], DataPhase::In, None)?;
        if r.data.len() < 4 {
            return Ok(Vec::new());
        }
        Reader::new(&r.data).u32_array()
    }

    pub fn object_info(&mut self, h: u32) -> Result<ObjectInfo> {
        let r = self.ok(op::GET_OBJECT_INFO, &[h], DataPhase::In, None)?;
        ObjectInfo::parse(&r.data)
    }

    pub fn get_object(&mut self, h: u32, progress: Progress<'_>) -> Result<Vec<u8>> {
        Ok(self.ok(op::GET_OBJECT, &[h], DataPhase::In, progress)?.data)
    }

    pub fn get_thumb(&mut self, h: u32) -> Result<Vec<u8>> {
        Ok(self.ok(op::GET_THUMB, &[h], DataPhase::In, None)?.data)
    }

    pub fn delete_object(&mut self, h: u32) -> Result<()> {
        self.ok(op::DELETE_OBJECT, &[h, 0], DataPhase::None, None)?;
        Ok(())
    }

    /// Upload a RAF into camera memory using Fuji vendor ops 0x900C / 0x900D.
    pub fn send_raf(&mut self, raf: &[u8], progress: Progress<'_>) -> Result<()> {
        let info = ObjectInfo {
            format: format::FUJI_RAF,
            compressed_size: raf.len() as u32,
            filename: "FUP_FILE.dat".into(),
            ..Default::default()
        };
        self.ok(op::FUJI_SEND_OBJECT_INFO, &[0, 0, 0], DataPhase::Out(&info.encode()), None)?;
        self.ok(op::FUJI_SEND_OBJECT, &[], DataPhase::Out(raf), progress)?;
        Ok(())
    }

    pub fn get_profile(&mut self) -> Result<Profile> {
        let raw = self.get_prop(prop::RAW_CONV_PROFILE)?;
        if raw.is_empty() {
            bail!("D185 profile is empty — is a RAF loaded?");
        }
        Profile::new(raw.clone()).with_context(|| format!("D185 head: {}", hex(&raw[..raw.len().min(32)])))
    }

    pub fn set_profile(&mut self, p: &Profile) -> Result<()> {
        self.set_prop(prop::RAW_CONV_PROFILE, &p.raw)
    }

    /// D183 trigger: 0 = quick preview render (S size, ~0.3 s on X-Pro3),
    /// 1 = full-resolution render (L size). Verified on X-Pro3 fw 2.00.
    pub fn start_conversion(&mut self, quality: RenderQuality) -> Result<()> {
        let mut w = Writer::default();
        w.u16(quality as u16);
        self.set_prop(prop::START_RAW_CONVERSION, &w.0)
    }

    /// Poll for a new object that was not present in `before`.
    pub fn wait_for_new_object(&mut self, before: &[u32], timeout: Duration) -> Result<u32> {
        let t0 = Instant::now();
        loop {
            match self.object_handles(0xFFFF_FFFF, 0, 0) {
                Ok(hs) => {
                    if let Some(h) = hs.into_iter().find(|h| !before.contains(h)) {
                        return Ok(h);
                    }
                }
                Err(e) if self.verbose => eprintln!("[poll] {e:#}"),
                Err(_) => {}
            }
            if t0.elapsed() > timeout {
                bail!("conversion timeout after {:?}", timeout);
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    /// Full pipeline: upload RAF -> read D185 -> apply recipe -> write -> convert -> download -> delete.
    pub fn convert(
        &mut self,
        raf: &[u8],
        recipe: Option<&Recipe>,
        quality: RenderQuality,
        on_stage: &mut dyn FnMut(Stage),
    ) -> Result<ConvertResult> {
        let mut tm = Timings::default();
        let before = self.object_handles(0xFFFF_FFFF, 0, 0).unwrap_or_default();

        let t = Instant::now();
        {
            let mut cb = |d, n| on_stage(Stage::Uploading { done: d, total: n });
            self.send_raf(raf, Some(&mut cb)).context("upload RAF")?;
        }
        tm.upload = t.elapsed();

        let t = Instant::now();
        on_stage(Stage::ReadingProfile);
        let base = self.get_profile()?;
        let mut applied = base.clone();
        let changes = match recipe {
            Some(r) => r.apply(&mut applied)?,
            None => Vec::new(),
        };
        on_stage(Stage::WritingProfile);
        self.set_profile(&applied).context("write D185 profile")?;
        tm.profile = t.elapsed();

        let t = Instant::now();
        on_stage(Stage::Converting);
        self.start_conversion(quality).context("start conversion (D183)")?;
        let h = self.wait_for_new_object(&before, Duration::from_secs(90))?;
        tm.convert = t.elapsed();

        let t = Instant::now();
        let jpeg = {
            let mut cb = |d, n| on_stage(Stage::Downloading { done: d, total: n });
            self.get_object(h, Some(&mut cb)).context("download result")?
        };
        if let Err(e) = self.delete_object(h) {
            if self.verbose {
                eprintln!("[warn] delete temp object 0x{h:08X}: {e:#}");
            }
        }
        tm.download = t.elapsed();

        Ok(ConvertResult { jpeg, base_profile: base, applied_profile: applied, changes, timings: tm })
    }
}
