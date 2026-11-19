import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import { L } from "./i18n";

/**
 * Blocking yes/no question. Do NOT use window.confirm() in this app: inside the
 * Tauri webview it returns immediately without waiting for the user (verified).
 */
export function confirmAsk(message: string, title = "Fuji Studio"): Promise<boolean> {
  return ask(message, { title, kind: "warning", okLabel: L("确定", "OK"), cancelLabel: L("取消", "Cancel") });
}

export interface CamInfo {
  model: string;
  firmware: string;
  serial: string;
  raw_conv: boolean;
  device_id: string;
}

export interface CameraStatus {
  present: boolean;
  name: string;
  busy: boolean;
  info: CamInfo | null;
}

export interface RafEntry {
  path: string;
  name: string;
  model: string;
  size: number;
  modified: number;
}

/** Mirrors fuji_ptp::Recipe. Unset (undefined/null) = keep the as-shot value. */
export interface Recipe {
  name: string;
  film?: string | null;
  exposure?: number | null;
  dynamic_range?: number | null;
  grain?: string | null;
  color_chrome?: string | null;
  color_chrome_blue?: string | null;
  smooth_skin?: string | null;
  white_balance?: string | null;
  wb_kelvin?: number | null;
  wb_shift_r?: number | null;
  wb_shift_b?: number | null;
  highlight?: number | null;
  shadow?: number | null;
  color?: number | null;
  sharpness?: number | null;
  noise_reduction?: number | null;
  clarity?: number | null;
  raw?: [number, number][];
}

export interface RecipeEntry {
  recipe: Recipe;
  builtin: boolean;
}

export interface PreviewResult {
  jpg: string;
  ms: number;
}

export interface ProgressEvent {
  index: number;
  total: number;
  file: string;
  stage: "upload" | "profile" | "convert" | "download";
  pct: number;
}

export interface ItemEvent {
  index: number;
  total: number;
  file: string;
  out: string | null;
  error: string | null;
  ms: number;
}

export interface FinishedEvent {
  ok: number;
  failed: number;
  cancelled: boolean;
  elapsed_ms: number;
  wrong_mode: boolean;
}

export interface CameraPreset {
  slot: number;
  name: string;
  recipe: Recipe;
}

export interface WriteResult {
  preset: CameraPreset;
  changed: number;
  problems: string[];
  backup: string | null;
  can_undo: boolean;
}

export interface DirNode {
  name: string;
  path: string;
  has_children: boolean;
  kind: "place" | "drive" | "dir";
}

export type ViewMode = "xl" | "large" | "medium" | "small" | "list";
export type SortKey = "name" | "date" | "size" | "model";

export interface Settings {
  last_dir: string | null;
  include_subfolders: boolean;
  view: ViewMode | "";
  sort_by: SortKey | "";
  sort_desc: boolean;
}

export const DEFAULT_SETTINGS: Settings = { last_dir: null, include_subfolders: false, view: "large", sort_by: "name", sort_desc: false };

/** Drop null/undefined fields so serde treats them as "keep as shot". */
export function cleanRecipe(r: Recipe): Recipe {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(r)) {
    if (v !== null && v !== undefined && v !== "") out[k] = v;
  }
  out.name = r.name ?? "";
  return out as unknown as Recipe;
}

export const api = {
  cameraStatus: () => invoke<CameraStatus>("camera_status"),
  probeCamera: () => invoke<CamInfo>("probe_camera"),
  scanFolder: (path: string, recursive = true) => invoke<RafEntry[]>("scan_folder", { path, recursive }),
  thumbnail: (path: string, size = 480) => invoke<string>("thumbnail", { path, size }),
  listRecipes: () => invoke<RecipeEntry[]>("list_recipes"),
  saveRecipe: (recipe: Recipe) => invoke<void>("save_recipe", { recipe: cleanRecipe(recipe) }),
  deleteRecipe: (name: string) => invoke<void>("delete_recipe", { name }),
  filmSims: () => invoke<[string, number][]>("film_sims"),
  previewRender: (path: string, recipe: Recipe) =>
    invoke<PreviewResult>("preview_render", { path, recipe: cleanRecipe(recipe) }),
  startRender: (items: { path: string; recipe: Recipe }[], outDir: string) =>
    invoke<void>("start_render", { items: items.map((i) => ({ path: i.path, recipe: cleanRecipe(i.recipe) })), outDir }),
  cancelRender: () => invoke<void>("cancel_render"),
  defaultOutputDir: () => invoke<string>("default_output_dir"),
  reveal: (path: string) => invoke<void>("reveal", { path }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  fsRoots: () => invoke<DirNode[]>("fs_roots"),
  listDirs: (path: string) => invoke<DirNode[]>("list_dirs", { path }),
  cameraPresets: () => invoke<CameraPreset[]>("camera_presets"),
  writeCameraPreset: (slot: number, name: string, recipe: Recipe) =>
    invoke<WriteResult>("write_camera_preset", { slot, name, recipe: cleanRecipe(recipe) }),
  undoCameraPreset: (slot: number) => invoke<WriteResult>("undo_camera_preset", { slot }),
};

export const fileUrl = (p: string) => convertFileSrc(p);

// ---- thumbnail loader with limited concurrency + memo ----
const thumbCache = new Map<string, Promise<string>>();
let active = 0;
const queue: (() => void)[] = [];
const MAX_ACTIVE = 4;

function runNext() {
  while (active < MAX_ACTIVE && queue.length) {
    active++;
    queue.shift()!();
  }
}

export function loadThumb(path: string, size = 480): Promise<string> {
  const key = `${path}|${size}`;
  let p = thumbCache.get(key);
  if (!p) {
    p = new Promise<string>((resolve, reject) => {
      queue.push(() => {
        api
          .thumbnail(path, size)
          .then((f) => resolve(fileUrl(f)))
          .catch(reject)
          .finally(() => {
            active--;
            runNext();
          });
      });
      runNext();
    });
    p.catch(() => thumbCache.delete(key));
    thumbCache.set(key, p);
  }
  return p;
}

// ---- labels ----
const FILM_LABELS: Record<string, [string, string]> = {
  provia: ["PROVIA / 标准", "PROVIA / Standard"],
  velvia: ["Velvia / 鲜艳", "Velvia / Vivid"],
  astia: ["ASTIA / 柔和", "ASTIA / Soft"],
  "pro-neg-hi": ["PRO Neg. Hi", "PRO Neg. Hi"],
  "pro-neg-std": ["PRO Neg. Std", "PRO Neg. Std"],
  monochrome: ["黑白", "Monochrome"],
  "monochrome-ye": ["黑白 + 黄滤镜", "Monochrome + Ye"],
  "monochrome-r": ["黑白 + 红滤镜", "Monochrome + R"],
  "monochrome-g": ["黑白 + 绿滤镜", "Monochrome + G"],
  sepia: ["棕褐色", "Sepia"],
  "classic-chrome": ["Classic Chrome / 经典正片", "Classic Chrome"],
  acros: ["ACROS", "ACROS"],
  "acros-ye": ["ACROS + 黄滤镜", "ACROS + Ye"],
  "acros-r": ["ACROS + 红滤镜", "ACROS + R"],
  "acros-g": ["ACROS + 绿滤镜", "ACROS + G"],
  eterna: ["ETERNA / 影院", "ETERNA / Cinema"],
  "classic-neg": ["Classic Neg. / 经典负片", "Classic Neg."],
  "eterna-bleach-bypass": ["ETERNA 漂白效果", "ETERNA Bleach Bypass"],
  "nostalgic-neg": ["Nostalgic Neg. / 怀旧负片", "Nostalgic Neg."],
  "reala-ace": ["REALA ACE", "REALA ACE"],
};

/** Display name of a film simulation id ("" / null = keep as shot). */
export function filmName(k: string | null | undefined): string {
  if (!k) return L("原片", "Default");
  const v = FILM_LABELS[k];
  return v ? L(v[0], v[1]) : k;
}

/** Recipe name stored for "keep everything as shot"; shown translated. */
export const AS_SHOT = "原片设置";
export function recipeLabel(r: { name?: string | null }): string {
  if (!r.name || r.name === AS_SHOT) return L("原片设置", "Default");
  return r.name;
}

export function fmtDuration(ms: number): string {
  if (!isFinite(ms) || ms < 0) return "--";
  const s = ms / 1000;
  if (s < 60) return L(`${s.toFixed(1)} 秒`, `${s.toFixed(1)} s`);
  const m = Math.floor(s / 60);
  return L(`${m} 分 ${Math.round(s % 60)} 秒`, `${m} min ${Math.round(s % 60)} s`);
}

export function fmtSize(b: number): string {
  return `${(b / 1024 / 1024).toFixed(1)} MB`;
}
