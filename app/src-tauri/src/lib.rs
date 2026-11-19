mod recipes;
mod settings;
mod thumbs;
mod worker;

use settings::{DirNode, Settings};
use fuji_ptp::Recipe;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::sync::Mutex;
use tauri::{Manager, State};
use worker::{CamInfo, Job, PreviewResult, Worker};

struct AppState {
    worker: Worker,
    cache_dir: PathBuf,
    recipes_dir: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
}

async fn ask<T: Send + 'static>(state: &AppState, job: impl FnOnce(std::sync::mpsc::Sender<Result<T, String>>) -> Job) -> CmdResult<T> {
    let (tx, rx) = channel();
    state.worker.send(job(tx)).map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || rx.recv()).await.map_err(err)?.map_err(err)?
}

// ---------------- settings & folder browser ----------------

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
    *s = settings;
    s.save(&state.settings_path).map_err(err)
}

#[tauri::command]
async fn fs_roots(app: tauri::AppHandle) -> CmdResult<Vec<DirNode>> {
    let p = app.path();
    let places = vec![
        ("图片", p.picture_dir().ok()),
        ("桌面", p.desktop_dir().ok()),
        ("下载", p.download_dir().ok()),
        ("主目录", p.home_dir().ok()),
    ];
    blocking(move || Ok(settings::roots(&places))).await
}

#[tauri::command]
async fn list_dirs(path: String) -> CmdResult<Vec<DirNode>> {
    blocking(move || settings::list_dirs(&PathBuf::from(path))).await
}

#[tauri::command]
async fn camera_presets(state: State<'_, AppState>) -> CmdResult<Vec<worker::CameraPresetView>> {
    // Queued behind whatever the camera is doing (the worker is serial), never rejected.
    ask(&state, Job::CameraPresets).await
}

#[tauri::command]
async fn write_camera_preset(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    slot: u8,
    name: String,
    recipe: Recipe,
) -> CmdResult<worker::WriteResult> {
    let backup_dir = app.path().app_local_data_dir().map_err(err)?.join("camera-backups");
    ask(&state, move |reply| Job::WritePreset { slot, name, recipe, backup_dir, reply }).await
}

#[tauri::command]
async fn undo_camera_preset(state: State<'_, AppState>, slot: u8) -> CmdResult<worker::WriteResult> {
    ask(&state, move |reply| Job::UndoPreset { slot, reply }).await
}

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?.map_err(|e| format!("{e:#}"))
}

#[derive(Serialize)]
struct CameraStatus {
    present: bool,
    name: String,
    busy: bool,
    info: Option<CamInfo>,
}

#[tauri::command]
async fn camera_status(state: State<'_, AppState>) -> CmdResult<CameraStatus> {
    let devs = blocking(fuji_ptp::wpd::list_devices).await?;
    let dev = devs.into_iter().find(|d| d.is_fujifilm());
    let info = state.worker.info.lock().unwrap().clone();
    // Unplugged, or re-enumerated under a new id (USB mode switched on the camera):
    // forget what we knew so the frontend probes again.
    let stale = match (&dev, &info) {
        (None, Some(_)) => true,
        (Some(d), Some(i)) => d.id != i.device_id,
        _ => false,
    };
    if stale && !state.worker.busy.load(Ordering::SeqCst) {
        *state.worker.info.lock().unwrap() = None;
    }
    Ok(CameraStatus {
        present: dev.is_some(),
        name: dev.map(|d| d.friendly_name).unwrap_or_default(),
        busy: state.worker.busy.load(Ordering::SeqCst),
        info: if state.worker.info.lock().unwrap().is_some() { info } else { None },
    })
}

#[tauri::command]
async fn probe_camera(state: State<'_, AppState>) -> CmdResult<CamInfo> {
    let (tx, rx) = channel();
    state.worker.send(Job::Probe(tx)).map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(err)?
        .map_err(err)?
}

#[derive(Serialize)]
struct RafEntry {
    path: String,
    name: String,
    model: String,
    size: u64,
    modified: u64,
}

#[tauri::command]
async fn scan_folder(path: String, recursive: bool) -> CmdResult<Vec<RafEntry>> {
    blocking(move || {
        let files = raf::scan_dir(&PathBuf::from(&path), recursive)?;
        Ok(files
            .into_iter()
            .map(|p| {
                let meta = std::fs::metadata(&p).ok();
                RafEntry {
                    name: p.file_name().unwrap_or_default().to_string_lossy().into(),
                    model: raf::read_header(&p).map(|h| h.model).unwrap_or_default(),
                    size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    modified: meta
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    path: p.to_string_lossy().into(),
                }
            })
            .collect())
    })
    .await
}

#[tauri::command]
async fn thumbnail(state: State<'_, AppState>, path: String, size: u32) -> CmdResult<String> {
    let dir = state.cache_dir.join("thumbs");
    blocking(move || Ok(thumbs::thumbnail(&dir, &PathBuf::from(path), size)?.to_string_lossy().into())).await
}

#[tauri::command]
fn list_recipes(state: State<'_, AppState>) -> Vec<recipes::RecipeEntry> {
    recipes::list(&state.recipes_dir)
}

#[tauri::command]
fn save_recipe(state: State<'_, AppState>, recipe: Recipe) -> CmdResult<()> {
    recipes::save(&state.recipes_dir, &recipe).map(|_| ()).map_err(err)
}

#[tauri::command]
fn delete_recipe(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    recipes::delete(&state.recipes_dir, &name).map_err(err)
}

#[tauri::command]
fn film_sims() -> Vec<(String, i32)> {
    fuji_ptp::recipe::FILM_SIMS.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[tauri::command]
async fn preview_render(state: State<'_, AppState>, path: String, recipe: Recipe) -> CmdResult<PreviewResult> {
    let (tx, rx) = channel();
    state
        .worker
        .send(Job::Preview { path: path.into(), recipe, out_dir: state.cache_dir.join("previews"), reply: tx })
        .map_err(err)?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(err)?
        .map_err(err)?
}

#[derive(serde::Deserialize)]
struct RenderItem {
    path: String,
    recipe: Recipe,
}

/// Each photo is rendered with its own recipe (the one batch-applied to it on the Configure page).
#[tauri::command]
fn start_render(app: tauri::AppHandle, state: State<'_, AppState>, items: Vec<RenderItem>, out_dir: String) -> CmdResult<()> {
    if state.worker.busy.load(Ordering::SeqCst) {
        return Err("相机正忙".into());
    }
    let items = items.into_iter().map(|i| (PathBuf::from(i.path), i.recipe)).collect();
    state.worker.send(Job::Render { items, out_dir: out_dir.into(), app }).map_err(err)
}

#[tauri::command]
fn cancel_render(state: State<'_, AppState>) {
    state.worker.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn default_output_dir(app: tauri::AppHandle) -> String {
    app.path()
        .picture_dir()
        .map(|p| p.join("Fuji Studio"))
        .unwrap_or_else(|_| PathBuf::from("Fuji Studio"))
        .to_string_lossy()
        .into()
}

#[tauri::command]
fn reveal(path: String) -> CmdResult<()> {
    // Folder: open it. File: open its folder with the file selected.
    // Windows: explorer doesn't understand a quoted "/select,C:\a b\x.jpg" (which is what
    // Command::arg produces for paths with spaces, e.g. "Pictures\Fuji Studio") and falls
    // back to Documents. So pass the raw command line: /select,"C:\a b\x.jpg".
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        let p = path.replace('/', "\\");
        let arg = if std::path::Path::new(&p).is_dir() { format!("\"{p}\"") } else { format!("/select,\"{p}\"") };
        std::process::Command::new("explorer").raw_arg(arg).spawn()
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").args(["-R", &path]).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&path).spawn();
    r.map(|_| ()).map_err(err)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            let recipes_dir = app.path().app_config_dir()?.join("recipes");
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            std::fs::create_dir_all(&cache_dir)?;
            let settings = Mutex::new(Settings::load(&settings_path));
            app.manage(AppState {
                worker: Worker::spawn(),
                cache_dir,
                recipes_dir,
                settings_path,
                settings,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            camera_status,
            probe_camera,
            scan_folder,
            thumbnail,
            list_recipes,
            save_recipe,
            delete_recipe,
            film_sims,
            preview_render,
            start_render,
            cancel_render,
            default_output_dir,
            reveal,
            get_settings,
            set_settings,
            fs_roots,
            list_dirs,
            camera_presets,
            write_camera_preset,
            undo_camera_preset
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
