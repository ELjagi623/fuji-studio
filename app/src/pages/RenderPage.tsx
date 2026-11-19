import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { api, FinishedEvent, fmtDuration, ItemEvent, ProgressEvent, RafEntry, Recipe, recipeLabel } from "../api";
import { L, photos } from "../i18n";
import { toast } from "../components/Toaster";
import { Thumb } from "../components/Thumb";
import { isWrongMode, ModeActions, openModeGuide } from "../components/ModeGuide";

export interface JobState {
  running: boolean;
  total: number;
  done: ItemEvent[];
  current: ProgressEvent | null;
  startedAt: number | null;
  finished: FinishedEvent | null;
  outDir: string | null;
}

const EMPTY: JobState = { running: false, total: 0, done: [], current: null, startedAt: null, finished: null, outDir: null };

export function useRenderJob() {
  const [state, setState] = useState<JobState>(EMPTY);

  useEffect(() => {
    const unsubs = [
      listen<ProgressEvent>("render-progress", (e) => setState((s) => ({ ...s, current: e.payload }))),
      listen<ItemEvent>("render-item", (e) => setState((s) => ({ ...s, done: [...s.done, e.payload] }))),
      listen<FinishedEvent>("render-finished", (e) => {
        const f = e.payload;
        setState((s) => ({ ...s, running: false, current: null, finished: f }));
        if (f.wrong_mode) {
          toast(L("相机连接模式不对，已停止渲染", "Camera is in the wrong USB mode, render stopped"), "error");
          openModeGuide();
        } else if (f.cancelled) toast(L(`已停止，完成 ${f.ok} 张`, `Stopped, ${f.ok} done`), "warn");
        else if (f.failed) toast(L(`渲染完成 ${f.ok} 张，失败 ${f.failed} 张`, `Rendered ${f.ok}, ${f.failed} failed`), "warn");
        else toast(L(`渲染完成，共 ${f.ok} 张`, `Done, rendered ${photos(f.ok)}`));
      }),
    ];
    return () => {
      unsubs.forEach((p) => p.then((f) => f()));
    };
  }, []);

  async function start(items: { path: string; recipe: Recipe }[], outDir: string) {
    const paths = items.map((i) => i.path);
    setState({ ...EMPTY, running: true, total: paths.length, startedAt: Date.now(), outDir });
    try {
      await api.startRender(items, outDir);
    } catch (e) {
      setState((s) => ({ ...s, running: false, finished: { ok: 0, failed: paths.length, cancelled: false, elapsed_ms: 0, wrong_mode: false } }));
      throw e;
    }
  }

  return { state, start, cancel: api.cancelRender };
}

const STAGE: Record<string, [string, string]> = {
  upload: ["上传 RAW", "Uploading RAW"],
  profile: ["写入配方", "Applying recipe"],
  convert: ["相机渲染中", "Rendering in camera"],
  download: ["下载 JPEG", "Downloading JPEG"],
};

interface Props {
  selected: Map<string, RafEntry>;
  /** recipe batch-applied to each photo on the Configure page; photos without one are skipped */
  assigned: Map<string, Recipe>;
  job: ReturnType<typeof useRenderJob>;
  cameraReady: boolean;
  cameraModel: string | null;
  wrongMode: boolean;
}

export function RenderPage({ selected, assigned, job, cameraReady, cameraModel, wrongMode }: Props) {
  const [outDir, setOutDir] = useState("");
  const [now, setNow] = useState(Date.now());

  const s = job.state;

  useEffect(() => {
    api.defaultOutputDir().then((d) => setOutDir((o) => o || d));
  }, []);
  useEffect(() => {
    if (!s.running) return;
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, [s.running]);

  const all = [...selected.values()];
  const sameModel = all.filter((e) => !cameraModel || e.model.toLowerCase() === cameraModel.toLowerCase());
  const usable = sameModel.filter((e) => assigned.has(e.path));
  const skippedModel = all.length - sameModel.length;
  const skippedNoRecipe = sameModel.length - usable.length;
  const recipeNames = [...new Set(usable.map((e) => recipeLabel(assigned.get(e.path)!)))];

  const doneCount = s.done.length;
  const okItems = s.done.filter((d) => d.out);
  const failItems = s.done.filter((d) => d.error);
  const elapsed = s.startedAt ? (s.running ? now : s.startedAt + (s.finished?.elapsed_ms ?? 0)) - s.startedAt : 0;
  const avg = doneCount ? elapsed / doneCount : NaN;
  const remaining = s.running && doneCount ? avg * (s.total - doneCount) : NaN;
  const overallPct = s.total ? ((doneCount + (s.current ? s.current.pct / 100 / 4 + stageBase(s.current.stage) : 0)) / s.total) * 100 : 0;

  async function browse() {
    const d = await open({ directory: true, title: L("选择输出文件夹", "Choose output folder"), defaultPath: outDir || undefined });
    if (typeof d === "string") setOutDir(d);
  }

  async function start() {
    try {
      await job.start(usable.map((e) => ({ path: e.path, recipe: assigned.get(e.path)! })), outDir);
    } catch (e) {
      if (isWrongMode(e)) openModeGuide();
      else toast(L("无法开始渲染", "Couldn't start rendering"), "error", String(e));
    }
  }

  return (
    <div className="render">
      <div className="panel renderctl">
        <div className="toolbar">
          <div className="title">{L("渲染设置", "Render settings")}</div>
        </div>
        <div className="kv">
          <span>{L("将渲染", "To render")}</span>
          <span>{photos(usable.length)}</span>
        </div>
        {skippedNoRecipe > 0 && (
          <div className="kv">
            <span>{L("未应用配方，跳过", "No recipe applied, skipped")}</span>
            <span className="muted">{photos(skippedNoRecipe)}</span>
          </div>
        )}
        {skippedModel > 0 && (
          <div className="kv">
            <span>{L("其他机型，跳过", "Other camera, skipped")}</span>
            <span className="muted">{photos(skippedModel)}</span>
          </div>
        )}
        <div className="kv">
          <span>{L("配方", "Recipes")}</span>
          <span className="ellipsis" title={recipeNames.join("\n")}>
            {recipeNames.length > 2 ? L(`${recipeNames.length} 种`, `${recipeNames.length} different`) : recipeNames.join(" · ") || "—"}
          </span>
        </div>
        <div className="kv">
          <span>{L("输出", "Output")}</span>
          <span>{L("全尺寸 JPEG", "Full-size JPEG")}</span>
        </div>
        <label className="field">
          <span className="flabel">{L("输出文件夹", "Output folder")}</span>
          <div className="row">
            <input className="grow" value={outDir} onChange={(e) => setOutDir(e.target.value)} />
            <button className="btn" onClick={browse}>
              {L("浏览", "Browse")}
            </button>
          </div>
        </label>

        <div className="row gap">
          {!s.running ? (
            <button className="btn primary big grow" disabled={!cameraReady || !usable.length || !outDir} onClick={start}>
              ▶ {L("开始渲染", "Start render")}
            </button>
          ) : (
            <button className="btn danger big grow" onClick={() => job.cancel()}>
              ■ {L("停止", "Stop")}
            </button>
          )}
        </div>
        {!s.running && !usable.length && all.length > 0 && (
          <div className="hint small">{L("先在「配置」页把配方批量应用到照片上", "Batch-apply a recipe to photos on the Configure page first")}</div>
        )}
        {!cameraReady && <div className="warnbox small">
            {wrongMode ? (
              <>
                {L("相机连接模式不对", "The camera is in the wrong USB mode")}{" "}
                <ModeActions />
              </>
            ) : (
              L("相机未连接", "Camera not connected")
            )}
          </div>}


        <div className="stats">
          <Stat label={L("已完成", "Done")} value={`${okItems.length} / ${s.total || usable.length}`} />
          <Stat label={L("失败", "Failed")} value={`${failItems.length}`} bad={failItems.length > 0} />
          <Stat label={L("已用时间", "Elapsed")} value={s.startedAt ? fmtDuration(elapsed) : "--"} />
          <Stat label={L("预计剩余", "Remaining")} value={s.running ? (doneCount ? fmtDuration(remaining) : L("计算中…", "Estimating…")) : "--"} />
          <Stat label={L("平均每张", "Per photo")} value={doneCount ? fmtDuration(avg) : "--"} />
          <Stat
            label={L("状态", "Status")}
            value={
              s.running
                ? L("渲染中", "Rendering")
                : s.finished
                  ? s.finished.cancelled
                    ? L("已停止", "Stopped")
                    : L("完成", "Finished")
                  : L("就绪", "Ready")
            }
          />
        </div>

        <div className="progress">
          <div style={{ width: `${Math.min(100, overallPct)}%` }} />
        </div>
        {s.current && (
          <div className="muted small">
            {L(`第 ${s.current.index + 1}/${s.current.total} 张`, `${s.current.index + 1}/${s.current.total}`)} · {s.current.file} ·{" "}
            {L(...(STAGE[s.current.stage] ?? [s.current.stage, s.current.stage]))}
            {s.current.stage === "upload" || s.current.stage === "download" ? ` ${s.current.pct}%` : ""}
          </div>
        )}
        {s.finished && s.outDir && (
          <button className="btn" onClick={() => api.reveal(s.outDir!)}>
            {L("打开输出文件夹", "Open output folder")}
          </button>
        )}
        {failItems.length > 0 && (
          <div className="faillist">
            {failItems.map((f) => (
              <div key={f.index} className="warnbox small">
                {f.file}：{f.error}
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="panel results">
        <div className="toolbar">
          <div className="title">{L("结果", "Results")}</div>
        </div>
        {okItems.length ? (
        <div className="grid">
          {[...okItems].reverse().map((d) => (
            <div key={d.out!} className="tile" onClick={() => api.reveal(d.out!)} title={d.out!}>
              <Thumb path={d.out!} />
              <div className="tilebar">
                <span className="ellipsis">{d.out!.split(/[\\/]/).pop()}</span>
                <span className="badge">{(d.ms / 1000).toFixed(1)} {L("秒", "s")}</span>
              </div>
            </div>
          ))}
        </div>
        ) : (
          <div className="empty">{L("渲染完成的照片会出现在这里", "Rendered photos will appear here")}</div>
        )}
      </div>
    </div>
  );
}

function stageBase(stage: string) {
  return { upload: 0, profile: 0.25, convert: 0.5, download: 0.75 }[stage] ?? 0;
}

function Stat({ label, value, bad }: { label: string; value: string; bad?: boolean }) {
  return (
    <div className={`stat ${bad ? "bad" : ""}`}>
      <div className="muted small">{label}</div>
      <div className="statval">{value}</div>
    </div>
  );
}
