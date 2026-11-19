import { useEffect, useRef, useState } from "react";
import { api, CameraStatus } from "../api";
import { L } from "../i18n";
import { toast } from "./Toaster";

/**
 * Step-by-step guide for putting the camera into USB RAW CONV. mode.
 * Opens by itself when a camera is found in the wrong mode, or from anywhere via
 * `openModeGuide()` (e.g. after a preview fails with a "not supported" error).
 * Watches the live camera status and closes itself once the mode is right.
 */
let opener: (() => void) | null = null;
export function openModeGuide() {
  opener?.();
}

let refresher: (() => Promise<void>) | null = null;
let rechecking = false;

/**
 * Re-detect the camera now and always say what was found (toast), so the button
 * never looks like it did nothing. Opens the guide if the mode is still wrong.
 */
export async function recheckCamera(): Promise<void> {
  if (rechecking) return;
  rechecking = true;
  try {
    const info = await api.probeCamera();
    await refresher?.();
    if (info.raw_conv) toast(L(`${info.model} 已就绪（RAW 转换模式）`, `${info.model} is ready (RAW CONV. mode)`));
    else {
      toast(L("相机仍不是 RAW 转换模式", "The camera is still not in RAW CONV. mode"), "warn");
      openModeGuide();
    }
  } catch (e) {
    await refresher?.();
    toast(L("未检测到相机", "No camera found"), "error", L("请确认相机已开机，并用 USB 线连接", "Make sure the camera is on and connected via USB"));
  } finally {
    rechecking = false;
  }
}

/** The camera menu item, as it is called in the camera's own menu language. */
export const RAW_CONV_MENU = () => L("USB RAW 转换/备份恢复", "USB RAW CONV./BACKUP RESTORE");

/** Error strings tagged by the backend when the camera refused because of its USB mode. */
export const isWrongMode = (e: unknown) => String(e).includes("WRONG_MODE");

/** "查看设置方法" + "重新检测" side by side (sidebar, preview area, render page). */
export function ModeActions() {
  const [busy, setBusy] = useState(false);
  return (
    <span className="modeactions">
      <button className="btn small" onClick={(e) => (e.stopPropagation(), openModeGuide())}>
        {L("查看设置方法", "Show me how")}
      </button>
      <button
        className="btn small"
        disabled={busy}
        onClick={async (e) => {
          e.stopPropagation();
          setBusy(true);
          await recheckCamera();
          setBusy(false);
        }}
      >
        {busy ? L("检测中…", "Checking…") : L("重新检测", "Check again")}
      </button>
    </span>
  );
}

interface Props {
  camera: CameraStatus | null;
  onRecheck: () => Promise<void>;
}

export function ModeGuide({ camera, onRecheck }: Props) {
  const [open, setOpen] = useState(false);
  const [checking, setChecking] = useState(false);
  const shownFor = useRef<string | null>(null);

  const present = !!camera?.present;
  const wrong = present && !!camera?.info && !camera.info.raw_conv;
  const ok = !!camera?.info?.raw_conv;

  useEffect(() => {
    opener = () => setOpen(true);
    refresher = onRecheck;
    return () => {
      opener = null;
      refresher = null;
    };
  }, [onRecheck]);

  // pop up once per detected device that is in the wrong mode
  useEffect(() => {
    const id = camera?.info?.device_id ?? null;
    if (wrong && id && shownFor.current !== id) {
      shownFor.current = id;
      setOpen(true);
    }
  }, [wrong, camera?.info?.device_id]);

  // done: close shortly after the right mode shows up
  useEffect(() => {
    if (!open || !ok) return;
    const t = setTimeout(() => setOpen(false), 1500);
    return () => clearTimeout(t);
  }, [open, ok]);

  async function recheck() {
    setChecking(true);
    await recheckCamera();
    setChecking(false);
  }

  if (!open) return null;

  const state = ok
    ? { cls: "ok", text: L("已切换到 RAW 转换模式，可以开始使用了", "RAW CONV. mode detected, you're all set") }
    : wrong
      ? { cls: "warn", text: L(`已连接 ${camera!.info!.model}，但连接模式不对`, `${camera!.info!.model} connected, but in the wrong mode`) }
      : present
        ? { cls: "busy", text: L("正在识别相机…", "Detecting camera…") }
        : { cls: "off", text: L("等待相机连接…", "Waiting for the camera…") };

  return (
    <div className="modalback" onClick={() => setOpen(false)}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modalhead">
          <div className="title">{L("把相机切换到 RAW 转换模式", "Switch the camera to RAW CONV. mode")}</div>
          <button className="x" onClick={() => setOpen(false)}>
            ×
          </button>
        </div>
        <div className="muted small">
          {L(
            "软件要借相机的机内引擎渲染照片，相机必须处于这个 USB 模式，否则预览和渲染都会失败。",
            "Rendering runs on the camera's own engine, which only works in this USB mode. Otherwise preview and render will fail.",
          )}
        </div>
        <ol className="steps">
          <li>{L("拔下 USB 线", "Unplug the USB cable")}</li>
          <li>{L("按相机上的 MENU/OK 打开菜单", "Press MENU/OK on the camera")}</li>
          <li>
            {L("进入", "Go to")} <b>{L("设置（扳手图标）", "SET UP (wrench icon)")}</b> → <b>{L("连接设置", "CONNECTION SETTING")}</b> →{" "}
            <b>{L("PC 连接模式", "PC CONNECTION MODE")}</b>
          </li>
          <li>
            {L("选择", "Choose")} <b>{RAW_CONV_MENU()}</b>
          </li>
          <li>{L("重新插上 USB 线，软件会自动识别", "Plug the USB cable back in; the app detects it automatically")}</li>
        </ol>
        <div className={`modestate ${state.cls}`}>
          <span className={`dot ${state.cls === "ok" ? "on" : state.cls}`} />
          {state.text}
        </div>
        <div className="row gap modalfoot">
          <button className="btn" onClick={recheck} disabled={checking}>
            {checking ? L("检测中…", "Checking…") : L("重新检测", "Check again")}
          </button>
          <button className="btn primary" onClick={() => setOpen(false)}>
            {ok ? L("完成", "Done") : L("知道了", "Got it")}
          </button>
        </div>
      </div>
    </div>
  );
}
