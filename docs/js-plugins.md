# JavaScript plugins

Write your own Streamtubah plugins in JavaScript: a voice changer that feeds the stream, a soundboard,
a bot that switches scenes on chat commands, a port of an app you already have…

Each plugin runs in its own hidden window (a normal browser page: Web Audio, microphone, WebSocket,
`fetch` all work). It talks to Streamtubah through `window.streamtubah`, limited to the permissions
you approved. Plugin pages cannot call the app's internals directly.

## Install

1. Settings → Plugins → JavaScript plugins → **Open** (the folder is `%APPDATA%\com.streamtubah.app\plugins-js`).
2. Put the plugin in its own folder there, e.g. `plugins-js\voice-fx\`. The folder name is the plugin id
   (letters, digits, `-`, `_`).
3. **Refresh**, tick the plugin, read the warning and the permissions, enable it.

The remote control API must be on (Settings → Remote API, default) because plugins use its port.
If a plugin's manifest later asks for a permission you did not approve, it stops until you enable it again.

Sample: `examples/js-plugins/voice-fx` (mic → Web Audio effect → a "Voice FX" Plugin Audio source).

## manifest.json

```json
{
  "name": "Voice FX",
  "version": "1.0.0",
  "description": "Shown in the plugin list and the enable dialog",
  "author": "you",
  "main": "index.html",
  "permissions": ["mic", "audio", "inputs"],
  "window": { "title": "Voice FX", "width": 420, "height": 520 }
}
```

- `main`: an `.html` page, or a `.js` file (Streamtubah wraps it in an empty page).
  Other files in the folder (scripts, CSS, images, sounds, wasm) load with relative paths.
- `window`: size/title of the plugin's own window (hidden until shown).

| Permission | Allows |
|---|---|
| `scenes` | scene list, switch/create/remove scenes, scene items (show/hide) — and their events |
| `inputs` | list/create/remove sources, volume, mute, settings, filters — and their events |
| `outputs` | start/stop stream, recording, replay buffer, virtual camera — and their events |
| `collections` | scene collections and profiles |
| `audio` | send audio into Plugin Audio sources; process audio through Audio Bridge filters |
| `mic` | use the microphone (`getUserMedia`) |

Without a permission the request fails with `This plugin has no '<permission>' permission`.
Requests not listed here need every permission.

## API (`window.streamtubah`)

```js
streamtubah.id, streamtubah.permissions

// Control API: obs-websocket v5 request names and fields (see docs/control-api.md).
const { scenes } = await streamtubah.request("GetSceneList");
await streamtubah.request("SetCurrentProgramScene", { sceneName: "BRB" });

// Events (obs-websocket names); "*" = all events the permissions allow.
const off = streamtubah.on("CurrentProgramSceneChanged", (data) => console.log(data.sceneName));

// Own window (the close button only hides it; the plugin keeps running).
await streamtubah.showWindow();
await streamtubah.hideWindow();

// Log: console.* output and uncaught errors also go to Settings → Plugins → Log.
streamtubah.log("hello");

// Public chat messages and alerts (no permission needed).
streamtubah.chat.on((msg) => { /* { platform, author, text, … } */ });
streamtubah.alerts.on((m) => { if (m.type === "alert") console.log(m.event); });
```

### Audio (`audio` permission)

Audio is 48 kHz, stereo, interleaved `Float32Array` (`streamtubah.audio.SAMPLE_RATE`).

```js
// 1. Any Web Audio node → a Plugin Audio source on channel "my-voice".
const ctx = new AudioContext({ sampleRate: 48000 });
const mic = ctx.createMediaStreamSource(await navigator.mediaDevices.getUserMedia({ audio: true }));
const fx = ctx.createBiquadFilter();            // your effect chain
mic.connect(fx);
const link = await streamtubah.audio.connectNode(fx, "my-voice");   // link.close() to stop

// 2. Raw blocks you produce yourself.
const src = await streamtubah.audio.openSource("my-voice");
src.write(new Float32Array(960 * 2));           // 20 ms of silence

// 3. Process an existing source (e.g. the Mic) in place: add the "Audio Bridge (external app)"
//    filter to it with channel "voice", then:
await streamtubah.audio.bridge("voice", (block) => block.map((s) => s * 0.5));
```

The Plugin Audio source: Add Source → Audio → **Plugin Audio**, channel = the name your plugin uses
(or create it from the plugin: `CreateInput` with `inputKind: "streamtubah_plugin_audio"`,
`inputSettings: { channel }`, needs `inputs`). To replace your voice, mute the normal Mic in the mixer.

## Notes

- Plugins keep running at full speed while their window is hidden (no background throttling);
  audio starts without a click.
- All plugins share one origin (`http://127.0.0.1:<API port>`), so `localStorage` keys should be
  prefixed with the plugin id.
- Microphone prompts are answered automatically from the `mic` permission; other browser permission
  prompts (notifications, location, camera…) are denied.
- **Reload** in the plugin list restarts a plugin after you edit its files.
- Other languages/apps can use the same endpoints directly: `ws://127.0.0.1:<port>/audio-source/<channel>`
  (Plugin Audio) and `/audio/<channel>` (Audio Bridge) accept the API password (`?password=`).
