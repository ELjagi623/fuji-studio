//! Camera worker thread. The camera handle (a COM object on Windows) is not Send, so a
//! single thread owns it and processes jobs serially (the camera can only do one
//! conversion at a time anyway).

use crate::settings::unix_date;
use anyhow::{anyhow, bail, Context, Result};
use fuji_ptp::platform::{self, CameraTransport};
use fuji_ptp::presets::CameraPreset;
use fuji_ptp::{FujiCamera, Profile, Recipe, RenderQuality, Stage};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Debug)]
pub struct CamInfo {
    pub model: String,
    pub firmware: String,
    pub serial: String,
    /// Camera is in USB RAW CONV mode (or at least looks like it; see `looks_raw_conv`).
    pub raw_conv: bool,
    /// OS device id (WPD id / `usb:bus:addr:vid:pid`). Switching the camera's USB mode re-enumerates it under a new id,
    /// so `camera_status` uses this to notice a mode switch it didn't see as a disconnect.
    pub device_id: String,
}

/// What RAW CONV mode advertises (X-Pro3, pid 02e4). Card-reader mode also lists
/// 0x900C/0x900D, so the RAF upload image format 0xB103 is the deciding part.
/// NOTE: RAW CONV mode does NOT list 0xD183/0xD185 in its property list (verified
/// 2026-11-18: conversion works while they are absent), so don't require them.
fn looks_raw_conv(di: &fuji_ptp::data::DeviceInfo) -> bool {
    use fuji_ptp::codes::{format, op};
    di.image_formats.contains(&format::FUJI_RAF_RAWCONV)
        && di.operations.contains(&op::FUJI_SEND_OBJECT_INFO)
        && di.operations.contains(&op::FUJI_SEND_OBJECT)
}

/// Errors that mean "this camera can't do RAW conversion in its current USB mode":
/// the upload / conversion ops or properties are refused as unsupported.
pub fn is_wrong_mode_error(e: &anyhow::Error) -> bool {
    let s = format!("{e:#}");
    ["(0x2005)", "(0x200A)", "(0x200B)", "Fuji SendObjectInfo (0x900C) failed"].iter().any(|k| s.contains(k))
}

/// Prefix the frontend recognises to show the connection-mode guide instead of a raw error.
pub const WRONG_MODE: &str = "WRONG_MODE";

#[derive(Serialize, Clone)]
pub struct PreviewResult {
    pub jpg: String,
    pub ms: u64,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    index: usize,
    total: usize,
    file: String,
    stage: &'static str,
    pct: u32,
}

#[derive(Serialize, Clone)]
struct ItemEvent {
    index: usize,
    total: usize,
    file: String,
    out: Option<String>,
    error: Option<String>,
    ms: u64,
}

#[derive(Serialize, Clone)]
struct FinishedEvent {
    ok: usize,
    failed: usize,
    cancelled: bool,
    elapsed_ms: u64,
    /// stopped early because the camera turned out to be in the wrong USB mode
    wrong_mode: bool,
}

type Reply<T> = Sender<Result<T, String>>;

pub enum Job {
    Probe(Reply<CamInfo>),
    Preview { path: PathBuf, recipe: Recipe, out_dir: PathBuf, reply: Reply<PreviewResult> },
    /// (photo, the recipe to render it with)
    Render { items: Vec<(PathBuf, Recipe)>, out_dir: PathBuf, app: AppHandle },
    CameraPresets(Reply<Vec<CameraPresetView>>),
    WritePreset { slot: u8, name: String, recipe: Recipe, backup_dir: PathBuf, reply: Reply<WriteResult> },
    UndoPreset { slot: u8, reply: Reply<WriteResult> },
}

#[derive(Serialize, Clone)]
pub struct CameraPresetView {
    pub slot: u8,
    pub name: String,
    pub recipe: Recipe,
}

#[derive(Serialize, Clone)]
pub struct WriteResult {
    pub preset: CameraPresetView,
    /// number of property writes (+1 if the name changed)
    pub changed: usize,
    pub problems: Vec<String>,
    /// full C1–C7 backup written before the first change this session
    pub backup: Option<String>,
    /// an undo snapshot exists for this slot
    pub can_undo: bool,
}

fn view(p: &CameraPreset) -> Result<CameraPresetView> {
    Ok(CameraPresetView { slot: p.slot, name: p.name.trim().to_string(), recipe: fuji_ptp::presets::to_recipe(p)? })
}

pub struct Worker {
    tx: Mutex<Sender<Job>>,
    pub busy: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    pub info: Arc<Mutex<Option<CamInfo>>>,
}

impl Worker {
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<Job>();
        let busy = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let info = Arc::new(Mutex::new(None));
        let (b, c, i) = (busy.clone(), cancel.clone(), info.clone());
        std::thread::Builder::new()
            .name("camera".into())
            .spawn(move || {
                platform::com_init();
                let mut st = State {
                    undo: Default::default(),
                    backed_up: Vec::new(),
                    cam: None,
                    loaded: None,
                    info: i,
                    preview_seq: 0,
                };
                for job in rx {
                    b.store(true, Ordering::SeqCst);
                    match job {
                        Job::Probe(reply) => {
                            let _ = reply.send(st.probe().map_err(|e| format!("{e:#}")));
                        }
                        Job::Preview { path, recipe, out_dir, reply } => {
                            let r = st.preview(&path, &recipe, &out_dir);
                            let r = r.map_err(|e| st.after_error(e));
                            let _ = reply.send(r);
                        }
                        Job::Render { items, out_dir, app } => {
                            c.store(false, Ordering::SeqCst);
                            st.render(&items, &out_dir, &app, &c);
                        }
                        Job::CameraPresets(reply) => {
                            let r = st.camera_presets();
                            let r = r.map_err(|e| st.after_error(e));
                            let _ = reply.send(r);
                        }
                        Job::WritePreset { slot, name, recipe, backup_dir, reply } => {
                            let r = st.write_preset(slot, &name, &recipe, &backup_dir);
                            if r.is_err() {
                                st.reset();
                            }
                            let _ = reply.send(r.map_err(|e| format!("{e:#}")));
                        }
                        Job::UndoPreset { slot, reply } => {
                            let r = st.undo_preset(slot);
                            if r.is_err() {
                                st.reset();
                            }
                            let _ = reply.send(r.map_err(|e| format!("{e:#}")));
                        }
                    }
                    b.store(false, Ordering::SeqCst);
                }
            })
            .expect("spawn camera thread");
        Self { tx: Mutex::new(tx), busy, cancel, info }
    }

    pub fn send(&self, job: Job) -> Result<()> {
        self.tx.lock().unwrap().send(job).map_err(|_| anyhow!("camera worker stopped"))
    }
}

struct State {
    /// slot -> state before the most recent write this session
    undo: std::collections::HashMap<u8, CameraPreset>,
    /// cameras (device folder names) already backed up this session
    backed_up: Vec<String>,
    cam: Option<FujiCamera<CameraTransport>>,
    /// RAF currently loaded in camera memory + its base profile (skip re-upload for previews)
    loaded: Option<(PathBuf, Profile)>,
    info: Arc<Mutex<Option<CamInfo>>>,
    preview_seq: u64,
}

/// Short name for output files: "<raw name>_<tag>.jpg".
fn file_tag(recipe: &Recipe) -> String {
    let t = if recipe.name.trim().is_empty() {
        recipe.film.clone().unwrap_or_else(|| "asshot".into())
    } else {
        recipe.name.clone()
    };
    t.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

fn open_camera() -> Result<(FujiCamera<CameraTransport>, String)> {
    let dev = platform::list_devices()?
        .into_iter()
        .find(|d| d.is_fujifilm())
        .ok_or_else(|| anyhow!("未检测到富士相机"))?;
    let mut cam = FujiCamera::new(CameraTransport::open(&dev)?);
    cam.open_session()?;
    Ok((cam, dev.id))
}

impl State {
    fn reset(&mut self) {
        self.cam = None;
        self.loaded = None;
        *self.info.lock().unwrap() = None;
    }

    /// After a failed camera job. A "not supported" answer means the camera is connected
    /// but in the wrong USB mode: keep it listed, flag raw_conv=false (UI shows the mode
    /// guide) and tag the error. Anything else: drop the connection so it is re-probed.
    fn after_error(&mut self, e: anyhow::Error) -> String {
        self.cam = None;
        self.loaded = None;
        if is_wrong_mode_error(&e) {
            if let Some(i) = self.info.lock().unwrap().as_mut() {
                i.raw_conv = false;
            }
            format!("{WRONG_MODE}: {e:#}")
        } else {
            *self.info.lock().unwrap() = None;
            format!("{e:#}")
        }
    }

    fn cam(&mut self) -> Result<&mut FujiCamera<CameraTransport>> {
        if self.cam.is_none() {
            let (mut cam, device_id) = open_camera()?;
            let di = cam.device_info()?;
            *self.info.lock().unwrap() = Some(CamInfo {
                model: di.model.clone(),
                firmware: di.device_version.clone(),
                serial: di.serial_number.clone(),
                raw_conv: looks_raw_conv(&di),
                device_id,
            });
            self.cam = Some(cam);
            self.loaded = None;
        }
        Ok(self.cam.as_mut().unwrap())
    }

    fn probe(&mut self) -> Result<CamInfo> {
        // Always re-open so a replugged / mode-switched camera is detected.
        self.reset();
        let r = self.cam().map(|_| ());
        if let Err(e) = r {
            self.reset();
            return Err(e);
        }
        // DeviceInfo alone is not enough: in at least one other USB mode the X-Pro3
        // advertises exactly what RAW CONV mode does, and only fails once you actually
        // use it (the UI then flipped green → yellow). So really read something the
        // conversion mode serves — the custom settings C1–C7 (read-only; the selected
        // slot is restored). A "not supported" answer means wrong mode; other errors
        // (busy, …) don't decide anything.
        // While verifying, keep the info unpublished so the UI keeps showing
        // "detecting…" instead of flashing green and then flipping to yellow.
        let mut info = self.info.lock().unwrap().take().expect("cam() sets info");
        if info.raw_conv {
            if let Err(e) = self.cam()?.read_presets() {
                if is_wrong_mode_error(&e) {
                    eprintln!("probe: DeviceInfo looks like RAW CONV but presets were refused ({e:#}) -> wrong mode");
                    info.raw_conv = false;
                }
            }
        }
        *self.info.lock().unwrap() = Some(info.clone());
        Ok(info)
    }

    fn model(&self) -> String {
        self.info.lock().unwrap().as_ref().map(|i| i.model.clone()).unwrap_or_default()
    }

    fn check_model(&self, path: &Path) -> Result<()> {
        let h = raf::read_header(path)?;
        let m = self.model();
        if !m.is_empty() && !h.model.eq_ignore_ascii_case(&m) {
            bail!("这张照片由 {} 拍摄，相机是 {m}，只能转换同型号的 RAW", h.model);
        }
        Ok(())
    }

    fn preview(&mut self, path: &Path, recipe: &Recipe, out_dir: &Path) -> Result<PreviewResult> {
        let t = Instant::now();
        self.cam()?;
        self.check_model(path)?;
        let need_upload = self.loaded.as_ref().map(|(p, _)| p != path).unwrap_or(true);
        if need_upload {
            let bytes = std::fs::read(path)?;
            let cam = self.cam.as_mut().unwrap();
            cam.send_raf(&bytes, None).context("上传 RAW")?;
            let base = cam.get_profile()?;
            self.loaded = Some((path.to_path_buf(), base));
        }
        let mut prof = self.loaded.as_ref().unwrap().1.clone();
        recipe.apply(&mut prof)?;
        let cam = self.cam.as_mut().unwrap();
        let before = cam.object_handles(0xFFFF_FFFF, 0, 0).unwrap_or_default();
        cam.set_profile(&prof)?;
        cam.start_conversion(RenderQuality::Preview)?;
        let h = cam.wait_for_new_object(&before, Duration::from_secs(60))?;
        let jpg = cam.get_object(h, None)?;
        let _ = cam.delete_object(h);
        std::fs::create_dir_all(out_dir)?;
        self.preview_seq += 1;
        let out = out_dir.join(format!("preview_{}.jpg", self.preview_seq % 8));
        std::fs::write(&out, jpg)?;
        Ok(PreviewResult { jpg: out.to_string_lossy().into(), ms: t.elapsed().as_millis() as u64 })
    }

    fn render(&mut self, items: &[(PathBuf, Recipe)], out_dir: &Path, app: &AppHandle, cancel: &AtomicBool) {
        let start = Instant::now();
        let total = items.len();
        let (mut ok, mut failed) = (0, 0);
        let mut cancelled = false;
        let mut wrong_mode = false;
        for (index, (path, recipe)) in items.iter().enumerate() {
            let tag = file_tag(recipe);
            if cancel.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            let t = Instant::now();
            let file = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
            let result = self.render_one(path, recipe, out_dir, &tag, |stage, pct| {
                let _ = app.emit(
                    "render-progress",
                    ProgressEvent { index, total, file: file.clone(), stage, pct },
                );
            });
            let ev = match result {
                Ok(out) => {
                    ok += 1;
                    ItemEvent { index, total, file, out: Some(out.to_string_lossy().into()), error: None, ms: t.elapsed().as_millis() as u64 }
                }
                Err(e) => {
                    failed += 1;
                    self.cam = None; // reopen on next item
                    self.loaded = None;
                    if is_wrong_mode_error(&e) {
                        wrong_mode = true;
                        if let Some(i) = self.info.lock().unwrap().as_mut() {
                            i.raw_conv = false;
                        }
                    }
                    ItemEvent { index, total, file, out: None, error: Some(format!("{e:#}")), ms: t.elapsed().as_millis() as u64 }
                }
            };
            let _ = app.emit("render-item", ev);
            if wrong_mode {
                break; // every remaining photo would fail the same way
            }
        }
        let _ = app.emit(
            "render-finished",
            FinishedEvent { ok, failed, cancelled, elapsed_ms: start.elapsed().as_millis() as u64, wrong_mode },
        );
    }

    /// Custom settings C1–C7 stored in the camera, translated to recipes.
    fn camera_presets(&mut self) -> Result<Vec<CameraPresetView>> {
        let presets = self.cam()?.read_presets()?;
        presets.iter().map(view).collect()
    }

    /// Write an edited recipe (+ name) into camera slot `slot`.
    /// Before the first write of the session for this camera, all 7 slots are backed up
    /// to `backup_dir`; the slot's previous state is kept in memory for undo.
    fn write_preset(&mut self, slot: u8, name: &str, recipe: &Recipe, backup_dir: &Path) -> Result<WriteResult> {
        fuji_ptp::presets::validate_name(name)?;
        let all = self.cam()?.read_presets()?;
        let base = all.iter().find(|p| p.slot == slot).ok_or_else(|| anyhow!("相机里没有 C{slot}"))?.clone();
        let props = fuji_ptp::presets::encode_changes(recipe, &base)?;
        let name_change = (name != base.name).then_some(name);
        if props.is_empty() && name_change.is_none() {
            return Ok(WriteResult { preset: view(&base)?, changed: 0, problems: vec![], backup: None, can_undo: self.undo.contains_key(&slot) });
        }
        let device = self.device_folder();
        let backup = if self.backed_up.contains(&device) {
            None
        } else {
            std::fs::create_dir_all(backup_dir)?;
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let stamp = unix_date(secs).1.replace([' ', ':'], "-");
            let p = backup_dir.join(format!("{device}_C1-C7_{stamp}.json"));
            std::fs::write(&p, serde_json::to_string_pretty(&all)?)?;
            self.backed_up.push(device);
            Some(p.to_string_lossy().to_string())
        };
        self.undo.insert(slot, base);
        let (back, problems) = self.cam()?.write_preset(slot, name_change, &props)?;
        Ok(WriteResult {
            preset: view(&back)?,
            changed: props.len() + name_change.is_some() as usize,
            problems,
            backup,
            can_undo: true,
        })
    }

    /// Put slot `slot` back to how it was before the last write this session.
    fn undo_preset(&mut self, slot: u8) -> Result<WriteResult> {
        let before = self.undo.get(&slot).cloned().ok_or_else(|| anyhow!("C{slot} 没有可以撤销的修改"))?;
        let current = self.cam()?.read_presets()?.into_iter().find(|p| p.slot == slot).ok_or_else(|| anyhow!("相机里没有 C{slot}"))?;
        // only write what actually differs (mono toning props reject writes on colour sims)
        let props: Vec<(u16, Vec<u8>)> = before
            .values
            .iter()
            .filter(|(id, v)| current.values.iter().find(|(i, _)| i == id).map(|(_, c)| c != v).unwrap_or(false))
            .cloned()
            .collect();
        let name_change = (before.name != current.name).then_some(before.name.as_str());
        let (back, problems) = self.cam()?.write_preset(slot, name_change, &props)?;
        self.undo.remove(&slot);
        Ok(WriteResult { preset: view(&back)?, changed: props.len() + name_change.is_some() as usize, problems, backup: None, can_undo: false })
    }

    /// Folder name for this camera inside the cache: "<model>-<serial tail>".
    fn device_folder(&self) -> String {
        let info = self.info.lock().unwrap().clone();
        match info {
            Some(i) => {
                let tail: String = i.serial.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
                format!("{}-{}", i.model, tail)
            }
            None => "camera".into(),
        }
    }

    fn render_one(
        &mut self,
        path: &Path,
        recipe: &Recipe,
        out_dir: &Path,
        tag: &str,
        mut progress: impl FnMut(&'static str, u32),
    ) -> Result<PathBuf> {
        self.cam()?;
        self.check_model(path)?;
        let bytes = std::fs::read(path)?;
        let mut last = (u32::MAX, "");
        let mut on_stage = |s: Stage| {
            let (stage, pct) = match s {
                Stage::Uploading { done, total } => ("upload", (done * 100 / total.max(1)) as u32),
                Stage::ReadingProfile | Stage::WritingProfile => ("profile", 0),
                Stage::Converting => ("convert", 0),
                Stage::Downloading { done, total } => ("download", (done * 100 / total.max(1)) as u32),
            };
            if (pct, stage) != last {
                last = (pct, stage);
                progress(stage, pct);
            }
        };
        let cam = self.cam.as_mut().unwrap();
        let r = cam.convert(&bytes, Some(recipe), RenderQuality::Full, &mut on_stage)?;
        self.loaded = Some((path.to_path_buf(), r.base_profile.clone()));
        std::fs::create_dir_all(out_dir)?;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let mut out = out_dir.join(format!("{stem}_{tag}.jpg"));
        let mut i = 1;
        while out.exists() {
            out = out_dir.join(format!("{stem}_{tag}-{i}.jpg"));
            i += 1;
        }
        std::fs::write(&out, &r.jpeg)?;
        Ok(out)
    }
}
