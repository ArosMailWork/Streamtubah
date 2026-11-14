// Auto-Optimize: one click measures this PC (upload + real encode test) and proposes settings
// for the chosen preference between a smooth PC (left) and the best stream (right).
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, OptimizeResult } from "../api";
import { act, toast } from "../store";
import { UPLOAD_KEY } from "./Destinations";

export const LEVELS = [
  { name: "Smoothest PC", hint: "Up to 720p30, fastest encoder preset, 60 % bitrate. Games keep the most FPS." },
  { name: "Favor PC", hint: "Up to 720p60, fast preset, 75 % bitrate." },
  { name: "Balanced", hint: "Up to 1080p60, medium preset, full bitrate." },
  { name: "Favor stream", hint: "Up to 1080p60, slower preset for a cleaner picture." },
  { name: "Best stream", hint: "Up to 1440p (2K) when your PC, upload and platform allow; best preset." },
];

export function AutoOptimize(props: { live: boolean; onApplied: () => void; autoStart?: boolean; initialLevel?: number; onLevel?: (l: number) => void }) {
  const [level, setLevel] = useState(props.initialLevel ?? 3);
  const [busy, setBusy] = useState(false);
  const [step, setStep] = useState("");
  const [result, setResult] = useState<OptimizeResult | null>(null);
  const started = useRef(false);

  useEffect(() => {
    if (props.initialLevel === undefined) api.settingsGet().then((s) => setLevel(s.optimize.level || 3));
    const un = listen<{ step: string }>("optimize:progress", (e) => setStep(e.payload.step));
    return () => { un.then((f) => f()); };
  }, []);

  const run = async (lvl = level) => {
    setBusy(true);
    setResult(null);
    setStep("Starting…");
    const r = await act(() => api.optimizeRun(lvl));
    setBusy(false);
    setStep("");
    if (r) {
      setResult(r);
      // Remembered for the multistream upload estimate (Settings → Stream).
      if (r.measurement.upload_mbps !== null) localStorage.setItem(UPLOAD_KEY, String(r.measurement.upload_mbps));
    }
  };
  useEffect(() => {
    if (props.autoStart && !props.live && !started.current) {
      started.current = true;
      run();
    }
  }, [props.autoStart]);

  const apply = async () => {
    if (!result) return;
    const ok = await act(async () => { await api.optimizeApply(result.recommendation); return true; });
    if (ok) {
      toast("Optimized settings applied to the current profile");
      props.onApplied();
    }
  };

  const m = result?.measurement;
  const r = result?.recommendation;
  return (
    <div className="optimize">
      <div className="prop-info">
        One click measures this PC: upload speed, then a real encode test of a few seconds per setting (the preview may
        flicker). Only throw-away upload data goes to Cloudflare's public speed test. Takes about 10–25 seconds.
      </div>
      <div className="optimize-slider">
        <div className="row"><span className="muted small">◀ Smoother PC</span><span className="grow" /><span className="muted small">Better stream ▶</span></div>
        <input type="range" min={1} max={5} step={1} value={level} disabled={busy}
          onChange={(e) => { const l = +e.target.value; setLevel(l); setResult(null); props.onLevel?.(l); }} />
        <div className="optimize-ticks">{LEVELS.map((l, i) => <span key={l.name} className={i + 1 === level ? "on" : ""}>{l.name}</span>)}</div>
        <div className="muted small">{LEVELS[level - 1].hint}</div>
      </div>
      <div className="row">
        <button className="primary" disabled={busy || props.live} onClick={() => run()}>
          {busy ? "Optimizing…" : result ? "Test again" : "Optimize for this PC"}
        </button>
        {busy && <span className="muted small">{step}</span>}
        {props.live && <span className="muted small">Stop streaming/recording first.</span>}
      </div>
      {m && r && (
        <>
          <h3>This PC</h3>
          <div className="prop-row"><span className="prop-label">Graphics</span><span>{m.gpu_name}</span></div>
          <div className="prop-row"><span className="prop-label">Processor</span><span>{m.cpu_name} · {m.cpu_threads} threads</span></div>
          <div className="prop-row"><span className="prop-label">Upload speed</span>
            <span>{m.upload_mbps !== null ? `${m.upload_mbps} Mbit/s` : <span className="warning">{m.upload_error}</span>}</span></div>
          <div className="prop-row"><span className="prop-label">Encode test</span>
            <span className="trials">
              {r.trials.map((t) => (
                <span key={`${t.height}-${t.fps}`} className={t.passed ? "ok" : "bad"}
                  title={`Skipped ${t.skipped_pct}% · render lag ${t.lagged_pct}%`}>
                  {t.passed ? "✓" : "✗"} {t.height}p{t.fps}
                </span>
              ))}
            </span></div>

          <h3>Recommended · {LEVELS[r.level - 1].name}</h3>
          <div className="prop-row"><span className="prop-label">Encoder</span><span>{r.encoder_name}{r.hardware ? " (GPU)" : " (CPU)"}</span></div>
          <div className="prop-row"><span className="prop-label">Bitrate</span><span>{r.bitrate_kbps} kbps, CBR, keyframe every 2 s</span></div>
          <div className="prop-row"><span className="prop-label">Output</span><span>{r.width}×{r.height} at {r.fps} FPS</span></div>
          <ul className="tips">{r.reasons.map((x) => <li key={x}>{x}</li>)}</ul>
          <div className="row">
            <button className="primary" disabled={props.live} onClick={apply}>Apply to current profile</button>
            <span className="muted small">Canvas, scenes and stream keys are not changed. Higher than 1440p: set it yourself in Video/Output.</span>
          </div>
        </>
      )}
    </div>
  );
}
