//! libusb transport (macOS, Linux): talks PTP directly to the camera's USB
//! still-image interface, so vendor operations like Fuji's 0x900C/0x900D/0xD185
//! pass through untouched, exactly as on the Windows WPD path.
//!
//! macOS notes
//! * No driver install is needed (the camera is a standard USB class 6 device), and
//!   no special permission prompt either for a normal, non-sandboxed app.
//! * The system "PTPCamera" daemon (`ptpcamerad`, plus Photos / Image Capture) grabs
//!   every camera the moment it is plugged in. While it holds the interface we can't
//!   claim it, so when claiming fails we ask those daemons to let go (`killall`; they
//!   run as the logged-in user, no admin rights needed) and retry. launchd only
//!   starts them again on the next plug-in, by which time we own the interface.

use crate::codes::FUJI_VID;
use crate::ptp_usb::{Bulk, PtpUsb};
use anyhow::{anyhow, bail, Context, Result};
use rusb::{Direction, GlobalContext, TransferType};
use std::time::Duration;

pub type UsbTransport = PtpUsb<RusbBulk>;

/// USB still-image (PTP) interface class.
const CLASS_STILL_IMAGE: u8 = 0x06;
const CLASS_VENDOR: u8 = 0xFF;

#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// `usb:<bus>:<address>:<vid>:<pid>` — changes when the camera re-enumerates
    /// (replugged or USB mode switched), which the app uses to notice a mode change.
    pub id: String,
    pub friendly_name: String,
    pub manufacturer: String,
    pub description: String,
}

impl UsbDevice {
    pub fn is_fujifilm(&self) -> bool {
        self.id.contains(&format!(":{FUJI_VID:04x}:"))
    }
}

/// Nothing to initialise on this platform (kept so callers are OS-agnostic).
pub fn com_init() {}

#[derive(Debug, Clone, Copy)]
struct Iface {
    number: u8,
    ep_in: u8,
    ep_out: u8,
    mps_in: usize,
    mps_out: usize,
}

fn find_iface(dev: &rusb::Device<GlobalContext>) -> Option<Iface> {
    let cfg = dev.active_config_descriptor().or_else(|_| dev.config_descriptor(0)).ok()?;
    let mut fallback = None;
    for i in cfg.interfaces() {
        for d in i.descriptors() {
            let class = d.class_code();
            if class != CLASS_STILL_IMAGE && class != CLASS_VENDOR {
                continue;
            }
            let (mut ep_in, mut ep_out) = (None, None);
            for e in d.endpoint_descriptors() {
                if e.transfer_type() != TransferType::Bulk {
                    continue;
                }
                match e.direction() {
                    Direction::In => ep_in = Some((e.address(), e.max_packet_size() as usize)),
                    Direction::Out => ep_out = Some((e.address(), e.max_packet_size() as usize)),
                }
            }
            if let (Some((ei, mi)), Some((eo, mo))) = (ep_in, ep_out) {
                let found = Iface { number: d.interface_number(), ep_in: ei, ep_out: eo, mps_in: mi.max(64), mps_out: mo.max(64) };
                if class == CLASS_STILL_IMAGE {
                    return Some(found);
                }
                fallback.get_or_insert(found);
            }
        }
    }
    fallback
}

fn device_id(d: &rusb::Device<GlobalContext>, vid: u16, pid: u16) -> String {
    format!("usb:{}:{}:{:04x}:{:04x}", d.bus_number(), d.address(), vid, pid)
}

/// All USB cameras (devices exposing a still-image interface). Reads descriptors only:
/// it never opens a device, so polling this is harmless while the camera is in use.
pub fn list_devices() -> Result<Vec<UsbDevice>> {
    let mut out = Vec::new();
    for d in rusb::devices().context("enumerate USB devices")?.iter() {
        let Ok(desc) = d.device_descriptor() else { continue };
        let Some(_) = find_iface(&d) else { continue };
        let (vid, pid) = (desc.vendor_id(), desc.product_id());
        let fuji = vid == FUJI_VID;
        out.push(UsbDevice {
            id: device_id(&d, vid, pid),
            friendly_name: String::new(), // the real model name comes from PTP DeviceInfo
            manufacturer: if fuji { "FUJIFILM".into() } else { String::new() },
            description: format!("USB {vid:04x}:{pid:04x}"),
        });
    }
    Ok(out)
}

pub struct RusbBulk {
    h: rusb::DeviceHandle<GlobalContext>,
    iface: Iface,
    timeout: Duration,
}

impl RusbBulk {
    fn err(&self, what: &str, ep: u8, e: rusb::Error) -> anyhow::Error {
        if e == rusb::Error::Pipe {
            let _ = self.h.clear_halt(ep); // un-stall so the next transaction can work
        }
        match e {
            rusb::Error::Timeout => anyhow!("{what}: USB 传输超时（相机没有响应）"),
            rusb::Error::NoDevice => anyhow!("{what}: 相机已断开"),
            _ => anyhow!("{what}: {e}"),
        }
    }
}

impl Bulk for RusbBulk {
    fn max_packet_in(&self) -> usize {
        self.iface.mps_in
    }
    fn max_packet_out(&self) -> usize {
        self.iface.mps_out
    }
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.h.write_bulk(self.iface.ep_out, buf, self.timeout).map_err(|e| self.err("bulk write", self.iface.ep_out, e))
    }
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.h.read_bulk(self.iface.ep_in, buf, self.timeout).map_err(|e| self.err("bulk read", self.iface.ep_in, e))
    }
    fn set_timeout(&mut self, t: Duration) {
        self.timeout = t;
    }
}

impl Drop for RusbBulk {
    fn drop(&mut self) {
        let _ = self.h.release_interface(self.iface.number);
    }
}

/// macOS: make the system camera daemons let go of the device.
#[cfg(target_os = "macos")]
fn release_from_system_daemons(forceful: bool) {
    for name in ["ptpcamerad", "PTPCamera"] {
        let mut c = std::process::Command::new("killall");
        if forceful {
            c.arg("-9");
        }
        let _ = c.arg(name).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

#[cfg(not(target_os = "macos"))]
fn release_from_system_daemons(_forceful: bool) {}

fn try_open(dev: &rusb::Device<GlobalContext>, iface: Iface) -> Result<rusb::DeviceHandle<GlobalContext>, rusb::Error> {
    let h = dev.open()?;
    let _ = h.set_auto_detach_kernel_driver(true); // Linux; "not supported" on macOS is fine
    if h.active_configuration().unwrap_or(0) == 0 {
        let _ = h.set_active_configuration(1);
    }
    h.claim_interface(iface.number)?;
    // start from a clean slate: clear any stall left by a previous (crashed) session
    let _ = h.clear_halt(iface.ep_in);
    let _ = h.clear_halt(iface.ep_out);
    Ok(h)
}

impl PtpUsb<RusbBulk> {
    pub fn open(info: &UsbDevice) -> Result<Self> {
        let list = rusb::devices().context("enumerate USB devices")?;
        let (dev, iface) = list
            .iter()
            .find_map(|d| {
                let desc = d.device_descriptor().ok()?;
                (device_id(&d, desc.vendor_id(), desc.product_id()) == info.id).then(|| find_iface(&d).map(|i| (d, i)))?
            })
            .ok_or_else(|| anyhow!("相机不见了（USB 设备 {} 已断开或重新枚举）", info.id))?;

        let mut last = None;
        for attempt in 0..8 {
            match try_open(&dev, iface) {
                Ok(h) => {
                    let label = format!("USB: {} ({})", info.description, info.id);
                    let io = RusbBulk { h, iface, timeout: Duration::from_secs(30) };
                    return Ok(PtpUsb::new(io, label, Duration::from_secs(30)));
                }
                Err(e) => {
                    last = Some(e);
                    if matches!(e, rusb::Error::NoDevice | rusb::Error::NotFound) {
                        break;
                    }
                    // busy / access denied: most likely the system's camera daemon owns it
                    release_from_system_daemons(attempt >= 2);
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
        }
        let e = last.unwrap_or(rusb::Error::Other);
        if cfg!(target_os = "macos") {
            bail!(
                "无法占用相机的 USB 接口（{e}）。请退出「照片」「图像捕捉」等会自动打开相机的 App，\
                 然后重新插拔数据线；或在终端运行 killall ptpcamerad 后重试"
            );
        }
        bail!("无法打开相机的 USB 接口（{e}）。Linux 下可能需要 udev 规则或 root 权限");
    }
}
