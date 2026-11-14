// Right-side chat drawer. It sits beside the workspace (never over it) so the
// native preview shrinks smoothly instead of being covered. The tab strip is
// part of the drawer, so it travels with the opening edge.
import { useEffect, useRef, useState } from "react";
import { api, ApiInfo } from "../api";
import { act, refresh, toast } from "../store";
import { ChatView, overlayUrl } from "./ChatView";

const WIDTH_KEY = "streamtubah.chat.width";

export function ChatDrawer(props: { open: boolean; setOpen: (o: boolean) => void; onOpenSettings: () => void }) {
  const [width, setWidth] = useState(() => Number(localStorage.getItem(WIDTH_KEY)) || 340);
  const [dragging, setDragging] = useState(false);
  const [apiInfo, setApi] = useState<ApiInfo | null>(null);
  const start = useRef<{ x: number; w: number } | null>(null);

  useEffect(() => {
    api.apiInfo().then(setApi).catch(() => {});
  }, []);

  useEffect(() => {
    if (!dragging) return;
    const move = (e: PointerEvent) => {
      if (!start.current) return;
      const w = Math.min(Math.max(start.current.w + (start.current.x - e.clientX), 240), Math.round(window.innerWidth * 0.6));
      setWidth(w);
    };
    const up = () => {
      setDragging(false);
      start.current = null;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [dragging]);

  useEffect(() => {
    if (!dragging) localStorage.setItem(WIDTH_KEY, String(width));
  }, [dragging, width]);

  const addOverlay = async () => {
    if (!apiInfo) return;
    const name = await act(() => api.sourceCreate("browser_source", "Chat Overlay"));
    if (!name) return;
    await act(() => api.propertiesUpdate({ kind: "source", name }, { url: overlayUrl(apiInfo.port), width: 420, height: 640 }));
    refresh();
    toast("Chat overlay added to the current scene");
  };

  return (
    <aside className={`chat-drawer ${props.open ? "open" : ""} ${dragging ? "dragging" : ""}`}
      style={{ width: props.open ? width + 16 : 16 }}>
      <button className="edge-tab right-tab" onClick={() => props.setOpen(!props.open)} title={props.open ? "Hide chat" : "Show chat"}>
        <span className="edge-arrow">{props.open ? "›" : "‹"}</span>
        <span className="edge-label">CHAT</span>
      </button>
      <div className="chat-drawer-content" style={{ width }}>
        <div className="chat-resize" onPointerDown={(e) => { start.current = { x: e.clientX, w: width }; setDragging(true); }} />
        <ChatView
          toolbar={
            <>
              <button className="icon small" title="Pop out chat (always on top)" onClick={() => act(() => api.popoutOpen())}>⧉</button>
              <button className="icon small" title="Add chat overlay to the current scene" disabled={!apiInfo?.enabled} onClick={addOverlay}>＋</button>
              <button className="icon small" title="Chat settings" onClick={props.onOpenSettings}>⚙</button>
            </>
          }
        />
      </div>
    </aside>
  );
}
