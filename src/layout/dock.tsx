// Unity-style docking: drag a panel by its header onto another panel. Dropping
// in the middle swaps the two panels; dropping near an edge splits that panel.
// Native displays are hidden while dragging so the HTML drop zones are visible.
import { createContext, useContext, useEffect, useRef, useSyncExternalStore } from "react";
import { holdDisplaysHidden } from "../store";
import { closePanel, dock, Path, WidgetId, widgetInfo, Zone } from "./model";

interface DragState {
  presetId: string;
  from: Path;
  widget: WidgetId;
  x: number;
  y: number;
  over: { path: Path; zone: Zone } | null;
}

let drag: DragState | null = null;
const subs = new Set<() => void>();
const setDrag = (d: DragState | null) => {
  drag = d;
  subs.forEach((f) => f());
};
export const useDrag = () =>
  useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => {
        subs.delete(cb);
      };
    },
    () => drag,
  );

/** Which panel and which part of it is under the pointer. */
function hitTest(x: number, y: number): { path: Path; zone: Zone } | null {
  const el = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>("[data-dock-path]");
  if (!el) return null;
  const r = el.getBoundingClientRect();
  const fx = (x - r.left) / r.width;
  const fy = (y - r.top) / r.height;
  const edge = 0.25;
  const d = [
    { zone: "left" as Zone, v: fx },
    { zone: "right" as Zone, v: 1 - fx },
    { zone: "top" as Zone, v: fy },
    { zone: "bottom" as Zone, v: 1 - fy },
  ].sort((a, b) => a.v - b.v)[0];
  return { path: JSON.parse(el.dataset.dockPath!), zone: d.v < edge ? d.zone : "center" };
}

interface DockCtx {
  presetId: string;
  path: Path;
  widget: WidgetId;
}
const Ctx = createContext<DockCtx | null>(null);
export const DockProvider = Ctx.Provider;

/** Props for a panel header that should act as the drag handle, plus a close action. */
export function useDockHandle() {
  const ctx = useContext(Ctx);
  const start = useRef<{ x: number; y: number } | null>(null);
  const release = useRef<(() => void) | null>(null);

  useEffect(() => () => release.current?.(), []);

  if (!ctx) return { handle: {}, close: undefined };
  const handle = {
    className: "dock-handle",
    onPointerDown: (e: React.PointerEvent) => {
      if ((e.target as HTMLElement).closest("button, input, select")) return;
      start.current = { x: e.clientX, y: e.clientY };
      const move = (ev: PointerEvent) => {
        if (!start.current) return;
        if (!drag) {
          if (Math.hypot(ev.clientX - start.current.x, ev.clientY - start.current.y) < 6) return;
          release.current = holdDisplaysHidden();
          setDrag({ presetId: ctx.presetId, from: ctx.path, widget: ctx.widget, x: ev.clientX, y: ev.clientY, over: null });
        }
        setDrag({ ...drag!, x: ev.clientX, y: ev.clientY, over: hitTest(ev.clientX, ev.clientY) });
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        const d = drag;
        start.current = null;
        setDrag(null);
        release.current?.();
        release.current = null;
        if (d?.over) dock(d.presetId, d.from, d.over.path, d.over.zone);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
  };
  return { handle, close: () => closePanel(ctx.presetId, ctx.path) };
}

/** Wraps a panel: marks it as a drop target and draws the drop preview. */
export function DockLeaf({ ctx, children }: { ctx: DockCtx; children: React.ReactNode }) {
  const d = useDrag();
  const over = d?.over && d.over.path.join() === ctx.path.join() && d.from.join() !== ctx.path.join() ? d.over.zone : null;
  return (
    <Ctx.Provider value={ctx}>
      <div className={`dock-leaf ${d ? "dragging" : ""}`} data-dock-path={JSON.stringify(ctx.path)}>
        {children}
        {d && <div className={`dock-drop ${over ?? ""}`}>{over && <span>{over === "center" ? "Swap" : `Dock ${over}`}</span>}</div>}
      </div>
    </Ctx.Provider>
  );
}

/** Label that follows the pointer while dragging. */
export function DragGhost() {
  const d = useDrag();
  if (!d) return null;
  const w = widgetInfo(d.widget);
  return (
    <div className="drag-ghost" style={{ left: d.x + 12, top: d.y + 12 }}>
      {w.icon} {w.name}
    </div>
  );
}
