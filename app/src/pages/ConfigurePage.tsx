import { useEffect, useRef, useState } from "react";
import { api, AS_SHOT, CameraPreset, confirmAsk, filmName, fileUrl, loadThumb, RafEntry, Recipe, RecipeEntry, recipeLabel, WriteResult } from "../api";
import { toast } from "../components/Toaster";
import { isWrongMode, ModeActions, openModeGuide } from "../components/ModeGuide";
import { BatchApply } from "../components/BatchApply";
import { GridPrefs } from "../components/PhotoGrid";
import { FieldGroup } from "../recipeFields";
import { L, photos } from "../i18n";

const filmLabel = (r: Recipe) => filmName(r.film);

const LEVEL: Record<string, [string, string]> = {
  off: ["关", "Off"],
  weak: ["弱", "Weak"],
  strong: ["强", "Strong"],
  "weak-large": ["弱·大", "Weak·Large"],
  "strong-large": ["强·大", "Strong·Large"],
};
const lvl = (k: string) => (LEVEL[k] ? L(...LEVEL[k]) : k);
const sign = (v: number) => (v > 0 ? `+${v}` : `${v}`);

/** One-line description of a recipe, e.g. for tooltips. */
function summary(r: Recipe): string {
  const parts = [filmLabel(r)];
  if (r.dynamic_range) parts.push(`DR${r.dynamic_range}`);
  if (r.grain) parts.push(`${L("颗粒", "Grain ")}${lvl(r.grain)}`);
  if (r.color_chrome) parts.push(`${L("色彩效果", "Color Chrome ")}${lvl(r.color_chrome)}`);
  if (r.color_chrome_blue) parts.push(`${L("色彩效果蓝", "CC FX Blue ")}${lvl(r.color_chrome_blue)}`);
  if (r.white_balance) parts.push(`${L("白平衡", "WB")} ${r.white_balance}${r.wb_shift_r || r.wb_shift_b ? ` R${sign(r.wb_shift_r ?? 0)} B${sign(r.wb_shift_b ?? 0)}` : ""}`);
  if (r.highlight != null) parts.push(`${L("高光", "Highlight ")}${sign(r.highlight)}`);
  if (r.shadow != null) parts.push(`${L("阴影", "Shadow ")}${sign(r.shadow)}`);
  if (r.color != null) parts.push(`${L("色彩", "Color ")}${sign(r.color)}`);
  if (r.sharpness != null) parts.push(`${L("锐度", "Sharpness ")}${sign(r.sharpness)}`);
  if (r.noise_reduction != null) parts.push(`${L("降噪", "NR ")}${sign(r.noise_reduction)}`);
  if (r.clarity != null) parts.push(`${L("清晰度", "Clarity ")}${sign(r.clarity)}`);
  return parts.join(" · ");
}
import { Thumb } from "../components/Thumb";

interface Props {
  active: boolean;
  selected: Map<string, RafEntry>;
  recipe: Recipe;
  setRecipe: (r: Recipe) => void;
  recipes: RecipeEntry[];
  reloadRecipes: () => Promise<void>;
  cameraReady: boolean;
  cameraModel: string | null;
  wrongMode: boolean;
  onCameraError: () => Promise<void>;
  /** recipe batch-applied to each photo */
  assigned: Map<string, Recipe>;
  onApply: (paths: string[], src: Recipe, srcName: string, groups: FieldGroup[], all: boolean) => void;
  prefs: GridPrefs;
  onPrefs: (p: Partial<GridPrefs>) => void;
}

import { DR, EFFECT, GRAIN, Opt, WB } from "../recipeFields";

function EnumField({
  label,
  value,
  options,
  onChange,
  note,
}: {
  label: string;
  value: string | null | undefined;
  options: Opt[];
  onChange: (v: string | null) => void;
  note?: string;
}) {
  return (
    <label className="field">
      <span className="flabel">
        {label}
        {note && <em>{note}</em>}
      </span>
      <select value={value ?? ""} onChange={(e) => onChange(e.target.value || null)}>
        <option value="">{L("原片设置", "Default")}</option>
        {options.map(([v, zh, en]) => (
          <option key={v} value={v}>
            {L(zh, en)}
          </option>
        ))}
      </select>
    </label>
  );
}

function NumField({
  label,
  value,
  min,
  max,
  step = 1,
  def = 0,
  onChange,
  note,
}: {
  label: string;
  value: number | null | undefined;
  min: number;
  max: number;
  step?: number;
  def?: number;
  onChange: (v: number | null) => void;
  note?: string;
}) {
  const on = value !== null && value !== undefined;
  return (
    <div className="field">
      <span className="flabel">
        <label className="check">
          <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked ? def : null)} />
          {label}
        </label>
        {note && <em>{note}</em>}
      </span>
      <div className="numrow">
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          disabled={!on}
          value={on ? value! : def}
          onChange={(e) => onChange(Number(e.target.value))}
        />
        <span className="numval">{on ? (value! > 0 ? `+${value}` : `${value}`) : L("原片", "Default")}</span>
      </div>
    </div>
  );
}

export function ConfigurePage({
  active,
  selected,
  recipe,
  setRecipe,
  recipes,
  reloadRecipes,
  cameraReady,
  cameraModel,
  wrongMode,
  onCameraError,
  assigned,
  onApply,
  prefs,
  onPrefs,
}: Props) {
  const [batchOpen, setBatchOpen] = useState(false);
  const [films, setFilms] = useState<[string, number][]>([]);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ url: string; ms: number } | null>(null);
  const [original, setOriginal] = useState<string | null>(null);
  const [showOriginal, setShowOriginal] = useState(false);
  const [busy, setBusy] = useState(false);

  const [auto, setAuto] = useState(true);

  const pending = useRef(false);
  // A photo that already has a recipe applied previews with that recipe; others follow the editor.
  const appliedHere = previewPath ? assigned.get(previewPath) : undefined;
  const previewRecipe = appliedHere ?? recipe;
  const latest = useRef(previewRecipe);
  latest.current = previewRecipe;

  useEffect(() => {
    api.filmSims().then(setFilms);
  }, []);

  const list = [...selected.values()];
  const usable = list.filter((e) => !cameraModel || e.model.toLowerCase() === cameraModel.toLowerCase());
  useEffect(() => {
    if (!previewPath || !selected.has(previewPath)) setPreviewPath(usable[0]?.path ?? null);
  }, [selected, cameraModel]);

  useEffect(() => {
    setPreview(null);
    setOriginal(null);
    if (previewPath) loadThumb(previewPath, 1600).then(setOriginal).catch(() => {});
  }, [previewPath]);

  const set = <K extends keyof Recipe>(k: K, v: Recipe[K]) => setRecipe({ ...recipe, [k]: v });

  async function renderPreview() {
    if (!previewPath || !cameraReady) return;
    if (busy) {
      pending.current = true;
      return;
    }
    setBusy(true);

    try {
      do {
        pending.current = false;
        const r = await api.previewRender(previewPath, latest.current);
        setPreview({ url: `${fileUrl(r.jpg)}?t=${Date.now()}`, ms: r.ms });
      } while (pending.current);
    } catch (e) {
      if (isWrongMode(e)) {
        // not a photo problem: the camera is in the wrong USB mode — explain how to fix it
        onCameraError();
        openModeGuide();
      } else toast(L("预览失败", "Preview failed"), "error", String(e));
    } finally {
      setBusy(false);
    }
  }

  // auto preview on change (debounced)
  useEffect(() => {
    if (!auto || !active || !previewPath || !cameraReady) return;
    const t = setTimeout(renderPreview, 400);
    return () => clearTimeout(t);
  }, [previewRecipe, previewPath, auto, active, cameraReady]);

  // ---- in-camera custom settings (C1–C7) ----
  const [camPresets, setCamPresets] = useState<CameraPreset[] | null>(null);
  const [camLoading, setCamLoading] = useState(false);
  const [camError, setCamError] = useState<string | null>(null);
  const [selKey, setSelKey] = useState("orig");

  async function loadCamPresets() {
    setCamLoading(true);
    setCamError(null);
    try {
      setCamPresets(await api.cameraPresets());
    } catch (e) {
      setCamError(String(e));
    } finally {
      setCamLoading(false);
    }
  }

  // read once whenever a camera in RAW CONV mode (re)appears
  useEffect(() => {
    if (cameraReady && !camPresets && !camLoading) loadCamPresets();
    if (!cameraReady) setCamPresets(null);
  }, [cameraReady]);

  async function pick(key: string, r: Recipe, slotName?: string) {
    if (key === selKey) return;
    if (dirty && !(await confirmAsk(L(`放弃对 C${selSlot} 的修改？`, `Discard changes to C${selSlot}?`)))) return;
    setSelKey(key);
    setRecipe({ ...r });
    setEditName(slotName ?? "");
  }

  // ---- editing an in-camera slot: picking C1–C7 makes it editable directly ----
  const [editName, setEditName] = useState("");
  const [saving, setSaving] = useState(false);

  const [undoSlots, setUndoSlots] = useState<Set<number>>(new Set());
  const selSlot = selKey.startsWith("cam:") ? Number(selKey.slice(4)) : null;
  const editSlot = selSlot;
  const nameOk = /^[\x20-\x7e]{0,25}$/.test(editName);
  const slotPreset = camPresets?.find((x) => x.slot === selSlot) ?? null;
  const dirty = !!slotPreset && (editName !== slotPreset.name || sameSettings(recipe, slotPreset.recipe) === false);

  function revert() {
    if (!slotPreset) return;
    setRecipe({ ...slotPreset.recipe });
    setEditName(slotPreset.name);
  }

  function applyResult(r: WriteResult, verb: [string, string]) {
    setCamPresets((ps) => ps?.map((p) => (p.slot === r.preset.slot ? r.preset : p)) ?? null);
    setRecipe({ ...r.preset.recipe });
    setEditName(r.preset.name);
    setUndoSlots((s) => {
      const n = new Set(s);
      if (r.can_undo) n.add(r.preset.slot);
      else n.delete(r.preset.slot);
      return n;
    });
    const text =
      r.changed === 0
        ? L("没有改动", "No changes")
        : L(`已${verb[0]} C${r.preset.slot}（${r.changed} 项）`, `${verb[1]} C${r.preset.slot} (${r.changed} ${r.changed === 1 ? "change" : "changes"})`);
    if (r.problems.length)
      toast(L(`${text}，${r.problems.length} 项相机不接受`, `${text}, camera rejected ${r.problems.length}`), "warn", r.problems.join("\n"));
    else toast(text);
    if (r.backup) toast(L("已备份原来的 C1–C7", "Backed up original C1–C7"), "ok", r.backup);
  }

  async function saveToCamera() {
    if (editSlot === null || !nameOk) return;
    if (!(await confirmAsk(
          L(`覆盖相机里的 C${editSlot} 自定义设置？\n原设置会自动备份。`, `Overwrite custom setting C${editSlot} on the camera?\nThe original is backed up automatically.`),
        ))) return;
    setSaving(true);
    try {
      applyResult(await api.writeCameraPreset(editSlot, editName.trim(), recipe), ["保存", "Saved"]);
    } catch (e) {
      toast(L("保存到相机失败", "Couldn't save to camera"), "error", String(e));
    } finally {
      setSaving(false);
    }
  }

  async function undoCamera(slot: number) {
    if (!(await confirmAsk(L(`撤销 C${slot} 的修改？`, `Undo changes to C${slot}?`)))) return;
    setSaving(true);
    try {
      applyResult(await api.undoCameraPreset(slot), ["恢复", "Restored"]);
    } catch (e) {
      toast(L("撤销失败", "Undo failed"), "error", String(e));
    } finally {
      setSaving(false);
    }
  }

  async function save() {
    try {
      await api.saveRecipe(recipe);
      await reloadRecipes();
      setSelKey("u:" + recipe.name);
      toast(L(`已保存预设「${recipe.name}」`, `Saved preset “${recipe.name}”`));
    } catch (e) {
      toast(L("保存预设失败", "Couldn't save preset"), "error", String(e));
    }
  }

  async function remove(name: string) {
    await api.deleteRecipe(name);
    await reloadRecipes();
    toast(L(`已删除预设「${name}」`, `Deleted preset “${name}”`));
  }

  const shown = showOriginal ? original : preview?.url ?? null;
  const appliedCount = list.filter((e) => assigned.has(e.path)).length;
  const sourceName = editSlot !== null ? `C${editSlot}${editName.trim() ? ` ${editName.trim()}` : ""}` : recipeLabel(recipe);

  return (
    <div className="configure">
      <div className="panel recipes">
        <div className="toolbar">
          <div className="title">{L("机内设置", "In-camera")}</div>
          <button className="btn small" onClick={loadCamPresets} disabled={!cameraReady || camLoading || saving}>
            {camLoading ? L("读取中…", "Loading…") : L("刷新", "Refresh")}
          </button>
        </div>
        <div className="recipelist campresets">
          {camPresets?.map((p) => {
            const key = `cam:${p.slot}`;
            return (
              <div
                key={key}
                className={`recipeitem ${selKey === key ? "active" : ""}`}
                onClick={() => pick(key, p.recipe, p.name)}
                title={summary(p.recipe)}
              >
                <div className="ellipsis">
                  <span className="slot">C{p.slot}</span>
                  {p.name || <span className="muted">{L("未命名", "Unnamed")}</span>}
                  {selKey === key && dirty && <span className="editing"> ●</span>}
                </div>
                <div className="muted small ellipsis">{filmLabel(p.recipe)}</div>
              </div>
            );
          })}
          {!camPresets && (
            <div className="muted small pad">
              {camError ? <span title={camError}>{L("读取失败", "Couldn't read")}</span> : cameraReady ? L("读取中…", "Loading…") : L("未连接相机", "No camera")}
            </div>
          )}
        </div>

        <div className="toolbar topline">
          <div className="title">{L("我的预设", "My presets")}</div>
          <button className="btn small" onClick={() => pick("new", { name: L("新配方", "New recipe") })}>
            {L("新建", "New")}
          </button>
        </div>
        <div className="recipelist">
          <div className={`recipeitem ${selKey === "orig" ? "active" : ""}`} onClick={() => pick("orig", { name: AS_SHOT })}>
            <div>{L("原片设置", "Default")}</div>
            <div className="muted small">{L("保持拍摄设置", "Keep the original settings")}</div>
          </div>
          {recipes.map((r) => (
            <div
              key={(r.builtin ? "b:" : "u:") + r.recipe.name}
              className={`recipeitem ${selKey === (r.builtin ? "b:" : "u:") + r.recipe.name ? "active" : ""}`}
              onClick={() => pick((r.builtin ? "b:" : "u:") + r.recipe.name, r.recipe)}
            >
              <div className="ellipsis">{r.recipe.name}</div>
              <div className="muted small ellipsis">
                {r.builtin ? L("内置", "Built-in") : L("我的", "Mine")} · {filmLabel(r.recipe)}
              </div>
              {!r.builtin && (
                <button
                  className="x"
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(r.recipe.name);
                  }}
                >
                  ×
                </button>
              )}
            </div>
          ))}
        </div>
      </div>

      <div className="panel editor">
        <div className="toolbar">
          {editSlot !== null ? (
            <>
              <span className="slot big">C{editSlot}</span>
              <input
                className={`nameinput ${nameOk ? "" : "bad"}`}
                value={editName}
                maxLength={25}
                onChange={(e) => setEditName(e.target.value)}
                placeholder={L("名称（英文）", "Name")}
              />
              <button className="btn" onClick={save} title={L("保存到「我的预设」", "Save to My presets")}>
                {L("另存为我的预设", "Save as preset")}
              </button>
            </>
          ) : (
            <>
              <input
                className="nameinput"
                value={recipe.name === AS_SHOT ? L("原片设置", "Default") : recipe.name}
                onChange={(e) => set("name", e.target.value)}
                placeholder={L("配方名称", "Recipe name")}
              />
              <button className="btn primary" onClick={save} title={L("保存到「我的预设」", "Save to My presets")}>
                {selKey.startsWith("cam:") || selKey.startsWith("b:") ? L("另存为我的预设", "Save as preset") : L("保存预设", "Save preset")}
              </button>
            </>
          )}
        </div>
        {editSlot !== null && !nameOk && <div className="warnbox small m">{L("仅限英文，最多 25 个字符", "ASCII only, max 25 characters")}</div>}

        <div className="fields">
          <label className="field">
            <span className="flabel">{L("胶片模拟", "Film simulation")}</span>
            <select value={recipe.film ?? ""} onChange={(e) => set("film", e.target.value || null)}>
              <option value="">{L("原片设置", "Default")}</option>
              {films.map(([k]) => (
                <option key={k} value={k}>
                  {filmName(k)}
                </option>
              ))}
            </select>
          </label>
          <EnumField label={L("动态范围", "Dynamic range")} value={recipe.dynamic_range?.toString()} options={DR} onChange={(v) => set("dynamic_range", v ? Number(v) : null)} />
          <EnumField label={L("颗粒效果", "Grain effect")} value={recipe.grain} options={GRAIN} onChange={(v) => set("grain", v)} />
          <EnumField label={L("色彩效果", "Color Chrome effect")} value={recipe.color_chrome} options={EFFECT} onChange={(v) => set("color_chrome", v)} />
          <EnumField label={L("色彩效果 · 蓝", "Color Chrome FX Blue")} value={recipe.color_chrome_blue} options={EFFECT} onChange={(v) => set("color_chrome_blue", v)} />
          <EnumField label={L("美肤效果", "Smooth skin")} value={recipe.smooth_skin} options={EFFECT} onChange={(v) => set("smooth_skin", v)} note={L("未验证", "Unverified")} />
          <EnumField label={L("白平衡", "White balance")} value={recipe.white_balance} options={WB} onChange={(v) => set("white_balance", v)} />
          {recipe.white_balance === "kelvin" && (
            <NumField label={L("色温 K", "Color temp K")} value={recipe.wb_kelvin} min={2500} max={10000} step={100} def={5500} onChange={(v) => set("wb_kelvin", v)} />
          )}
          <NumField label={L("白平衡偏移 R", "WB shift R")} value={recipe.wb_shift_r} min={-9} max={9} onChange={(v) => set("wb_shift_r", v)} />
          <NumField label={L("白平衡偏移 B", "WB shift B")} value={recipe.wb_shift_b} min={-9} max={9} onChange={(v) => set("wb_shift_b", v)} />
          {/* X-Pro3 only accepts whole steps here (+2.5 is rejected with 0x201C) */}
          <NumField label={L("高光", "Highlight")} value={recipe.highlight} min={-2} max={4} onChange={(v) => set("highlight", v)} />
          <NumField label={L("阴影", "Shadow")} value={recipe.shadow} min={-2} max={4} onChange={(v) => set("shadow", v)} />
          <NumField label={L("色彩", "Color")} value={recipe.color} min={-4} max={4} onChange={(v) => set("color", v)} />
          <NumField label={L("锐度", "Sharpness")} value={recipe.sharpness} min={-4} max={4} onChange={(v) => set("sharpness", v)} />
          <NumField label={L("高 ISO 降噪", "High ISO NR")} value={recipe.noise_reduction} min={-4} max={4} onChange={(v) => set("noise_reduction", v)} />
          <NumField label={L("清晰度", "Clarity")} value={recipe.clarity} min={-5} max={5} onChange={(v) => set("clarity", v)} />
          {editSlot === null && (
            <NumField label={L("曝光补偿 EV", "Exposure EV")} value={recipe.exposure} min={-5} max={3} step={1 / 3} onChange={(v) => set("exposure", v === null ? null : Math.round(v * 100) / 100)} note={L("未验证", "Unverified")} />
          )}
        </div>
        {editSlot !== null ? (
          <div className="editbar">
            <div className="row gap">
              <button className="btn big" onClick={() => setBatchOpen(true)} disabled={!list.length} title={L("把这个配方应用到选中的照片", "Apply this recipe to chosen photos")}>
                {L("批量应用", "Batch apply")}
              </button>
              {dirty && (
                <button className="btn" onClick={revert} disabled={saving}>
                  {L("还原", "Revert")}
                </button>
              )}
              <button
                className="btn primary big grow"
                onClick={saveToCamera}
                disabled={saving || !nameOk || !dirty || !cameraReady}
                title={dirty ? "" : L("还没有改动", "No changes yet")}
              >
                {saving ? L("写入中…", "Writing…") : L(`保存到相机 C${editSlot}`, `Save to camera C${editSlot}`)}
              </button>
            </div>
            {undoSlots.has(editSlot) && (
              <button className="btn small" onClick={() => undoCamera(editSlot)} disabled={saving || !cameraReady}>
                ↶ {L("撤销上次保存", "Undo last save")}
              </button>
            )}
          </div>
        ) : (
          <div className="editbar">
            <button className="btn primary big" onClick={() => setBatchOpen(true)} disabled={!list.length} title={L("把这个配方应用到选中的照片", "Apply this recipe to chosen photos")}>
              {L("批量应用", "Batch apply")}
            </button>
          </div>
        )}
        {batchOpen && (
          <BatchApply
            recipe={recipe}
            sourceName={sourceName}
            hideExposure={editSlot !== null}
            photos={list}
            assigned={assigned}
            prefs={prefs}
            onPrefs={onPrefs}
            cameraModel={cameraModel}
            onApply={(paths, groups, all) => {
              onApply(paths, recipe, sourceName, groups, all);
              toast(L(`已将「${sourceName}」应用到 ${paths.length} 张照片`, `Applied “${sourceName}” to ${photos(paths.length)}`));
            }}
            onClose={() => setBatchOpen(false)}
          />
        )}
      </div>

      <div className="panel previewpanel">
        <div className="toolbar">
          <div className="title">{L("相机预览", "Camera preview")}</div>
          <label className="check">
            <input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} />
            {L("自动", "Auto")}
          </label>
          <button className="btn primary" onClick={renderPreview} disabled={!previewPath || !cameraReady}>
            {busy ? L("渲染中…", "Rendering…") : L("预览", "Preview")}
          </button>
        </div>
        <div
          className="viewer"
          title={L("按住对比原片", "Hold to compare with original")}
          onMouseDown={() => setShowOriginal(true)}
          onMouseUp={() => setShowOriginal(false)}
          onMouseLeave={() => setShowOriginal(false)}
        >
          {shown ? <img src={shown} /> : original ? <img src={original} className="dim" /> : null}
          {!previewPath && <div className="viewermsg">{L("未选择照片", "No photos selected")}</div>}
          {previewPath && !cameraReady && <div className="viewermsg">
              {wrongMode ? (
                <>
                  <div>{L("相机连接模式不对", "The camera is in the wrong USB mode")}</div>
                  <ModeActions />
                </>
              ) : (
                L("相机未连接", "Camera not connected")
              )}
            </div>}
          {busy && <div className="spinner" />}
          <div className="viewertag">
            {showOriginal
              ? L("原片", "Original")
              : [appliedHere ? `${L("已应用", "Applied")}: ${recipeLabel(appliedHere)}` : "", preview ? `${(preview.ms / 1000).toFixed(1)} ${L("秒", "s")}` : ""]
                  .filter(Boolean)
                  .join(" · ")}
          </div>
        </div>


        {list.length > 0 && (
          <div className="muted small striphead">
            {L(`已应用配方 ${appliedCount} / ${list.length} 张`, `Recipe applied: ${appliedCount} of ${list.length}`)}
            {appliedCount < list.length && L(" · 未应用的照片不会渲染", " · photos without one won't be rendered")}
          </div>
        )}
        <div className="filmstrip">
          {list.map((e) => {
            const ok = !cameraModel || e.model.toLowerCase() === cameraModel.toLowerCase();
            const a = assigned.get(e.path);
            return (
              <div
                key={e.path}
                className={`stripitem ${previewPath === e.path ? "on" : ""} ${ok ? "" : "incompatible"}`}
                onClick={() => ok && setPreviewPath(e.path)}
                title={a ? `${e.name}\n${L("已应用", "Applied")}: ${recipeLabel(a)}` : e.name}
              >
                <Thumb path={e.path} size={160} />
                {a && <div className="recipebadge">{recipeLabel(a)}</div>}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

/** Same settings, ignoring the recipe name and unset fields. */
function sameSettings(a: Recipe, b: Recipe): boolean {
  const norm = (r: Recipe) =>
    JSON.stringify(
      Object.entries(r)
        .filter(([k, v]) => k !== "name" && v !== null && v !== undefined && v !== "" && !(Array.isArray(v) && !v.length))
        .sort(([x], [y]) => x.localeCompare(y)),
    );
  return norm(a) === norm(b);
}
