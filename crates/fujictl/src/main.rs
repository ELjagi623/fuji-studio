//! fujictl — feasibility probe for Fujifilm in-camera RAW conversion.

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use fuji_ptp::codes::{op_name, prop, prop_name, resp_name};
use fuji_ptp::data::hex;
use fuji_ptp::profile::field_name;
use fuji_ptp::recipe::film_name;
use fuji_ptp::{DataPhase, FujiCamera, Recipe, RenderQuality, Stage, Transport};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "fujictl", about = "Fujifilm in-camera RAW conversion probe")]
struct Cli {
    /// Print every PTP transaction
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Pick a device whose name/id contains this text (default: first Fujifilm device)
    #[arg(short, long, global = true)]
    device: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List portable (WPD) devices seen by Windows
    Devices,
    /// Show PTP DeviceInfo and check RAW-conversion support
    Info,
    /// Read every vendor property (0xD000+) and dump raw values
    Props {
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// List objects on the camera storage (tests card access in current USB mode)
    Ls {
        #[arg(long)]
        raf_only: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Download an object by handle (hex or decimal)
    Pull {
        handle: String,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Upload a RAF and dump the camera's D185 conversion profile
    Profile {
        raf: PathBuf,
        #[arg(long)]
        save: Option<PathBuf>,
    },
    /// Convert RAF files (files or folders) in-camera and save JPEGs locally
    Convert {
        inputs: Vec<PathBuf>,
        #[arg(short, long)]
        recipe: Option<PathBuf>,
        /// Film simulation override, e.g. classic-chrome
        #[arg(short, long)]
        film: Option<String>,
        #[arg(short, long, default_value = "out")]
        out: PathBuf,
        /// Also save base/applied D185 profiles next to each JPEG
        #[arg(long)]
        save_profile: bool,
        /// Convert even if the RAF was shot on a different camera model
        #[arg(long)]
        force: bool,
        /// Quick low-res preview render (S size) instead of full resolution
        #[arg(long)]
        preview: bool,
        /// Raw D185 param override for experiments, e.g. --set 2=7 (repeatable)
        #[arg(long = "set")]
        sets: Vec<String>,
    },
    /// Offline: show RAF header info and optionally extract embedded previews
    RafInfo {
        inputs: Vec<PathBuf>,
        #[arg(long)]
        thumbs: Option<PathBuf>,
    },
    /// Dump the in-camera custom settings C1–C7 (raw property values)
    Presets {
        /// Save all slots (raw values) to this JSON file
        #[arg(long)]
        backup: Option<PathBuf>,
    },
    /// Write a custom-setting slot: --name and/or --prop d19e=-10 (i16, repeatable)
    PresetWrite {
        slot: u8,
        #[arg(long)]
        name: Option<String>,
        #[arg(long = "prop")]
        props: Vec<String>,
    },
    /// Restore one slot (name + all props) from a `presets --backup` file
    PresetRestore { backup: PathBuf, slot: u8 },
    /// Send an arbitrary PTP operation, e.g. `raw 1015 d185 --read`
    Raw {
        op: String,
        params: Vec<String>,
        #[arg(long)]
        read: bool,
        /// hex bytes to send as data phase
        #[arg(long)]
        write: Option<String>,
    },
}

fn parse_num(s: &str) -> Result<u32> {
    let t = s.trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(t, 16).with_context(|| format!("bad hex number '{s}'"))
}

#[cfg(windows)]
fn open_camera(cli: &Cli) -> Result<FujiCamera<fuji_ptp::wpd::WpdTransport>> {
    use fuji_ptp::wpd;
    let devs = wpd::list_devices()?;
    let pick = match &cli.device {
        Some(q) => {
            let q = q.to_ascii_lowercase();
            devs.iter().find(|d| {
                d.friendly_name.to_ascii_lowercase().contains(&q) || d.id.to_ascii_lowercase().contains(&q)
            })
        }
        None => devs.iter().find(|d| d.is_fujifilm()),
    };
    let Some(d) = pick else {
        bail!(
            "no Fujifilm camera found ({} portable devices). Turn the camera on, set \
             CONNECTION SETTING > PC CONNECTION MODE > USB RAW CONV./BACKUP RESTORE, and use a data cable.",
            devs.len()
        );
    };
    let t = wpd::WpdTransport::open(d)?;
    eprintln!("connected: {}", t.describe());
    let mut cam = FujiCamera::new(t);
    cam.verbose = cli.verbose;
    cam.open_session()?;
    Ok(cam)
}

#[cfg(not(windows))]
fn open_camera(_cli: &Cli) -> Result<FujiCamera<NoTransport>> {
    bail!("only the Windows WPD transport is implemented so far")
}

#[cfg(not(windows))]
struct NoTransport;
#[cfg(not(windows))]
impl Transport for NoTransport {
    fn describe(&self) -> String {
        String::new()
    }
    fn driver_manages_session(&self) -> bool {
        false
    }
    fn execute(&mut self, _: u16, _: &[u32], _: DataPhase<'_>, _: fuji_ptp::transport::Progress<'_>) -> Result<fuji_ptp::Response> {
        unreachable!()
    }
}

fn expand_inputs(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in inputs {
        if p.is_dir() {
            out.extend(raf::scan_dir(p, true)?);
        } else if raf::has_raf_extension(p) {
            out.push(p.clone());
        } else {
            eprintln!("skip (not .RAF): {}", p.display());
        }
    }
    if out.is_empty() {
        bail!("no RAF files in inputs");
    }
    Ok(out)
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut i = 1;
    while p.exists() {
        p = dir.join(format!("{stem}-{i}.{ext}"));
        i += 1;
    }
    p
}

fn fmt_dur(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s < 60.0 {
        format!("{s:.1}s")
    } else {
        format!("{}m{:02}s", (s / 60.0) as u64, (s % 60.0) as u64)
    }
}

fn dump_profile(p: &fuji_ptp::Profile) {
    println!("D185 profile: {} bytes, {} params (offset {})", p.raw.len(), p.num_params(), p.params_offset().unwrap_or(0));
    let off = p.params_offset().unwrap_or(0);
    println!("header bytes [0..{off}]:");
    for (i, chunk) in p.raw[..off].chunks(32).enumerate() {
        println!("  {:04x}: {}", i * 32, hex(chunk));
    }
    for (i, v) in p.params().iter().enumerate() {
        let name = field_name(i).unwrap_or("");
        let extra = if i == fuji_ptp::profile::idx::FILM_SIMULATION {
            film_name(*v).map(|n| format!(" ({n})")).unwrap_or_default()
        } else {
            String::new()
        };
        println!("  [{i:2}] {v:>8}  0x{:08X}  {name}{extra}", *v as u32);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Devices => {
            #[cfg(windows)]
            {
                let devs = fuji_ptp::wpd::list_devices()?;
                if devs.is_empty() {
                    println!("no portable devices");
                }
                for d in devs {
                    println!(
                        "{} {} | {} | {}\n    {}",
                        if d.is_fujifilm() { "*" } else { " " },
                        d.friendly_name,
                        d.manufacturer,
                        d.description,
                        d.id
                    );
                }
            }
        }
        Cmd::Info => {
            let mut cam = open_camera(&cli)?;
            let di = cam.device_info()?;
            println!("{} {}  fw {}  serial {}", di.manufacturer, di.model, di.device_version, di.serial_number);
            println!("vendor ext 0x{:08X} v{} '{}'  functional mode 0x{:04X}", di.vendor_extension_id, di.vendor_extension_version, di.vendor_extension_desc, di.functional_mode);
            println!("operations ({}):", di.operations.len());
            for o in &di.operations {
                println!("  0x{o:04X} {}", op_name(*o).unwrap_or(""));
            }
            println!("events: {:04X?}", di.events);
            println!("properties ({}):", di.properties.len());
            for p in &di.properties {
                println!("  0x{p:04X} {}", prop_name(*p).unwrap_or(""));
            }
            println!("image formats: {:04X?}", di.image_formats);
            println!("\nRAW conversion support check:");
            for (name, ok) in [
                ("RAW CONV mode (image format 0xB103)", di.image_formats.contains(&fuji_ptp::codes::format::FUJI_RAF_RAWCONV)),
                ("op 0x900C Fuji SendObjectInfo", di.operations.contains(&0x900C)),
                ("op 0x900D Fuji SendObject2", di.operations.contains(&0x900D)),
                ("prop 0xD183 StartRawConversion", di.properties.contains(&prop::START_RAW_CONVERSION)),
                ("prop 0xD185 RawConvProfile", di.properties.contains(&prop::RAW_CONV_PROFILE)),
            ] {
                println!("  [{}] {name}", if ok { "x" } else { " " });
            }
        }
        Cmd::Props { out } => {
            let mut cam = open_camera(&cli)?;
            let di = cam.device_info()?;
            let mut rows = Vec::new();
            for p in di.properties.iter().filter(|p| **p >= 0xD000) {
                if *p == prop::RAW_CONV_PROFILE {
                    continue;
                }
                match cam.get_prop(*p) {
                    Ok(v) => {
                        println!("0x{p:04X} {:<20} [{}] {}", prop_name(*p).unwrap_or(""), v.len(), hex(&v[..v.len().min(48)]));
                        rows.push(serde_json::json!({"id": format!("0x{p:04X}"), "hex": hex(&v)}));
                    }
                    Err(e) => println!("0x{p:04X} error: {e:#}"),
                }
            }
            if let Some(o) = out {
                std::fs::write(o, serde_json::to_string_pretty(&rows)?)?;
                println!("saved {}", o.display());
            }
        }
        Cmd::Ls { raf_only, limit } => {
            let mut cam = open_camera(&cli)?;
            match cam.storage_ids() {
                Ok(ids) => println!("storages: {ids:08X?}"),
                Err(e) => println!("GetStorageIDs: {e:#}"),
            }
            let hs = cam.object_handles(0xFFFF_FFFF, 0, 0)?;
            println!("{} objects", hs.len());
            let mut shown = 0;
            for h in hs {
                if shown >= *limit {
                    println!("... (use --limit)");
                    break;
                }
                let oi = match cam.object_info(h) {
                    Ok(o) => o,
                    Err(e) => {
                        println!("0x{h:08X} info error: {e:#}");
                        continue;
                    }
                };
                let is_raf = oi.filename.to_ascii_uppercase().ends_with(".RAF");
                if *raf_only && !is_raf {
                    continue;
                }
                shown += 1;
                println!(
                    "0x{h:08X} fmt 0x{:04X} {:>11} B  {}  {}",
                    oi.format, oi.compressed_size, oi.capture_date, oi.filename
                );
            }
        }
        Cmd::Pull { handle, out } => {
            let h = parse_num(handle)?;
            let mut cam = open_camera(&cli)?;
            let oi = cam.object_info(h)?;
            let mut cb = |d: u64, n: u64| eprint!("\r  {d}/{n} bytes");
            let data = cam.get_object(h, Some(&mut cb))?;
            eprintln!();
            let path = out.clone().unwrap_or_else(|| PathBuf::from(&oi.filename));
            std::fs::write(&path, data)?;
            println!("saved {}", path.display());
        }
        Cmd::Profile { raf, save } => {
            let bytes = std::fs::read(raf)?;
            let mut cam = open_camera(&cli)?;
            let t = Instant::now();
            let mut cb = |d: u64, n: u64| eprint!("\r  upload {:.0}%", d as f64 * 100.0 / n.max(1) as f64);
            cam.send_raf(&bytes, Some(&mut cb))?;
            eprintln!("\r  upload done in {}", fmt_dur(t.elapsed()));
            let p = cam.get_profile()?;
            dump_profile(&p);
            if let Some(s) = save {
                std::fs::write(s, &p.raw)?;
                println!("saved {}", s.display());
            }
        }
        Cmd::Convert { inputs, recipe, film, out, save_profile, force, preview, sets } => {
            let force = *force;
            let quality = if *preview { RenderQuality::Preview } else { RenderQuality::Full };
            let files = expand_inputs(inputs)?;
            let mut rec = match recipe {
                Some(p) => Recipe::load(p).with_context(|| format!("load recipe {}", p.display()))?,
                None => Recipe::default(),
            };
            if let Some(f) = film {
                rec.film = Some(f.clone());
            }
            for s in sets {
                let (i, v) = s.split_once('=').context("--set expects idx=value")?;
                rec.raw.push((i.trim().parse()?, v.trim().parse()?));
            }
            let tag = if !rec.name.is_empty() {
                rec.name.clone()
            } else {
                rec.film.clone().unwrap_or_else(|| "asshot".into())
            };
            let tag: String = tag.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
            std::fs::create_dir_all(out)?;
            let mut cam = open_camera(&cli)?;
            // The camera can only develop RAFs shot on the same model.
            let model = cam.device_info().map(|d| d.model).unwrap_or_default();
            let files: Vec<PathBuf> = files
                .into_iter()
                .filter(|f| match raf::read_header(f) {
                    Ok(h) if !model.is_empty() && !h.model.eq_ignore_ascii_case(&model) && !force => {
                        println!("skip {} (shot on {}, camera is {model}; use --force to try anyway)", f.display(), h.model);
                        false
                    }
                    _ => true,
                })
                .collect();
            let start = Instant::now();
            let n = files.len();
            let mut failures = 0;
            for (i, f) in files.iter().enumerate() {
                let stem = f.file_stem().unwrap().to_string_lossy().to_string();
                println!("[{}/{n}] {}", i + 1, f.display());
                let bytes = std::fs::read(f)?;
                let t = Instant::now();
                let mut on_stage = |s: Stage| {
                    match s {
                        Stage::Uploading { done, total } => eprint!("\r  upload   {:5.1}%", done as f64 * 100.0 / total.max(1) as f64),
                        Stage::ReadingProfile => eprint!("\r  profile...       "),
                        Stage::WritingProfile => {}
                        Stage::Converting => eprint!("\r  converting...    "),
                        Stage::Downloading { done, total } => eprint!("\r  download {:5.1}%", done as f64 * 100.0 / total.max(1) as f64),
                    }
                    let _ = std::io::stderr().flush();
                };
                match cam.convert(&bytes, Some(&rec), quality, &mut on_stage) {
                    Ok(r) => {
                        eprint!("\r                    \r");
                        let path = unique_path(out, &format!("{stem}_{tag}"), "jpg");
                        std::fs::write(&path, &r.jpeg)?;
                        if *save_profile {
                            std::fs::write(path.with_extension("base.d185"), &r.base_profile.raw)?;
                            std::fs::write(path.with_extension("applied.d185"), &r.applied_profile.raw)?;
                        }
                        let tm = &r.timings;
                        println!(
                            "  -> {} ({:.1} MB)  upload {} | profile {} | convert {} | download {} | total {}",
                            path.display(),
                            r.jpeg.len() as f64 / 1e6,
                            fmt_dur(tm.upload),
                            fmt_dur(tm.profile),
                            fmt_dur(tm.convert),
                            fmt_dur(tm.download),
                            fmt_dur(t.elapsed())
                        );
                        if cli.verbose {
                            for c in &r.changes {
                                println!("     {c}");
                            }
                        }
                    }
                    Err(e) => {
                        failures += 1;
                        eprintln!();
                        println!("  !! failed: {e:#}");
                    }
                }
                let done = i + 1;
                let elapsed = start.elapsed();
                let eta = elapsed.div_f64(done as f64).mul_f64((n - done) as f64);
                println!("  progress {done}/{n}  elapsed {}  remaining ~{}", fmt_dur(elapsed), fmt_dur(eta));
            }
            println!("done: {} ok, {failures} failed, {}", n - failures, fmt_dur(start.elapsed()));
            cam.close_session()?;
        }
        Cmd::RafInfo { inputs, thumbs } => {
            let files = expand_inputs(inputs)?;
            if let Some(d) = thumbs {
                std::fs::create_dir_all(d)?;
            }
            for f in files {
                let h = raf::read_header(&f)?;
                let size = std::fs::metadata(&f)?.len();
                println!(
                    "{}  {}  v{}  {:.1} MB  preview {} B @0x{:X}",
                    f.display(),
                    h.model,
                    h.format_version,
                    size as f64 / 1e6,
                    h.jpeg_length,
                    h.jpeg_offset
                );
                if let Some(d) = thumbs {
                    let t = Instant::now();
                    let jpg = raf::read_embedded_jpeg(&f)?;
                    let p = d.join(format!("{}.jpg", f.file_stem().unwrap().to_string_lossy()));
                    std::fs::write(&p, jpg)?;
                    println!("    preview -> {} ({:?})", p.display(), t.elapsed());
                }
            }
        }
        Cmd::PresetWrite { slot, name, props } => {
            let mut cam = open_camera(&cli)?;
            let mut list = Vec::new();
            for p in props {
                let (id, v) = p.split_once('=').context("--prop expects id=value")?;
                list.push((parse_num(id)? as u16, (v.parse::<i32>()? as i16).to_le_bytes().to_vec()));
            }
            let (back, problems) = cam.write_preset(*slot, name.as_deref(), &list)?;
            println!("C{} now \"{}\"", back.slot, back.name);
            for (id, v) in &list {
                println!("   0x{id:04X} {:<22} wrote {}", fuji_ptp::presets::prop_label(*id), hex(v));
            }
            if problems.is_empty() {
                println!("verified OK");
            }
            for p in problems {
                println!("   !! {p}");
            }
        }
        Cmd::PresetRestore { backup, slot } => {
            let all: Vec<fuji_ptp::presets::CameraPreset> = serde_json::from_str(&std::fs::read_to_string(backup)?)?;
            let p = all.iter().find(|p| p.slot == *slot).context("slot not in backup")?;
            let mut cam = open_camera(&cli)?;
            let (back, problems) = cam.write_preset(*slot, Some(&p.name), &p.values)?;
            println!("C{} restored to \"{}\" ({} props)", back.slot, back.name, p.values.len());
            for x in problems {
                println!("   !! {x}");
            }
        }
        Cmd::Presets { backup } => {
            let mut cam = open_camera(&cli)?;
            let presets = cam.read_presets()?;
            if let Some(b) = backup {
                std::fs::write(b, serde_json::to_string_pretty(&presets)?)?;
                println!("backup saved to {}", b.display());
            }
            for p in &presets {
                println!("C{} \"{}\"", p.slot, p.name);
                for (id, v) in &p.values {
                    println!("   0x{id:04X} {:<22} {}", fuji_ptp::presets::prop_label(*id), hex(v));
                }
                match fuji_ptp::presets::to_recipe(p) {
                    Ok(r) => println!("   => {}", serde_json::to_string(&r)?),
                    Err(e) => println!("   => decode error: {e:#}"),
                }
            }
        }
        Cmd::Raw { op, params, read, write } => {
            let opc = parse_num(op)? as u16;
            let ps: Vec<u32> = params.iter().map(|p| parse_num(p)).collect::<Result<_>>()?;
            let wbytes = match write {
                Some(h) => Some(
                    h.split_whitespace()
                        .map(|b| u8::from_str_radix(b, 16))
                        .collect::<Result<Vec<u8>, _>>()?,
                ),
                None => None,
            };
            let mut cam = open_camera(&cli)?;
            let phase = match (&wbytes, read) {
                (Some(b), _) => DataPhase::Out(b),
                (None, true) => DataPhase::In,
                _ => DataPhase::None,
            };
            let r = cam.call(opc, &ps, phase, None)?;
            println!("response {} params {:08X?}", resp_name(r.code), r.params);
            if !r.data.is_empty() {
                println!("data {} bytes:", r.data.len());
                for (i, c) in r.data.chunks(32).enumerate().take(64) {
                    println!("  {:04x}: {}", i * 32, hex(c));
                }
            }
        }
    }
    Ok(())
}
