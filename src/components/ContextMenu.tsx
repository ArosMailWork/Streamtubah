// Right-click menu. Native displays cannot be covered by HTML, so they are hidden while it is open.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useModalGate } from "../store";

export type MenuEntry =
  | { label: string; onClick: () => void; disabled?: boolean; danger?: boolean }
  | { label: string; submenu: MenuEntry[] }
  | "-";

export function ContextMenu(props: { x: number; y: number; entries: MenuEntry[]; onClose: () => void }) {
  useModalGate();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x: props.x, y: props.y });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    setPos({
      x: Math.max(4, Math.min(props.x, window.innerWidth - r.width - 4)),
      y: Math.max(4, Math.min(props.y, window.innerHeight - r.height - 4)),
    });
  }, [props.x, props.y]);

  useEffect(() => {
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== "Escape") return;
      if (e instanceof MouseEvent && ref.current?.contains(e.target as Node)) return;
      props.onClose();
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", close);
      window.removeEventListener("blur", close);
    };
  }, [props.onClose]);

  return (
    <div ref={ref} className="context-menu" style={{ left: pos.x, top: pos.y }} onContextMenu={(e) => e.preventDefault()}>
      <MenuItems entries={props.entries} onClose={props.onClose} />
    </div>
  );
}

function MenuItems(props: { entries: MenuEntry[]; onClose: () => void }) {
  return (
    <>
      {props.entries.map((e, i) => {
        if (e === "-") return <div key={i} className="context-sep" />;
        if ("submenu" in e) {
          return (
            <div key={i} className="context-item has-sub">
              <span>{e.label}</span><span className="muted">▸</span>
              <div className="context-menu sub"><MenuItems entries={e.submenu} onClose={props.onClose} /></div>
            </div>
          );
        }
        return (
          <button key={i} className={`context-item ${e.danger ? "danger" : ""}`} disabled={e.disabled}
            onClick={() => { props.onClose(); e.onClick(); }}>
            {e.label}
          </button>
        );
      })}
    </>
  );
}
