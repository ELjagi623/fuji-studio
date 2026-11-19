//! Windows transport: raw PTP pass-through via the WPD MTP extension commands
//! (WPD_COMMAND_MTP_EXT_*). Works with the stock Windows MTP/PTP driver, so no
//! Zadig/WinUSB driver swap is needed and official Fuji software keeps working.
//!
//! The WPD driver owns the PTP session and transaction IDs; we never send
//! OpenSession/CloseSession ourselves.

use crate::transport::{DataPhase, Progress, Response, Transport};
use anyhow::{anyhow, Context, Result};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Devices::PortableDevices::*;
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, PROPERTYKEY};
use windows::Win32::System::Com::StructuredStorage::{
    PropVariantClear, PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VT_UI4;

const DEFAULT_CHUNK: u32 = 1024 * 1024;
const SECURITY_IMPERSONATION: u32 = 0x0002_0000;

#[derive(Debug, Clone)]
pub struct WpdDevice {
    pub id: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub description: String,
}

impl WpdDevice {
    pub fn is_fujifilm(&self) -> bool {
        self.manufacturer.to_ascii_uppercase().contains("FUJI")
            || self.id.to_ascii_lowercase().contains("vid_04cb")
    }
}

/// Initialise COM for this thread (multi-threaded apartment). Safe to call repeatedly.
pub fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

fn pv_u32(v: u32) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { ulVal: v },
            }),
        },
    }
}

pub fn list_devices() -> Result<Vec<WpdDevice>> {
    com_init();
    unsafe {
        let mgr: IPortableDeviceManager =
            CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)
                .context("create PortableDeviceManager")?;
        let _ = mgr.RefreshDeviceList();
        let mut n = 0u32;
        mgr.GetDevices(std::ptr::null_mut(), &mut n)?;
        let mut ids = vec![PWSTR::null(); n as usize];
        if n > 0 {
            mgr.GetDevices(ids.as_mut_ptr(), &mut n)?;
        }
        let mut out = Vec::new();
        for p in ids.into_iter().take(n as usize) {
            let id = take_pwstr(p);
            let wid = wide(&id);
            let pid = PCWSTR(wid.as_ptr());
            let get = |f: &dyn Fn(PWSTR, *mut u32) -> windows::core::Result<()>| -> String {
                let mut len = 0u32;
                if f(PWSTR::null(), &mut len).is_err() || len == 0 {
                    return String::new();
                }
                let mut buf = vec![0u16; len as usize];
                if f(PWSTR(buf.as_mut_ptr()), &mut len).is_err() {
                    return String::new();
                }
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..end])
            };
            let friendly_name = get(&|b, l| mgr.GetDeviceFriendlyName(pid, b, l));
            let manufacturer = get(&|b, l| mgr.GetDeviceManufacturer(pid, b, l));
            let description = get(&|b, l| mgr.GetDeviceDescription(pid, b, l));
            out.push(WpdDevice { id, friendly_name, manufacturer, description });
        }
        Ok(out)
    }
}

pub struct WpdTransport {
    dev: IPortableDevice,
    info: WpdDevice,
}

impl WpdTransport {
    pub fn open(info: &WpdDevice) -> Result<Self> {
        com_init();
        unsafe {
            let client: IPortableDeviceValues =
                CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
            let name = wide("fujictl");
            client.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(name.as_ptr()))?;
            client.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 0)?;
            client.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 1)?;
            client.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0)?;
            client.SetUnsignedIntegerValue(
                &WPD_CLIENT_SECURITY_QUALITY_OF_SERVICE,
                SECURITY_IMPERSONATION,
            )?;
            client.SetUnsignedIntegerValue(
                &WPD_CLIENT_DESIRED_ACCESS,
                GENERIC_READ.0 | GENERIC_WRITE.0,
            )?;
            let dev: IPortableDevice =
                CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER)?;
            let wid = wide(&info.id);
            dev.Open(PCWSTR(wid.as_ptr()), &client)
                .with_context(|| format!("open WPD device {}", info.friendly_name))?;
            Ok(Self { dev, info: info.clone() })
        }
    }

    unsafe fn new_cmd(&self, cmd: &PROPERTYKEY) -> Result<IPortableDeviceValues> {
        let v: IPortableDeviceValues =
            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)?;
        let fmtid: GUID = cmd.fmtid;
        v.SetGuidValue(&WPD_PROPERTY_COMMON_COMMAND_CATEGORY, &fmtid)?;
        v.SetUnsignedIntegerValue(&WPD_PROPERTY_COMMON_COMMAND_ID, cmd.pid)?;
        Ok(v)
    }

    unsafe fn send(&self, v: &IPortableDeviceValues, what: &str) -> Result<IPortableDeviceValues> {
        let r = self.dev.SendCommand(0, v).with_context(|| format!("WPD SendCommand ({what})"))?;
        r.GetErrorValue(&WPD_PROPERTY_COMMON_HRESULT)?
            .ok()
            .with_context(|| format!("WPD driver error ({what})"))?;
        Ok(r)
    }

    unsafe fn response(r: &IPortableDeviceValues) -> Result<(u16, Vec<u32>)> {
        let code = r.GetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_RESPONSE_CODE)? as u16;
        let mut params = Vec::new();
        if let Ok(coll) = r.GetIPortableDevicePropVariantCollectionValue(&WPD_PROPERTY_MTP_EXT_RESPONSE_PARAMS) {
            let mut n = 0u32;
            coll.GetCount(&mut n)?;
            for i in 0..n {
                let mut pv = PROPVARIANT::default();
                coll.GetAt(i, &mut pv)?;
                params.push(pv.Anonymous.Anonymous.Anonymous.ulVal);
                let _ = PropVariantClear(&mut pv);
            }
        }
        Ok((code, params))
    }

    unsafe fn context_of(r: &IPortableDeviceValues) -> Result<(Vec<u16>, u64, u32)> {
        let ctx = take_pwstr(r.GetStringValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_CONTEXT)?);
        let total = r.GetUnsignedLargeIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_TOTAL_DATA_SIZE).unwrap_or(0);
        let chunk = r
            .GetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_OPTIMAL_TRANSFER_BUFFER_SIZE)
            .unwrap_or(DEFAULT_CHUNK)
            .clamp(64 * 1024, 8 * 1024 * 1024);
        Ok((wide(&ctx), total, chunk))
    }

    unsafe fn end_transfer(&self, ctx: &[u16]) -> Result<(u16, Vec<u32>)> {
        let v = self.new_cmd(&WPD_COMMAND_MTP_EXT_END_DATA_TRANSFER)?;
        v.SetStringValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_CONTEXT, PCWSTR(ctx.as_ptr()))?;
        let r = self.send(&v, "END_DATA_TRANSFER")?;
        Self::response(&r)
    }
}

impl Transport for WpdTransport {
    fn describe(&self) -> String {
        format!("WPD: {} ({})", self.info.friendly_name, self.info.manufacturer)
    }

    fn driver_manages_session(&self) -> bool {
        true
    }

    fn execute(
        &mut self,
        op: u16,
        params: &[u32],
        data: DataPhase<'_>,
        mut progress: Progress<'_>,
    ) -> Result<Response> {
        unsafe {
            let cmd = match data {
                DataPhase::None => &WPD_COMMAND_MTP_EXT_EXECUTE_COMMAND_WITHOUT_DATA_PHASE,
                DataPhase::In => &WPD_COMMAND_MTP_EXT_EXECUTE_COMMAND_WITH_DATA_TO_READ,
                DataPhase::Out(_) => &WPD_COMMAND_MTP_EXT_EXECUTE_COMMAND_WITH_DATA_TO_WRITE,
            };
            let v = self.new_cmd(cmd)?;
            v.SetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_OPERATION_CODE, op as u32)?;
            let coll: IPortableDevicePropVariantCollection =
                CoCreateInstance(&PortableDevicePropVariantCollection, None, CLSCTX_INPROC_SERVER)?;
            for &p in params {
                coll.Add(&pv_u32(p))?;
            }
            v.SetIPortableDevicePropVariantCollectionValue(&WPD_PROPERTY_MTP_EXT_OPERATION_PARAMS, &coll)?;
            if let DataPhase::Out(buf) = &data {
                v.SetUnsignedLargeIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_TOTAL_DATA_SIZE, buf.len() as u64)?;
            }
            let what = format!("op 0x{op:04X}");
            let r = self.send(&v, &what)?;

            match data {
                DataPhase::None => {
                    let (code, params) = Self::response(&r)?;
                    Ok(Response { code, params, data: Vec::new() })
                }
                DataPhase::In => {
                    let (ctx, total, chunk) = Self::context_of(&r)?;
                    let mut out: Vec<u8> = Vec::with_capacity(total as usize);
                    loop {
                        if total > 0 && out.len() as u64 >= total {
                            break;
                        }
                        let want = if total > 0 {
                            ((total - out.len() as u64).min(chunk as u64)) as u32
                        } else {
                            chunk
                        };
                        let rv = self.new_cmd(&WPD_COMMAND_MTP_EXT_READ_DATA)?;
                        rv.SetStringValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_CONTEXT, PCWSTR(ctx.as_ptr()))?;
                        rv.SetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_NUM_BYTES_TO_READ, want)?;
                        let zero = vec![0u8; want as usize];
                        rv.SetBufferValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_DATA, &zero)?;
                        let rr = match self.send(&rv, "READ_DATA") {
                            Ok(rr) => rr,
                            Err(e) if total == 0 => {
                                let _ = e;
                                break;
                            }
                            Err(e) => {
                                let _ = self.end_transfer(&ctx);
                                return Err(e);
                            }
                        };
                        let got = rr.GetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_NUM_BYTES_READ)?;
                        let mut p: *mut u8 = std::ptr::null_mut();
                        let mut len = 0u32;
                        rr.GetBufferValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_DATA, &mut p, &mut len)?;
                        if !p.is_null() {
                            let n = got.min(len) as usize;
                            out.extend_from_slice(std::slice::from_raw_parts(p, n));
                            CoTaskMemFree(Some(p as *const c_void));
                        }
                        if let Some(cb) = progress.as_mut() {
                            cb(out.len() as u64, total);
                        }
                        if got == 0 || total == 0 {
                            break;
                        }
                    }
                    let (code, params) = self.end_transfer(&ctx)?;
                    Ok(Response { code, params, data: out })
                }
                DataPhase::Out(buf) => {
                    let (ctx, _total, chunk) = Self::context_of(&r)?;
                    let mut sent = 0usize;
                    while sent < buf.len() {
                        let n = (buf.len() - sent).min(chunk as usize);
                        let wv = self.new_cmd(&WPD_COMMAND_MTP_EXT_WRITE_DATA)?;
                        wv.SetStringValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_CONTEXT, PCWSTR(ctx.as_ptr()))?;
                        wv.SetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_NUM_BYTES_TO_WRITE, n as u32)?;
                        wv.SetBufferValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_DATA, &buf[sent..sent + n])?;
                        let wr = match self.send(&wv, "WRITE_DATA") {
                            Ok(wr) => wr,
                            Err(e) => {
                                let _ = self.end_transfer(&ctx);
                                return Err(e);
                            }
                        };
                        let written = wr
                            .GetUnsignedIntegerValue(&WPD_PROPERTY_MTP_EXT_TRANSFER_NUM_BYTES_WRITTEN)
                            .unwrap_or(n as u32) as usize;
                        if written == 0 {
                            let _ = self.end_transfer(&ctx);
                            return Err(anyhow!("WPD WRITE_DATA wrote 0 bytes at offset {sent}"));
                        }
                        sent += written;
                        if let Some(cb) = progress.as_mut() {
                            cb(sent as u64, buf.len() as u64);
                        }
                    }
                    let (code, params) = self.end_transfer(&ctx)?;
                    Ok(Response { code, params, data: Vec::new() })
                }
            }
        }
    }
}

impl Drop for WpdTransport {
    fn drop(&mut self) {
        unsafe {
            let _ = self.dev.Close();
        }
    }
}
