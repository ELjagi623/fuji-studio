import { useEffect, useRef, useState } from "react";
import { api, DirNode } from "../api";
import { isMac, L } from "../i18n";

/** Quick-place labels come from the backend in Chinese. */
const PLACE_EN: Record<string, string> = { 图片: "Pictures", 桌面: "Desktop", 下载: "Downloads", 主目录: "Home" };

/** Windows paths are case-insensitive; compare on a normalized form without trailing separator. */
export const normPath = (p: string) => p.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();

const isUnder = (child: string, parent: string) => {
  const c = normPath(child);
  const p = normPath(parent);
  return c === p || c.startsWith(p + "\\");
};

type Kids = DirNode[] | "loading" | { error: string };

interface Props {
  current: string | null;
  onPick: (path: string) => void;
  /** number of selected photos per folder subtree, for the little badge */
  countUnder: (path: string) => number;
}

/** Lightroom-style in-app folder browser: quick places + drives, lazily expanded. */
export function FolderTree({ current, onPick, countUnder }: Props) {
  const [roots, setRoots] = useState<DirNode[] | null>(null);
  const [kids, setKids] = useState<Map<string, Kids>>(new Map());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const kidsRef = useRef(kids);
  kidsRef.current = kids;
  const activeRef = useRef<HTMLDivElement | null>(null);
  const revealing = useRef<string | null>(null);

  useEffect(() => {
    api.fsRoots().then(setRoots, () => setRoots([]));
  }, []);

  async function load(path: string): Promise<DirNode[]> {
    const key = normPath(path);
    const have = kidsRef.current.get(key);
    if (Array.isArray(have)) return have;
    setKids((m) => new Map(m).set(key, "loading"));
    try {
      const list = await api.listDirs(path);
      setKids((m) => new Map(m).set(key, list));
      kidsRef.current = new Map(kidsRef.current).set(key, list);
      return list;
    } catch (e) {
      setKids((m) => new Map(m).set(key, { error: String(e) }));
      return [];
    }
  }

  function toggle(n: DirNode) {
    const key = normPath(n.path);
    if (open.has(key)) {
      setOpen((s) => {
        const t = new Set(s);
        t.delete(key);
        return t;
      });
    } else {
      setOpen((s) => new Set(s).add(key));
      load(n.path);
    }
  }

  async function refresh(n: DirNode) {
    const key = normPath(n.path);
    kidsRef.current = new Map(kidsRef.current);
    kidsRef.current.delete(key);
    await load(n.path);
  }

  // Expand every ancestor of `current` so it is visible (on start-up and when navigating from elsewhere).
  useEffect(() => {
    if (!roots || !current || revealing.current === current) return;
    const root = roots
      .filter((r) => isUnder(current, r.path))
      .sort((a, b) => normPath(b.path).length - normPath(a.path).length)[0];
    if (!root) return;
    revealing.current = current;
    (async () => {
      const chain: string[] = [];
      let at = root.path;
      const rest = current.slice(normPath(root.path).length).split(/[\\/]/).filter(Boolean);
      for (const seg of rest) {
        chain.push(at);
        const list = await load(at);
        const next = list.find((d) => d.name.toLowerCase() === seg.toLowerCase());
        if (!next) {
          // folder was moved/deleted since last time: fall back to the deepest one that still exists
          if (normPath(at) !== normPath(current)) onPick(at);
          break;
        }
        at = next.path;
      }
      setOpen((s) => {
        const t = new Set(s);
        for (const c of chain) t.add(normPath(c));
        return t;
      });
      setTimeout(() => activeRef.current?.scrollIntoView({ block: "nearest" }), 50);
    })();
  }, [roots, current]);

  function renderNode(n: DirNode, depth: number): React.ReactNode {
    const key = normPath(n.path);
    const isOpen = open.has(key);
    const isActive = !!current && normPath(current) === key;
    const k = kids.get(key);
    const count = countUnder(n.path);
    return (
      <div key={key}>
        <div
          ref={isActive ? activeRef : undefined}
          className={`treerow ${isActive ? "active" : ""} ${n.kind}`}
          style={{ paddingLeft: 6 + depth * 14 }}
          onClick={() => {
            // whole row: open + expand; clicking the folder that is already open collapses it
            if (n.has_children && (!isOpen || isActive)) toggle(n);
            onPick(n.path);
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            refresh(n);
          }}
          title={n.path}
        >
          <span
            className={`twisty ${n.has_children ? "" : "none"}`}
            onClick={(e) => {
              e.stopPropagation();
              if (n.has_children) toggle(n);
            }}
          >
            {n.has_children ? (isOpen ? "▾" : "▸") : ""}
          </span>
          <span className="ficon">{n.kind === "drive" ? "🖴" : n.kind === "place" ? "★" : "📁"}</span>
          <span className="ellipsis grow">{n.kind === "place" ? L(n.name, PLACE_EN[n.name] ?? n.name) : n.name}</span>
          {count > 0 && <span className="selcount">{count}</span>}
        </div>
        {isOpen && (
          <>
            {k === "loading" && <div className="treemsg" style={{ paddingLeft: 26 + depth * 14 }}>{L("读取中…", "Loading…")}</div>}
            {k && !Array.isArray(k) && k !== "loading" && (
              <div className="treemsg bad" style={{ paddingLeft: 26 + depth * 14 }} title={k.error}>
                {L("无法读取", "Can't read")}
              </div>
            )}
            {Array.isArray(k) && k.map((c) => renderNode(c, depth + 1))}
          </>
        )}
      </div>
    );
  }

  if (!roots) return <div className="muted small pad">{L("读取磁盘…", "Reading drives…")}</div>;
  const places = roots.filter((r) => r.kind === "place");
  const drives = roots.filter((r) => r.kind !== "place");
  return (
    <div className="tree">
      <div className="treehead">{L("常用位置", "Quick access")}</div>
      {places.map((n) => renderNode(n, 0))}
      <div className="treehead">{isMac ? L("磁盘与位置", "Locations") : L("此电脑", "This PC")}</div>
      {drives.map((n) => renderNode(n, 0))}
    </div>
  );
}
