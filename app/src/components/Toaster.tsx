import { useEffect, useState } from "react";

/** Top-right notifications. Call `toast("已保存")` / `toast("失败", "error")` from anywhere. */
export type ToastKind = "ok" | "warn" | "error";
interface Toast {
  id: number;
  text: string;
  detail?: string;
  kind: ToastKind;
  leaving?: boolean;
}

let nextId = 1;
let listeners: ((t: Toast[]) => void)[] = [];
let toasts: Toast[] = [];

function emit() {
  for (const l of listeners) l(toasts);
}

function dismiss(id: number) {
  toasts = toasts.map((t) => (t.id === id ? { ...t, leaving: true } : t));
  emit();
  setTimeout(() => {
    toasts = toasts.filter((t) => t.id !== id);
    emit();
  }, 200);
}

export function toast(text: string, kind: ToastKind = "ok", detail?: string) {
  const id = nextId++;
  toasts = [...toasts.slice(-4), { id, text, kind, detail }];
  emit();
  setTimeout(() => dismiss(id), kind === "ok" ? 3000 : 6000);
}

export function Toaster() {
  const [list, setList] = useState<Toast[]>(toasts);
  useEffect(() => {
    listeners.push(setList);
    return () => {
      listeners = listeners.filter((l) => l !== setList);
    };
  }, []);
  return (
    <div className="toaster">
      {list.map((t) => (
        <div key={t.id} className={`toast ${t.kind} ${t.leaving ? "leaving" : ""}`} onClick={() => dismiss(t.id)} title={t.detail}>
          <span className="toasticon">{t.kind === "ok" ? "✓" : "!"}</span>
          <div className="grow">
            <div>{t.text}</div>
            {t.detail && <div className="toastdetail">{t.detail}</div>}
          </div>
        </div>
      ))}
    </div>
  );
}
