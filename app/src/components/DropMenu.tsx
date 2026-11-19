import { useEffect, useRef, useState } from "react";

export type MenuItem = { label: string; checked?: boolean; onClick: () => void } | "sep";

/** Explorer-style toolbar dropdown: a button that opens a list of radio-like items. */
export function DropMenu({ label, icon, items }: { label: string; icon?: string; items: MenuItem[] }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);

  return (
    <div className="dropmenu" ref={ref}>
      <button className={`btn ${open ? "pressed" : ""}`} onClick={() => setOpen((o) => !o)}>
        {icon && <span className="dmicon">{icon}</span>}
        {label} <span className="caret">▾</span>
      </button>
      {open && (
        <div className="menu">
          {items.map((it, i) =>
            it === "sep" ? (
              <div key={i} className="menusep" />
            ) : (
              <div
                key={i}
                className="menuitem"
                onClick={() => {
                  it.onClick();
                  setOpen(false);
                }}
              >
                <span className="menucheck">{it.checked ? "●" : ""}</span>
                {it.label}
              </div>
            ),
          )}
        </div>
      )}
    </div>
  );
}
