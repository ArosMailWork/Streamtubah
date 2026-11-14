import { useEffect, useRef } from "react";
import { api, DisplayId } from "../api";
import { useDisplaysHidden } from "../store";

/**
 * Placeholder that reserves space for a native display. The Rust side positions
 * a Win32 child window exactly over this element (physical pixels). Bounds are
 * re-sent on every size/position change, so drawer and resize animations keep
 * the native window in sync frame by frame.
 */
export function Preview({ display }: { display: DisplayId }) {
  const ref = useRef<HTMLDivElement>(null);
  const hidden = useDisplaysHidden();

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = "";
    let frame = 0;
    const sync = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const r = el.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        const b = [r.left, r.top, r.width, r.height].map((v) => Math.round(v * dpr));
        const key = b.join(",");
        if (key === last) return;
        last = key;
        api.previewSetBounds(display, b[0], b[1], b[2], b[3]).catch(() => {});
      });
    };
    const ro = new ResizeObserver(sync);
    ro.observe(el);
    // Position can change without a size change (sibling panels resizing).
    const poll = window.setInterval(sync, 250);
    window.addEventListener("resize", sync);
    sync();
    return () => {
      ro.disconnect();
      clearInterval(poll);
      cancelAnimationFrame(frame);
      window.removeEventListener("resize", sync);
      api.previewSetVisible(display, false).catch(() => {});
    };
  }, [display]);

  useEffect(() => {
    api.previewSetVisible(display, !hidden).catch(() => {});
  }, [display, hidden]);

  return (
    <div className="preview" ref={ref}>
      {hidden && <div className="preview-paused">Preview paused</div>}
    </div>
  );
}
