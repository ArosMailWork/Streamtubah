import { useEffect, useState } from "react";
import { api, StudioState } from "../api";
import { act, statsStore, useStore } from "../store";
import { Panel } from "./Panels";
import { DestinationList } from "./Destinations";

function useElapsed(active: boolean) {
  const [start, setStart] = useState<number | null>(null);
  const [, tick] = useState(0);
  useEffect(() => setStart(active ? Date.now() : null), [active]);
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => tick((x) => x + 1), 1000);
    return () => clearInterval(t);
  }, [active]);
  if (!start) return "00:00:00";
  const s = Math.floor((Date.now() - start) / 1000);
  return [s / 3600, (s % 3600) / 60, s % 60].map((v) => String(Math.floor(v)).padStart(2, "0")).join(":");
}

export function ControlsPanel({ state }: { state: StudioState }) {
  const o = state.outputs;
  const streamTime = useElapsed(o.streaming);
  const recordTime = useElapsed(o.recording);
  return (
    <Panel title="Controls" className="controls">
        {state.studio_mode && (
          <button className="studio-transition" title="Put the preview scene on air"
            disabled={state.preview_scene === state.current}
            onClick={() => act(() => api.studioTransition())}>
            Transition ▶
          </button>
        )}
        <button className={`go-live ${o.streaming ? "live" : ""}`}
          onClick={() => act(() => (o.streaming ? api.outputStop("stream") : api.streamStart()))}>
          {o.streaming ? `End Stream · ${streamTime}` : "Go Live"}
        </button>
        <DestinationList destinations={state.destinations} />
        <div className="row">
          <button className={`grow ${o.recording ? "rec" : ""}`}
            onClick={() => act(() => (o.recording ? api.outputStop("record") : api.recordStart()))}>
            {o.recording ? `■ Stop Rec · ${recordTime}` : "● Record"}
          </button>
          {o.recording && (
            <button onClick={() => act(() => api.recordPause(!o.recording_paused))}>
              {o.recording_paused ? "Resume" : "Pause"}
            </button>
          )}
        </div>
        <div className="row">
          <button className={`grow ${o.replay ? "on" : ""}`}
            onClick={() => act(() => (o.replay ? api.outputStop("replay") : api.replayStart()))}>
            {o.replay ? "Stop Replay Buffer" : "Start Replay Buffer"}
          </button>
          {o.replay && <button title="Save replay" onClick={() => act(() => api.replaySave())}>Save</button>}
        </div>
        <button className={o.virtualcam ? "on" : ""}
          title="Use the program output as a webcam in Discord, Zoom, Meet… (first start asks for administrator permission once)"
          onClick={() => act(() => api.virtualcamToggle())}>
          {o.virtualcam ? "Stop Virtual Camera" : "Start Virtual Camera"}
        </button>
    </Panel>
  );
}

export function StatusBar() {
  const s = useStore(statsStore);
  if (!s) return <footer className="statusbar" />;
  const pct = (a: number, b: number) => (b > 0 ? ((a / b) * 100).toFixed(1) : "0.0");
  return (
    <footer className="statusbar">
      <span>CPU {s.cpu.toFixed(1)}%</span>
      <span>{s.fps.toFixed(2)} FPS</span>
      <span title="Frames missed due to rendering lag">Render lag {pct(s.render_lagged, s.render_total)}%</span>
      <span title="Frames skipped due to encoding lag">Encode lag {pct(s.encode_skipped, s.encode_total)}%</span>
      {s.stream && (
        <>
          <span className="live-dot">LIVE{s.stream.destinations.length > 1 ? ` ×${s.stream.destinations.length}` : ""}</span>
          <span>{s.stream.kbps} kb/s</span>
          <span>Dropped {s.stream.dropped} ({pct(s.stream.dropped, s.stream.total)}%)</span>
        </>
      )}
      {s.outputs.recording && <span className="rec-dot">REC</span>}
    </footer>
  );
}
