# Registering developer apps for platform login

Platform login (sending chat, follow/sub/raid alerts, auto stream key) needs one developer
app per platform. Register them with the project owner's account and put the resulting
**client IDs** into the build configuration (they are public identifiers; never commit
client *secrets*).

The app uses flows designed for desktop apps, so no secret has to ship with the binary:

| Platform | Flow | Redirect |
|---|---|---|
| Twitch | Device Code Grant (public client) | none |
| Google / YouTube | OAuth for installed apps, loopback + PKCE | `http://127.0.0.1` (any port) |
| Kick | OAuth 2.1 + PKCE | `http://localhost:17563/callback` |
| Facebook | Facebook Login for Devices | none |

## Twitch
1. https://dev.twitch.tv/console → *Register Your Application*.
2. Name: `Streamtubah`; OAuth Redirect URL: `http://localhost:17563/callback` (unused by device flow, but required by the form).
3. Category: *Broadcaster Suite*; **Client Type: Public**.
4. Copy the **Client ID**.

## Google / YouTube
1. https://console.cloud.google.com → create project `Streamtubah`.
2. *APIs & Services → Library* → enable **YouTube Data API v3**.
3. *OAuth consent screen*: External; add scopes `youtube.readonly` and `youtube.force-ssl` (needed to post chat).
   Add your own Google account as a test user while unverified.
4. *Credentials → Create credentials → OAuth client ID → Desktop app*.
5. Copy the **Client ID** (and the client "secret", which Google treats as non-confidential for desktop apps).
6. Publishing to everyone requires Google's app verification (sensitive scopes); until then only test users can sign in.
7. Quota: 10,000 units/day by default; reading chat through the API costs units, so chat reading stays on the
   no-login method and the API is used for posting messages, broadcast info and stream key.

## Kick
1. https://kick.com/settings/developer → create an app.
2. Redirect URL: `http://localhost:17563/callback`; scopes: `user:read`, `channel:read`, `chat:write`, `events:subscribe`.
3. Copy the **Client ID**.

## Facebook (Meta)
1. https://developers.facebook.com → *My Apps → Create App* → type *Business*.
2. Add product **Facebook Login for Business**; enable *Login from Devices*.
3. Copy the **App ID** and the **Client Token** (*Settings → Advanced*).
4. Live video and comments need permissions `publish_video`, `pages_read_engagement`, `pages_manage_posts`,
   which require Meta App Review before other people can use them.
