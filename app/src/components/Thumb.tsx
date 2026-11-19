import { useEffect, useRef, useState } from "react";
import { loadThumb } from "../api";
import { L } from "../i18n";

/** Lazily loads a cached thumbnail once the element scrolls into view. */
export function Thumb({ path, size = 480, className }: { path: string; size?: number; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setSrc(null);
    setFailed(false);
    const el = ref.current;
    if (!el) return;
    let cancelled = false;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          loadThumb(path, size)
            .then((s) => !cancelled && setSrc(s))
            .catch(() => !cancelled && setFailed(true));
        }
      },
      { rootMargin: "300px" },
    );
    io.observe(el);
    return () => {
      cancelled = true;
      io.disconnect();
    };
  }, [path, size]);

  return (
    <div ref={ref} className={`thumb ${className ?? ""}`}>
      {src ? <img src={src} draggable={false} /> : <div className="thumbph">{failed ? L("无预览", "No preview") : ""}</div>}
    </div>
  );
}
