# Remote control API (obs-websocket v5 compatible)

`ws://127.0.0.1:4455` by default (Settings → Remote API). Implements the obs-websocket v5
handshake (Hello / Identify with SHA-256 challenge auth / Identified), requests (op 6/7),
request batches (op 8/9) and events (op 5), so tools built for OBS work unchanged:
Stream Deck, Touch Portal, Streamer.bot, SAMMI, Mix It Up, obsws-python, obs-websocket-js…

## Supported requests
- General: GetVersion, GetStats, BroadcastCustomEvent
- Studio Mode: GetStudioModeEnabled, SetStudioModeEnabled, GetCurrentPreviewScene, SetCurrentPreviewScene,
  TriggerStudioModeTransition
- Scenes: GetSceneList, GetCurrentProgramScene, SetCurrentProgramScene, CreateScene, RemoveScene
- Inputs: GetInputList, CreateInput, RemoveInput, GetInputMute, SetInputMute, ToggleInputMute,
  GetInputVolume, SetInputVolume, GetInputSettings, SetInputSettings
- Scene items: GetSceneItemList, GetSceneItemId, GetSceneItemEnabled, SetSceneItemEnabled
- Filters: GetSourceFilterList, SetSourceFilterEnabled, CreateSourceFilter, RemoveSourceFilter
- Stream: GetStreamStatus, StartStream, StopStream, ToggleStream
- Record: GetRecordStatus, StartRecord, StopRecord, ToggleRecord, PauseRecord, ResumeRecord, ToggleRecordPause
- Replay buffer: GetReplayBufferStatus, StartReplayBuffer, StopReplayBuffer, ToggleReplayBuffer, SaveReplayBuffer
- Virtual camera: GetVirtualCamStatus, StartVirtualCam, StopVirtualCam, ToggleVirtualCam
  (the camera must have been started once from the app, which registers it with administrator permission)
- Scene collections: GetSceneCollectionList, SetCurrentSceneCollection, CreateSceneCollection
- Profiles: GetProfileList, SetCurrentProfile, CreateProfile, RemoveProfile

Unknown requests return code 204. UUID fields are empty strings (names are the identifiers).
`SetInputSettings` always overlays.

## Events
CurrentProgramSceneChanged, SceneListChanged, InputMuteStateChanged, InputVolumeChanged,
StreamStateChanged, RecordStateChanged, ReplayBufferStateChanged, ReplayBufferSaved, CustomEvent.

## Security
Binds to 127.0.0.1 unless "Allow other devices on my network" is enabled — and only when a
password is set. A random password is generated on first run.

Example client: `examples/control-api/client.py`.
