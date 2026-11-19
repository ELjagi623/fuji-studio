import { startTransition, useEffect, useState } from "react";

/**
 * Minimal bilingual UI. Strings are written inline as `L("中文", "English")`, so
 * each one is translated where it is used. The App root subscribes via useLang(),
 * so switching language re-renders everything. Choice is kept in localStorage.
 */
export type Lang = "zh" | "en";

function initial(): Lang {
  try {
    const v = localStorage.getItem("lang");
    if (v === "zh" || v === "en") return v;
  } catch {}
  return "en"; // default: English until the user switches
}

let lang: Lang = initial();
let listeners: ((l: Lang) => void)[] = [];
applyDocument();

function applyDocument() {
  document.documentElement.lang = lang === "en" ? "en" : "zh-CN";
}

export const getLang = () => lang;

/** Pick the string for the current language. */
export function L(zh: string, en: string): string {
  return lang === "en" ? en : zh;
}

export function setLang(l: Lang) {
  lang = l;
  try {
    localStorage.setItem("lang", l);
  } catch {}
  applyDocument();
  // Re-rendering every page is the slow part: let React do it as a low-priority update so the
  // click (and the switch highlight, which keeps its own state) responds immediately.
  startTransition(() => {
    for (const f of listeners) f(l);
  });
}

/** Re-render on language change; always returns the live value (never a stale copy). */
export function useLang(): Lang {
  const [, force] = useState(0);
  useEffect(() => {
    const f = () => force((n) => n + 1);
    listeners.push(f);
    return () => {
      listeners = listeners.filter((x) => x !== f);
    };
  }, []);
  return lang;
}

/** "张" in Chinese, nothing/plural in English: count(3) → "3 张" / "3 photos". */
export function photos(n: number): string {
  return lang === "en" ? `${n} ${n === 1 ? "photo" : "photos"}` : `${n} 张`;
}
