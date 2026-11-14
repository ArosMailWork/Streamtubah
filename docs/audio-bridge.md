# Audio Bridge protocol

Lets any external program (voice changer, AI voice model, DSP script, any language)
process an audio source inside Streamtubah.

## Setup
1. In Streamtubah: Mixer → 🎙 Voice → *Add slot* → **External app (Audio Bridge)**.
   Pick a channel name (default `voice`).
2. Your app connects to `ws://127.0.0.1:<port>/audio/<channel>?password=<API password>`
   (port and password: Settings → Remote API; default port 4455).

## Messages
- Server → client, once, text:
  `{"type":"hello","channel":"voice","sample_rate":48000,"channels":2,"format":"f32le","layout":"interleaved"}`
- Server → client, binary: a block of interleaved stereo float32 little-endian samples
  (`L R L R …`). Block size varies with the source (typically 480–4096 frames).
- Client → server, binary: processed samples in the same format. Send back as much audio as
  you received; individual message sizes may differ, but total throughput must match real time.

## Behaviour
- The filter buffers `latency_ms` (default 60 ms) of returned audio before playing it and
  re-buffers after an underrun (silence meanwhile). Audio beyond 3× the buffer is dropped so
  latency stays bounded.
- With no client connected the filter passes the original audio through (configurable).
- One client per channel; a second connection is closed with code 4010.
- Wrong password: close code 4009.

## Examples
- `examples/audio-bridge/streamtubah_bridge.py` — tiny client library (`pip install websockets`).
- `examples/audio-bridge/voice_fx.py --password … --effect robot|echo|radio|passthrough`.
