import { CameraStatus, filmName, Recipe, recipeLabel } from "../api";
import { useState } from "react";
import { getLang, isMac, L, Lang, MOD, photos, setLang } from "../i18n";
import type { JobState } from "../pages/RenderPage";
import { ModeActions, RAW_CONV_MENU } from "./ModeGuide";
import { CameraImage } from "./CameraImage";

interface Props {
  camera: CameraStatus | null;
  probeError: string | null;
  selectedCount: number;
  appliedCount: number;
  recipe: Recipe;
  job: JobState;
  onRetry: () => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
}

export function Sidebar({ camera, probeError, selectedCount, appliedCount, recipe, job, onRetry, canUndo, canRedo, onUndo, onRedo }: Props) {
  let dot = "off";
  let title = L("未连接相机", "No camera");
  let detail = L("请用 USB 连接相机", "Connect via USB");
  if (camera?.present) {
    if (camera.info?.raw_conv) {
      dot = camera.busy ? "busy" : "on";
      title = camera.info.model;
      detail = camera.busy ? L("正在处理…", "Working…") : L("已连接", "Connected");
    } else if (camera.info) {
      dot = "warn";
      title = camera.info.model;
      detail = L("连接模式不对", "Wrong USB mode");
    } else {
      dot = "busy";
      title = camera.name || L("富士相机", "Fujifilm camera");
      detail = probeError ? L(`连接失败：${probeError}`, `Connection failed: ${probeError}`) : L("正在识别…", "Detecting…");
    }
  }

  return (
    <aside className="sidebar">
      <div className="brand">Fuji Studio</div>

      <section className="card">
        <div className="cardtitle">{L("相机", "Camera")}</div>
        {camera?.info && <CameraImage model={camera.info.model} />}
        <div className="camrow">
          <span className={`dot ${dot}`} />
          <div>
            <div className="camname">{title}</div>
            <div className="muted small">{detail}</div>
          </div>
        </div>
        {camera?.info && (
          <div className="kv small">
            <span>{L("固件", "Firmware")}</span>
            <span>{camera.info.firmware}</span>
          </div>
        )}
        {camera?.present && camera.info && !camera.info.raw_conv && (
          <div className="hint small">
            {L("PC 连接模式改为", "Set PC connection mode to")} <b>{RAW_CONV_MENU()}</b>
            <ModeActions />
          </div>
        )}

        {probeError && (
          <button className="btn small" onClick={onRetry}>
            {L("重试", "Retry")}
          </button>
        )}
      </section>

      <section className="card">
        <div className="cardtitle">{L("本次任务", "Session")}</div>
        <div className="kv">
          <span>{L("已选照片", "Selected")}</span>
          <span>{photos(selectedCount)}</span>
        </div>
        <div className="kv">
          <span>{L("已应用配方", "Recipe applied")}</span>
          <span>
            {appliedCount} / {selectedCount}
          </span>
        </div>
        <div className="kv">
          <span>{L("正在编辑", "Editing")}</span>
          <span className="ellipsis" title={filmName(recipe.film)}>
            {recipeLabel(recipe)}
          </span>
        </div>
        {job.running && (
          <div className="kv">
            <span>{L("渲染进度", "Progress")}</span>
            <span>
              {job.done.length}/{job.total}
            </span>
          </div>
        )}
      </section>

      <div className="sidefoot">
        <div className="undoredo">
          <button onClick={onUndo} disabled={!canUndo} title={L(`撤销 (${MOD}Z)`, `Undo (${MOD}Z)`)}>
            <UndoIcon />
          </button>
          <button onClick={onRedo} disabled={!canRedo} title={isMac ? L("重做 (⇧⌘Z)", "Redo (⇧⌘Z)") : L("重做 (Ctrl+Y)", "Redo (Ctrl+Y)")}>
            <UndoIcon redo />
          </button>
        </div>
        <LangSwitch />
      </div>
    </aside>
  );
}

/** Bottom-left 中 / EN toggle. Keeps its own state so the highlight flips on the click itself. */
function LangSwitch() {
  const [cur, setCur] = useState<Lang>(getLang());
  const pick = (l: Lang) => {
    if (l === cur) return;
    setCur(l);
    setLang(l);
  };
  return (
    <div className="langswitch">
      <button className={cur === "zh" ? "on" : ""} onClick={() => pick("zh")}>
        中
      </button>
      <button className={cur === "en" ? "on" : ""} onClick={() => pick("en")}>
        EN
      </button>
    </div>
  );
}

/** Curved "turn back" arrow; mirrored for redo. */
function UndoIcon({ redo = false }: { redo?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={redo ? { transform: "scaleX(-1)" } : undefined} aria-hidden>
      <path d="M9 14 4 9l5-5" />
      <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
    </svg>
  );
}
