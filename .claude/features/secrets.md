# Secrets (encrypted vault + capture-protected Secrets window)

## Files
- `src-tauri/src/vault.rs` — DPAPI (current Windows user + app entropy) encrypted `secrets.bin` (base64 of the blob);
  get/set/copy_prefix/remove_prefix; `take_service_secrets` / `with_service_secrets` for service settings; unit tests
- `src-tauri/src/secrets.rs` — secret ids, list (names + set flags only), get/set, `open` (window label `secrets`)
- `src-tauri/src/settings.rs` — `AppSettings::take_secrets` (moves secrets out) / `with_secrets` (fills them back);
  vault keys `GOOGLE_SECRET`, `KICK_SECRET`, `API_PASSWORD`
- `src-tauri/src/lib.rs` — boot: `vault::init`, `migrate_secrets` (settings.json + every profiles/*.json), API password
  generated into the vault on first run
- `src/SecretsApp.tsx` (Secrets window), `src/components/SecretField.tsx` (main-window placeholders + `useSecrets`)

## What is secret
Stream key / WHIP bearer token / custom RTMP password of the main destination (`stream@<profile>.<field>`) and of
each multistream destination (`target.<id>.<field>`), Google + Kick client secrets, Remote API password.
settings.json and profile files never contain them (fields stripped by `apply_settings`, properties updates and boot
migration). Build-time platform secrets (option_env) are used as defaults and are not copied into the vault.

## Rules
- Main window: never receives values. `settings_get` has no secrets; service property forms hide `key`,
  `bearer_token`, `password`; `secrets_list` returns names + flags. `secret_get` / `secret_set` check the calling
  window label and refuse anything but `secrets`.
- Services are built from `settings.with_secrets()` (start_output, sync_destinations, properties); `secret_set`
  calls `outputs.reset_service()` so the next stream uses the new key (a running stream keeps its key).
- Secrets window: `content_protected(true)` → WDA_EXCLUDEFROMCAPTURE. Inputs are password type, empty until 👁
  fetches the value; 👁 again masks and drops it; window blur / visibilitychange masks everything and drops revealed
  values (unsaved typing is kept, masked). Focus target: URL `?focus=<id>` for a new window, `secrets:focus` event
  for an open one. `secrets:changed` event refreshes the main window placeholders.
- API password: clearing it generates a new random one (the API is never open). `api::configure` keeps the listener
  when only the password changes and joins the old listener thread before rebinding a new port.
- Profiles: create copies the main key and gives destinations new ids with copied keys; rename moves the key;
  remove deletes the profile's main + destination keys.
- Moving the portable app to another PC/user: the vault cannot be decrypted → renamed `secrets.bin.unreadable`,
  `vault::lost()` shows a banner; secrets must be entered again.

## Verified 2026-11-14 (CDP-driven tests against the running app, WebView2 remote debugging)
Migration of 4 plaintext secrets; main-window isolation (settings_get, secret_get/set refused, list without values,
forms without key); WDA_EXCLUDEFROMCAPTURE set; masking/eye/blur/unsaved-typing/save/focus (25/25, stable over 5
runs); capture with controls: GDI screen copy 55 % vs 0 %, OBS Display Capture 36 % vs 0.02 %, PrintWindow 93 % vs
0 % (main vs Secrets, magenta-marked); a mini RTMP server received exactly the vault key and the rotated key; profile
create/switch/rename/remove keep keys separate; no secret in settings.json, profiles or secrets.bin plaintext; API
password change applies at once, port change rebinds, saving settings from the main window keeps the API password.
Note: OBS Window Capture refuses windows of its own process, so it cannot serve as a test here.
