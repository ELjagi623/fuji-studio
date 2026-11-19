import { filmName, Recipe } from "./api";
import { L } from "./i18n";

/** Option tables for the recipe editor and the batch-apply window. [value, 中文, English] */
export type Opt = [string, string, string];

export const EFFECT: Opt[] = [
  ["off", "关", "Off"],
  ["weak", "弱", "Weak"],
  ["strong", "强", "Strong"],
];
export const GRAIN: Opt[] = [
  ["off", "关", "Off"],
  ["weak", "弱", "Weak"],
  ["strong", "强", "Strong"],
  ["weak-large", "弱 · 大", "Weak · Large"],
  ["strong-large", "强 · 大", "Strong · Large"],
];
export const WB: Opt[] = [
  ["auto", "自动", "Auto"],
  ["auto-white", "自动 · 白色优先", "Auto · White priority"],
  ["auto-ambience", "自动 · 氛围优先", "Auto · Ambience priority"],
  ["daylight", "晴天", "Daylight"],
  ["shade", "阴天", "Shade"],
  ["incandescent", "白炽灯", "Incandescent"],
  ["fluorescent1", "荧光灯 1", "Fluorescent 1"],
  ["fluorescent2", "荧光灯 2", "Fluorescent 2"],
  ["fluorescent3", "荧光灯 3", "Fluorescent 3"],
  ["underwater", "水下", "Underwater"],
  ["kelvin", "色温 K", "Kelvin"],
];
export const DR: Opt[] = [
  ["100", "DR100", "DR100"],
  ["200", "DR200", "DR200"],
  ["400", "DR400", "DR400"],
];

/** "keep the value the photo was shot with" */
export const defLabel = () => L("原片", "Default");

function optLabel(opts: Opt[], v: string | number | null | undefined): string {
  if (v === null || v === undefined || v === "") return defLabel();
  const o = opts.find((x) => x[0] === String(v));
  return o ? L(o[1], o[2]) : String(v);
}

const num = (v: number | null | undefined) => (v === null || v === undefined ? defLabel() : v > 0 ? `+${v}` : `${v}`);

export interface FieldGroup {
  id: string;
  /** recipe keys applied together (white balance carries its colour temperature) */
  keys: (keyof Recipe)[];
  label: () => string;
  value: (r: Recipe) => string;
}

/** Everything the batch-apply window can copy, in editor order. */
export const FIELD_GROUPS: FieldGroup[] = [
  { id: "film", keys: ["film"], label: () => L("胶片模拟", "Film simulation"), value: (r) => (r.film ? filmName(r.film) : defLabel()) },
  { id: "dr", keys: ["dynamic_range"], label: () => L("动态范围", "Dynamic range"), value: (r) => optLabel(DR, r.dynamic_range) },
  { id: "grain", keys: ["grain"], label: () => L("颗粒效果", "Grain effect"), value: (r) => optLabel(GRAIN, r.grain) },
  { id: "cc", keys: ["color_chrome"], label: () => L("色彩效果", "Color Chrome"), value: (r) => optLabel(EFFECT, r.color_chrome) },
  { id: "ccb", keys: ["color_chrome_blue"], label: () => L("色彩效果 · 蓝", "Color Chrome FX Blue"), value: (r) => optLabel(EFFECT, r.color_chrome_blue) },
  { id: "skin", keys: ["smooth_skin"], label: () => L("美肤效果", "Smooth skin"), value: (r) => optLabel(EFFECT, r.smooth_skin) },
  {
    id: "wb",
    keys: ["white_balance", "wb_kelvin"],
    label: () => L("白平衡", "White balance"),
    value: (r) => (r.white_balance === "kelvin" && r.wb_kelvin ? `${r.wb_kelvin}K` : optLabel(WB, r.white_balance)),
  },
  { id: "wbr", keys: ["wb_shift_r"], label: () => L("白平衡偏移 R", "WB shift R"), value: (r) => num(r.wb_shift_r) },
  { id: "wbb", keys: ["wb_shift_b"], label: () => L("白平衡偏移 B", "WB shift B"), value: (r) => num(r.wb_shift_b) },
  { id: "hi", keys: ["highlight"], label: () => L("高光", "Highlight"), value: (r) => num(r.highlight) },
  { id: "sh", keys: ["shadow"], label: () => L("阴影", "Shadow"), value: (r) => num(r.shadow) },
  { id: "col", keys: ["color"], label: () => L("色彩", "Color"), value: (r) => num(r.color) },
  { id: "sharp", keys: ["sharpness"], label: () => L("锐度", "Sharpness"), value: (r) => num(r.sharpness) },
  { id: "nr", keys: ["noise_reduction"], label: () => L("高 ISO 降噪", "High ISO NR"), value: (r) => num(r.noise_reduction) },
  { id: "clar", keys: ["clarity"], label: () => L("清晰度", "Clarity"), value: (r) => num(r.clarity) },
  { id: "ev", keys: ["exposure"], label: () => L("曝光补偿", "Exposure"), value: (r) => num(r.exposure) },
];

/**
 * Copy the chosen field groups from `src` onto `base` (a photo's existing recipe, or
 * nothing = Default). Unchosen fields keep what the photo already had, so applies stack.
 */
export function mergeRecipe(base: Recipe | undefined, src: Recipe, groups: FieldGroup[], srcName: string, all: boolean): Recipe {
  const next: Recipe = { ...(base ?? { name: srcName }) };
  for (const g of groups) {
    for (const k of g.keys) {
      const v = src[k];
      if (v === null || v === undefined) delete next[k];
      else (next as unknown as Record<string, unknown>)[k] = v;
    }
  }
  if (all || !base) {
    next.name = srcName;
    if (src.raw?.length) next.raw = src.raw;
  }
  return next;
}
