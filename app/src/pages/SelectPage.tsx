import { useCallback, useEffect, useMemo, useState } from "react";
import { api, fmtSize, RafEntry, Settings } from "../api";
import { L, photos } from "../i18n";
import { Thumb } from "../components/Thumb";
import { FolderTree, normPath } from "../components/FolderTree";
import { GridPrefs, PhotoGrid, SelectMode, sortEntries, SortViewMenus } from "../components/PhotoGrid";

interface Props {
  selected: Map<string, RafEntry>;
  setSelected: React.Dispatch<React.SetStateAction<Map<string, RafEntry>>>;
  cameraModel: string | null;
  /** remembered across restarts (settings.json); loaded by App */
  settings: Settings | null;
  updateSettings: (patch: Partial<Settings>) => void;
  prefs: GridPrefs;
  onPrefs: (p: Partial<GridPrefs>) => void;
}

interface Scan {
  entries: RafEntry[];
  loading: boolean;
  error?: string;
}

const baseName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
export const dirOf = (p: string) => p.replace(/[\\/][^\\/]*$/, "");

/** Apply a PhotoGrid selection change to a path -> entry map. */
export function applySelect(prev: Map<string, RafEntry>, list: RafEntry[], mode: SelectMode): Map<string, RafEntry> {
  const next = new Map(prev);
  for (const e of list) {
    const on = mode === "on" || (mode === "flip" && !next.has(e.path));
    if (on) next.set(e.path, e);
    else next.delete(e.path);
  }
  return next;
}

export function SelectPage({ selected, setSelected, cameraModel, settings, updateSettings, prefs, onPrefs }: Props) {
  const [scans, setScans] = useState<Map<string, Scan>>(new Map());
  const [onlyCompatible, setOnlyCompatible] = useState(false);

  const current = settings?.last_dir ?? null;
  const sub = settings?.include_subfolders ?? false;
  const scanKey = current ? `${normPath(current)}|${sub}` : "";
  const scan = scans.get(scanKey);

  const rescan = useCallback(
    async (force = false) => {
      if (!current) return;
      const key = `${normPath(current)}|${sub}`;
      if (!force && scans.get(key) && !scans.get(key)!.error) return;
      setScans((m) => new Map(m).set(key, { entries: m.get(key)?.entries ?? [], loading: true }));
      try {
        const entries = await api.scanFolder(current, sub);
        setScans((m) => new Map(m).set(key, { entries, loading: false }));
      } catch (e) {
        setScans((m) => new Map(m).set(key, { entries: [], loading: false, error: String(e) }));
      }
    },
    [current, sub, scans],
  );

  useEffect(() => {
    rescan();
  }, [scanKey]);

  const compatible = (e: RafEntry) => !cameraModel || e.model.toLowerCase() === cameraModel.toLowerCase();
  const visible = useMemo(
    () => (scan ? sortEntries(scan.entries.filter((e) => !onlyCompatible || compatible(e)), prefs) : []),
    [scan, onlyCompatible, cameraModel, prefs.sortBy, prefs.desc],
  );

  const selectedList = [...selected.values()];
  const selectedDirs = useMemo(() => selectedList.map((e) => normPath(dirOf(e.path))), [selected]);
  const countUnder = useCallback(
    (path: string) => {
      const p = normPath(path);
      return selectedDirs.filter((d) => d === p || d.startsWith(p + "\\")).length;
    },
    [selectedDirs],
  );

  const onSelect = (list: RafEntry[], mode: SelectMode) => setSelected((prev) => applySelect(prev, list, mode));

  const mismatched = selectedList.filter((e) => !compatible(e)).length;
  const selectedHere = visible.filter((e) => selected.has(e.path)).length;

  return (
    <div className="select">
      <div className="panel folders">
        <div className="toolbar">
          <div className="title">{L("选择本地目录", "Choose a folder")}</div>
        </div>
        {settings && <FolderTree current={current} onPick={(p) => updateSettings({ last_dir: p })} countUnder={countUnder} />}
      </div>

      <div className="panel grid-panel">
        <div className="toolbar">
          <div className="ellipsis title" title={current ?? ""}>
            {current ?? ""}
          </div>
          <label className="check">
            <input type="checkbox" checked={sub} onChange={(e) => updateSettings({ include_subfolders: e.target.checked })} />
            {L("包含子文件夹", "Include subfolders")}
          </label>
          <label className="check">
            <input type="checkbox" checked={onlyCompatible} onChange={(e) => setOnlyCompatible(e.target.checked)} />
            {L("只看", "Only ")}
            {cameraModel ?? L("本机", "this camera")}
          </label>
          <button className="btn" onClick={() => rescan(true)} disabled={!current || scan?.loading} title={L("重新扫描", "Rescan")}>
            ⟳
          </button>
          <SortViewMenus prefs={prefs} onChange={onPrefs} />
          <button className="btn" onClick={() => onSelect(visible, "on")} disabled={!visible.length}>
            {L("全选", "Select all")}
          </button>
          <button className="btn" onClick={() => onSelect(visible, "off")} disabled={!selectedHere}>
            {L("取消全选", "Select none")}
          </button>
        </div>
        {visible.length ? (
          <PhotoGrid
            entries={visible}
            isSelected={(p) => selected.has(p)}
            onSelect={onSelect}
            prefs={prefs}
            onPrefs={onPrefs}
            cameraModel={cameraModel}
          />
        ) : (
          <div className="empty">
            {!current ? (
              L("选择左侧的文件夹", "Pick a folder on the left")
            ) : !scan || scan.loading ? (
              L("扫描中…", "Scanning…")
            ) : scan.error ? (
              <span title={scan.error}>{L("无法读取此文件夹", "Can't read this folder")}</span>
            ) : (
              <>
                <div>{L("没有 RAF 照片", "No RAF photos")}</div>
                {!sub && <div className="small">{L("可勾选「包含子文件夹」", "Try “Include subfolders”")}</div>}
              </>
            )}
          </div>
        )}
        <div className="statusbar muted small">
          {scan && !scan.loading ? `${photos(visible.length)} · ${L("已选", "selected")} ${selectedHere} · ` : ""}
          {L("拖动可批量选择 · Shift+点击连续选择", "Drag to select many · Shift+click for a range")}
        </div>
      </div>

      <div className="panel basket">
        <div className="toolbar">
          <div className="title">{L(`已选 ${selected.size} 张`, `${selected.size} selected`)}</div>
          <button className="btn" onClick={() => setSelected(new Map())} disabled={!selected.size}>
            {L("清空", "Clear")}
          </button>
        </div>
        {mismatched > 0 && (
          <div className="warnbox small">
            {L(`${mismatched} 张非 ${cameraModel} 拍摄，将跳过`, `${mismatched} not shot on ${cameraModel}, will be skipped`)}
          </div>
        )}
        <div className="basketlist">
          {selectedList.map((e) => (
            <div key={e.path} className="basketitem" title={e.path} onClick={() => updateSettings({ last_dir: dirOf(e.path) })}>
              <Thumb path={e.path} size={160} className="mini" />
              <div className="grow">
                <div className="ellipsis">{e.name}</div>
                <div className="muted small ellipsis">
                  {baseName(dirOf(e.path))} · {fmtSize(e.size)}
                </div>
              </div>
              <button
                className="x"
                onClick={(ev) => {
                  ev.stopPropagation();
                  onSelect([e], "off");
                }}
              >
                ×
              </button>
            </div>
          ))}
          {!selected.size && <div className="empty">{L("未选择照片", "No photos selected")}</div>}
        </div>
      </div>
    </div>
  );
}
