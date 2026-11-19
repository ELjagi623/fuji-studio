import { useEffect, useMemo, useState } from "react";
import { RafEntry, Recipe, recipeLabel } from "../api";
import { L, photos } from "../i18n";
import { FIELD_GROUPS, FieldGroup } from "../recipeFields";
import { GridPrefs, PhotoGrid, SelectMode, sortEntries, SortViewMenus } from "./PhotoGrid";
import { normPath } from "./FolderTree";

/**
 * Batch-apply window: copy (some fields of) the recipe being edited onto chosen photos.
 *  left   — which settings to copy (all ticked by default)
 *  middle — folders the selected photos come from (hidden when there is only one)
 *  right  — the selected photos, click / drag / Shift+click to choose (none chosen by default)
 * Nothing is rendered here; the camera only renders when a photo is previewed or exported.
 */

interface Props {
  recipe: Recipe;
  sourceName: string;
  /** in-camera slots have no exposure setting */
  hideExposure: boolean;
  photos: RafEntry[];
  assigned: Map<string, Recipe>;
  prefs: GridPrefs;
  onPrefs: (p: Partial<GridPrefs>) => void;
  cameraModel: string | null;
  onApply: (paths: string[], groups: FieldGroup[], all: boolean) => void;
  onClose: () => void;
}

const dirOf = (p: string) => p.replace(/[\\/][^\\/]*$/, "");
const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

export function BatchApply({ recipe, sourceName, hideExposure, photos: list, assigned, prefs, onPrefs, cameraModel, onApply, onClose }: Props) {
  const groups = useMemo(() => FIELD_GROUPS.filter((g) => !(hideExposure && g.id === "ev")), [hideExposure]);
  const [fields, setFields] = useState<Set<string>>(() => new Set(groups.map((g) => g.id)));
  const [chosen, setChosen] = useState<Set<string>>(() => new Set()); // nothing chosen by default: the user picks the photos
  const [folder, setFolder] = useState<string | null>(null); // null = all folders

  useEffect(() => {
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [onClose]);

  const folders = useMemo(() => {
    const m = new Map<string, { path: string; count: number }>();
    for (const e of list) {
      const d = dirOf(e.path);
      const k = normPath(d);
      const f = m.get(k) ?? { path: d, count: 0 };
      f.count++;
      m.set(k, f);
    }
    return [...m.values()];
  }, [list]);

  const shown = useMemo(
    () => sortEntries(folder ? list.filter((e) => normPath(dirOf(e.path)) === normPath(folder)) : list, prefs),
    [list, folder, prefs.sortBy, prefs.desc],
  );

  const onSelect = (items: RafEntry[], mode: SelectMode) =>
    setChosen((prev) => {
      const next = new Set(prev);
      for (const e of items) {
        const on = mode === "on" || (mode === "flip" && !next.has(e.path));
        if (on) next.add(e.path);
        else next.delete(e.path);
      }
      return next;
    });

  const toggleField = (id: string) =>
    setFields((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const pickedGroups = groups.filter((g) => fields.has(g.id));
  const canApply = chosen.size > 0 && pickedGroups.length > 0;
  const chosenHere = shown.filter((e) => chosen.has(e.path)).length;

  function apply() {
    if (!canApply) return;
    onApply([...chosen], pickedGroups, pickedGroups.length === groups.length);
    onClose();
  }

  return (
    <div className="modalback" onMouseDown={onClose}>
      <div className="modal batch" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modalhead">
          <div className="title">{L(`批量应用「${sourceName}」`, `Batch apply “${sourceName}”`)}</div>
          <div className="row gap">
            <SortViewMenus prefs={prefs} onChange={onPrefs} />
            <button className="x" onClick={onClose}>
              ×
            </button>
          </div>
        </div>

        <div className={`batchbody ${folders.length > 1 ? "withfolders" : ""}`}>
          <div className="batchcol">
            <div className="batchcolhead">
              <span>{L("应用哪些设置", "Settings to apply")}</span>
              <button
                className="linkbtn"
                onClick={() => setFields(fields.size === groups.length ? new Set() : new Set(groups.map((g) => g.id)))}
              >
                {fields.size === groups.length ? L("全不选", "None") : L("全选", "All")}
              </button>
            </div>
            <div className="batchfields">
              {groups.map((g) => (
                <label key={g.id} className="batchfield">
                  <input type="checkbox" checked={fields.has(g.id)} onChange={() => toggleField(g.id)} />
                  <span className="grow ellipsis">{g.label()}</span>
                  <span className="muted">{g.value(recipe)}</span>
                </label>
              ))}
            </div>
          </div>

          {folders.length > 1 && (
            <div className="batchcol">
              <div className="batchcolhead">
                <span>{L("文件夹", "Folders")}</span>
              </div>
              <div className="batchfolders">
                <div className={`batchfolder ${folder === null ? "active" : ""}`} onClick={() => setFolder(null)}>
                  <span className="grow">{L("全部", "All")}</span>
                  <span className="muted">{list.length}</span>
                </div>
                {folders.map((f) => (
                  <div
                    key={f.path}
                    className={`batchfolder ${folder && normPath(folder) === normPath(f.path) ? "active" : ""}`}
                    onClick={() => setFolder(f.path)}
                    title={f.path}
                  >
                    <span className="grow ellipsis">{baseName(f.path)}</span>
                    <span className="muted">{f.count}</span>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="batchcol wide">
            <div className="batchcolhead">
              <span>
                {L(`已选 ${chosenHere} / ${shown.length} 张`, `${chosenHere} of ${shown.length} chosen`)}
              </span>
              <span className="row gap">
                <button className="linkbtn" onClick={() => onSelect(shown, "on")}>
                  {L("全选", "All")}
                </button>
                <button className="linkbtn" onClick={() => onSelect(shown, "off")}>
                  {L("取消全选", "None")}
                </button>
              </span>
            </div>
            <PhotoGrid
              entries={shown}
              isSelected={(p) => chosen.has(p)}
              onSelect={onSelect}
              prefs={prefs}
              onPrefs={onPrefs}
              cameraModel={cameraModel}
              badge={(e) => {
                const r = assigned.get(e.path);
                return r ? recipeLabel(r) : null;
              }}
            />
          </div>
        </div>

        <div className="row gap modalfoot">
          <span className="muted small grow">
            {L("拖动可批量选择 · Shift+点击连续选择 · 应用后不会立即渲染", "Drag to select many · Shift+click for a range · nothing is rendered until you preview or export")}
          </span>
          <button className="btn" onClick={onClose}>
            {L("取消", "Cancel")}
          </button>
          <button className="btn primary" onClick={apply} disabled={!canApply}>
            {L(`应用到 ${chosen.size} 张`, `Apply to ${photos(chosen.size)}`)}
          </button>
        </div>
      </div>
    </div>
  );
}
