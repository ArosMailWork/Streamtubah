// Streamtubah plugin SDK, injected into every plugin page as `window.streamtubah`.
// Docs: docs/js-plugins.md. Config comes from `window.__STREAMTUBAH__` (set just before this script).
(() => {
  "use strict";
  const cfg = window.__STREAMTUBAH__;
  if (!cfg || window.streamtubah) return;
  const base = `ws://127.0.0.1:${cfg.port}`;
  const auth = `plugin=${encodeURIComponent(cfg.id)}&token=${encodeURIComponent(cfg.token)}`;
  const SAMPLE_RATE = 48000;

  // ---- Control API (obs-websocket v5 protocol, limited by the plugin's permissions) ----
  let ws = null;
  let ready = null;
  let nextId = 1;
  const pending = new Map();
  const handlers = new Map();
  const queuedLogs = [];

  function connect() {
    ready = new Promise((resolve) => {
      ws = new WebSocket(`${base}/plugin?id=${encodeURIComponent(cfg.id)}&token=${encodeURIComponent(cfg.token)}`);
      ws.onmessage = (e) => {
        const m = JSON.parse(e.data);
        if (m.op === 0) ws.send(JSON.stringify({ op: 1, d: { rpcVersion: 1 } }));
        else if (m.op === 2) {
          resolve();
          for (const l of queuedLogs.splice(0)) send("PluginLog", l);
        } else if (m.op === 5) {
          for (const key of [m.d.eventType, "*"]) for (const fn of handlers.get(key) || []) {
            try { fn(m.d.eventData, m.d.eventType); } catch (err) { console.error(err); }
          }
        } else if (m.op === 7) {
          const p = pending.get(m.d.requestId);
          if (!p) return;
          pending.delete(m.d.requestId);
          if (m.d.requestStatus.result) p.resolve(m.d.responseData ?? null);
          else p.reject(new Error(m.d.requestStatus.comment || `Request failed (${m.d.requestStatus.code})`));
        }
      };
      ws.onclose = () => {
        for (const p of pending.values()) p.reject(new Error("Disconnected from Streamtubah"));
        pending.clear();
        setTimeout(connect, 2000);
      };
    });
  }

  function send(requestType, requestData) {
    const requestId = String(nextId++);
    return new Promise((resolve, reject) => {
      pending.set(requestId, { resolve, reject });
      ws.send(JSON.stringify({ op: 6, d: { requestType, requestId, requestData: requestData || {} } }));
    });
  }

  async function request(type, data) {
    await ready;
    return send(type, data);
  }

  function on(eventType, fn) {
    if (!handlers.has(eventType)) handlers.set(eventType, new Set());
    handlers.get(eventType).add(fn);
    return () => handlers.get(eventType).delete(fn);
  }

  // ---- Log (also captures console output and uncaught errors) ----
  const text = (args) => args.map((a) => (a instanceof Error ? a.stack || a.message : typeof a === "string" ? a : safeJson(a))).join(" ");
  function safeJson(v) { try { return JSON.stringify(v); } catch { return String(v); } }
  function log(level, args) {
    const entry = { level, text: text(args) };
    if (ws && ws.readyState === WebSocket.OPEN) send("PluginLog", entry).catch(() => {});
    else queuedLogs.push(entry);
  }
  for (const level of ["log", "info", "warn", "error"]) {
    const orig = console[level].bind(console);
    console[level] = (...args) => { orig(...args); log(level === "log" ? "info" : level, args); };
  }
  window.addEventListener("error", (e) => log("error", [e.error || e.message]));
  window.addEventListener("unhandledrejection", (e) => log("error", ["Unhandled promise rejection:", e.reason]));

  // ---- Simple JSON streams (public, read-only) ----
  function stream(path, fn) {
    let sock, closed = false;
    const open = () => {
      sock = new WebSocket(`${base}${path}`);
      sock.onmessage = (e) => { try { fn(JSON.parse(e.data)); } catch (err) { console.error(err); } };
      sock.onclose = () => { if (!closed) setTimeout(open, 2000); };
    };
    open();
    return () => { closed = true; sock.close(); };
  }

  // ---- Audio ----
  function audioSocket(path) {
    return new Promise((resolve, reject) => {
      const s = new WebSocket(`${base}${path}?${auth}`);
      s.binaryType = "arraybuffer";
      s.onmessage = (e) => { if (typeof e.data === "string") resolve(s); };
      s.onclose = (e) => reject(new Error(e.reason || "Audio connection closed (needs the 'audio' permission)"));
    });
  }

  /** Sends audio into every "Plugin Audio" source set to `channel`. */
  async function openSource(channel) {
    const s = await audioSocket(`/audio-source/${encodeURIComponent(channel)}`);
    return {
      /** Interleaved stereo Float32Array at 48 kHz. */
      write(samples) { if (s.readyState === WebSocket.OPEN) s.send(samples.buffer.slice(samples.byteOffset, samples.byteOffset + samples.byteLength)); },
      close() { s.close(); },
    };
  }

  /** Processes the audio of a source that has an "Audio Bridge" filter on `channel`.
   *  `process(block)` gets an interleaved stereo Float32Array and returns the processed block. */
  async function bridge(channel, process) {
    const s = await audioSocket(`/audio/${encodeURIComponent(channel)}`);
    s.onmessage = (e) => {
      if (typeof e.data === "string") return;
      const input = new Float32Array(e.data);
      const out = process(input) || input;
      s.send(out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength));
    };
    return { close() { s.close(); } };
  }

  const WORKLET = `
    class StreamtubahTap extends AudioWorkletProcessor {
      constructor() { super(); this.buf = new Float32Array(960 * 2); this.n = 0; }
      process(inputs) {
        const input = inputs[0];
        if (!input || input.length === 0) return true;
        const l = input[0], r = input[1] || input[0];
        for (let i = 0; i < l.length; i++) {
          this.buf[this.n++] = l[i]; this.buf[this.n++] = r[i];
          if (this.n === this.buf.length) { this.port.postMessage(this.buf, [this.buf.buffer]); this.buf = new Float32Array(960 * 2); this.n = 0; }
        }
        return true;
      }
    }
    registerProcessor("streamtubah-tap", StreamtubahTap);`;
  const workletLoaded = new WeakMap();

  /** Streams the output of a Web Audio node into the Plugin Audio source `channel`.
   *  The AudioContext must run at 48 kHz: `new AudioContext({ sampleRate: 48000 })`. */
  async function connectNode(node, channel) {
    const ctx = node.context;
    if (ctx.sampleRate !== SAMPLE_RATE) throw new Error(`AudioContext must run at ${SAMPLE_RATE} Hz (it runs at ${ctx.sampleRate})`);
    if (!workletLoaded.has(ctx)) {
      workletLoaded.set(ctx, ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([WORKLET], { type: "text/javascript" }))));
    }
    await workletLoaded.get(ctx);
    const out = await openSource(channel);
    const tap = new AudioWorkletNode(ctx, "streamtubah-tap", { numberOfInputs: 1, numberOfOutputs: 0, channelCount: 2, channelCountMode: "explicit" });
    tap.port.onmessage = (e) => out.write(e.data);
    node.connect(tap);
    return { close() { node.disconnect(tap); tap.port.close(); out.close(); } };
  }

  connect();
  window.streamtubah = Object.freeze({
    id: cfg.id,
    name: cfg.name,
    permissions: Object.freeze([...cfg.permissions]),
    request,
    on,
    log: (...args) => log("info", args),
    showWindow: () => request("PluginSetWindowVisible", { visible: true }),
    hideWindow: () => request("PluginSetWindowVisible", { visible: false }),
    chat: { on: (fn) => stream("/chat", fn) },
    alerts: { on: (fn) => stream("/alerts", fn) },
    audio: Object.freeze({ SAMPLE_RATE, openSource, bridge, connectNode }),
  });
})();
