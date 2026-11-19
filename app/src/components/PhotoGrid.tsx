import { useEffect, useRef } from "react";
import { fmtSize, RafEntry, SortKey, ViewMode } from "../api";
import { L } from "../i18n";
import { DropMenu } from "./DropMenu";
import { Thumb } from "./Thumb";

/**
 * Photo grid / list shared by the select page and the batch-apply window:
 * Explorer-style icon sizes and sorting, click / drag-sweep / Shift+click selection.
 */

type Label = [string, string]; // [中文, English]

export const VIEWS: { id: ViewMode; label: Label }[] = [
  { id: "xl", label: ["超大图标", "Extra large icons"] },
  { id: "large", label: ["大图标", "Large icons"] },
  { id: "medium", label: ["中等图标", "Medium icons"] },
  { id: "small", label: ["小图标", "Small icons"] },
  { id: "list", label: ["列表", "List"] },
];

const SORTS: { id: SortKey; label: Label }[] = [
  { id: "name", label: ["名称", "Name"] },
  { id: "date", label: ["修改日期", "Date modified"] },
  { id: "size", label: ["大小", "Size"] },
  { id: "model", label: ["机型", "Camera"] },
];

const COLUMNS: { id: SortKey; label: Label }[] = [
  { id: "name", label: ["名称", "Name"] },
  { id: "date", label: ["修改日期", "Date modified"] },
  { id: "model", label: ["机型", "Camera"] },
  { id: "size", label: ["大小", "Size"] },
];

const THUMB_SIZE: Record<ViewMode, number> = { xl: 960, large: 480, medium: 320, small: 200, list: 160 };

const collator = new Intl.Collator("zh-CN", { numeric: true, sensitivity: "base" });

export interface GridPrefs {
  view: ViewMode;
  sortBy: SortKey;
  desc: boolean;
}

/** Sort like Explorer: by the chosen key, ties by name. Returns a new array. */
export function sortEntries(list: RafEntry[], p: GridPrefs): RafEntry[] {
  const cmp: Record<SortKey, (a: RafEntry, b: RafEntry) => number> = {
    name: (a, b) => collator.compare(a.name, b.name),
    date: (a, b) => a.modified - b.modified,
    size: (a, b) => a.size - b.size,
    model: (a, b) => collator.compare(a.model, b.model),
  };
  return [...list].sort((a, b) => (cmp[p.sortBy](a, b) || collator.compare(a.name, b.name)) * (p.desc ? -1 : 1));
}

/** The 排序 / 查看 dropdowns. */
export function SortViewMenus({ prefs, onChange }: { prefs: GridPrefs; onChange: (p: Partial<GridPrefs>) => void }) {
  return (
    <>
      <DropMenu
        label={L("排序", "Sort")}
        icon="⇅"
        items={[
          ...SORTS.map((s) => ({ label: L(...s.label), checked: prefs.sortBy === s.id, onClick: () => onChange({ sortBy: s.id }) })),
          "sep" as const,
          { label: L("递增", "Ascending"), checked: !prefs.desc, onClick: () => onChange({ desc: false }) },
          { label: L("递减", "Descending"), checked: prefs.desc, onClick: () => onChange({ desc: true }) },
        ]}
      />
      <DropMenu
        label={L("查看", "View")}
        icon="▦"
        items={VIEWS.map((v) => ({ label: L(...v.label), checked: prefs.view === v.id, onClick: () => onChange({ view: v.id }) }))}
      />
    </>
  );
}

export type SelectMode = "flip" | "on" | "off";

interface Props {
  /** already filtered and sorted */
  entries: RafEntry[];
  isSelected: (path: string) => boolean;
  /** apply a selection change; "flip" toggles each entry */
  onSelect: (entries: RafEntry[], mode: SelectMode) => void;
  prefs: GridPrefs;
  onPrefs: (p: Partial<GridPrefs>) => void;
  cameraModel: string | null;
  /** small label shown on a photo (e.g. the recipe applied to it) */
  badge?: (e: RafEntry) => string | null;
}

export function PhotoGrid({ entries, isSelected, onSelect, prefs, onPrefs, cameraModel, badge }: Props) {
  const { view, sortBy, desc } = prefs;
  const lastClicked = useRef<number | null>(null);
  const dragging = useRef(false);
  const scrollBox = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    lastClicked.current = null;
  }, [entries]);

  useEffect(() => {
    const end = () => {
      dragging.current = false;
    };
    window.addEventListener("mouseup", end);
    window.addEventListener("blur", end);
    return () => {
      window.removeEventListener("mouseup", end);
      window.removeEventListener("blur", end);
    };
  }, []);

  const compatible = (e: RafEntry) => !cameraModel || e.model.toLowerCase() === cameraModel.toLowerCase();

  // Drag to select: press = flip this photo; while held, every photo the pointer enters
  // flips (sweeping back over one un-selects it). Shift+click = range from the last click.
  function itemDown(ev: React.MouseEvent, e: RafEntry, i: number) {
    if (ev.button !== 0) return;
    ev.preventDefault(); // no text selection / image drag while sweeping
    const anchor = lastClicked.current;
    lastClicked.current = i;
    if (ev.shiftKey && anchor !== null) {
      const [a, b] = [anchor, i].sort((x, y) => x - y);
      onSelect(entries.slice(a, b + 1), isSelected(e.path) ? "off" : "on");
      return;
    }
    onSelect([e], "flip");
    dragging.current = true;
    autoScroll(ev.clientY);
  }

  function itemEnter(e: RafEntry, i: number) {
    if (!dragging.current) return;
    lastClicked.current = i;
    onSelect([e], "flip");
  }

  /** While dragging near the top/bottom edge of the photo area, keep scrolling. */
  function autoScroll(startY: number) {
    let y = startY;
    const move = (m: MouseEvent) => {
      y = m.clientY;
    };
    window.addEventListener("mousemove", move);
    const tick = () => {
      const box = scrollBox.current;
      if (!dragging.current || !box) {
        window.removeEventListener("mousemove", move);
        return;
      }
      const r = box.getBoundingClientRect();
      const EDGE = 40;
      if (y < r.top + EDGE) box.scrollTop -= Math.ceil((r.top + EDGE - y) / 3);
      else if (y > r.bottom - EDGE) box.scrollTop += Math.ceil((y - (r.bottom - EDGE)) / 3);
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  /** Ctrl + mouse wheel steps through the icon sizes, like Explorer. */
  function onWheel(ev: React.WheelEvent) {
    if (!ev.ctrlKey) return;
    const i = VIEWS.findIndex((v) => v.id === view);
    const next = VIEWS[Math.min(VIEWS.length - 1, Math.max(0, i + (ev.deltaY < 0 ? -1 : 1)))];
    if (next.id !== view) onPrefs({ view: next.id });
  }

  const tip = (e: RafEntry, ok: boolean) => (ok ? e.path : L(`${e.model} 拍摄，无法转换`, `Shot on ${e.model}, can't convert`));

  if (view === "list") {
    return (
      <div className="listview" onWheel={onWheel} ref={scrollBox}>
        <div className="listhead">
          <span />
          {COLUMNS.map((c) => (
            <span
              key={c.id}
              className={`lh ${c.id}`}
              onClick={() => onPrefs(c.id === sortBy ? { desc: !desc } : { sortBy: c.id, desc: false })}
            >
              {L(...c.label)}
              {sortBy === c.id && <span className="sortarrow">{desc ? "▼" : "▲"}</span>}
            </span>
          ))}
        </div>
        {entries.map((e, i) => {
          const on = isSelected(e.path);
          const ok = compatible(e);
          const b = badge?.(e);
          return (
            <div
              key={e.path}
              className={`listrow ${on ? "on" : ""} ${ok ? "" : "incompatible"}`}
              onMouseDown={(ev) => itemDown(ev, e, i)}
              onMouseEnter={() => itemEnter(e, i)}
              title={tip(e, ok)}
            >
              <Thumb path={e.path} size={THUMB_SIZE.list} className="list" />
              <span className="ellipsis name">
                {on && <span className="listtick">✓</span>}
                {e.name}
                {b && <span className="recipebadge inline">{b}</span>}
              </span>
              <span className="muted date">{fmtDate(e.modified)}</span>
              <span className={`model ${ok ? "muted" : "badtext"}`}>{e.model}</span>
              <span className="muted size">{fmtSize(e.size)}</span>
            </div>
          );
        })}
      </div>
    );
  }

  return (
    <div className={`grid view-${view}`} onWheel={onWheel} ref={scrollBox}>
      {entries.map((e, i) => {
        const on = isSelected(e.path);
        const ok = compatible(e);
        const b = badge?.(e);
        return (
          <div
            key={e.path}
            className={`tile ${on ? "on" : ""} ${ok ? "" : "incompatible"}`}
            onMouseDown={(ev) => itemDown(ev, e, i)}
            onMouseEnter={() => itemEnter(e, i)}
            title={tip(e, ok)}
          >
            <Thumb path={e.path} size={THUMB_SIZE[view]} />
            <div className="tilebar">
              <span className="ellipsis">{e.name}</span>
              {view !== "small" && <span className={`badge ${ok ? "" : "bad"}`}>{e.model}</span>}
            </div>
            {b && <div className="recipebadge">{b}</div>}
            {on && <div className="tick">✓</div>}
          </div>
        );
      })}
    </div>
  );
}

function fmtDate(secs: number): string {
  if (!secs) return "";
  const d = new Date(secs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
