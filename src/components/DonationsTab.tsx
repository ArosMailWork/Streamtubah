// Settings → Donations: Ko-fi (webhook through a temporary tunnel), Patreon (polling),
// StreamElements / Streamlabs (their event sockets).
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, AppSettings, DonateStatus } from "../api";
import { toast } from "../store";
import { SecretFields, useSecrets } from "./SecretField";

export function DonationsTab(props: { draft: AppSettings; saveNow: (s: AppSettings) => void }) {
  const { draft } = props;
  const secrets = useSecrets();
  const [st, setSt] = useState<DonateStatus | null>(null);
  useEffect(() => {
    api.donateStatus().then(setSt);
    const un = listen<DonateStatus>("donate:status", (e) => setSt(e.payload));
    return () => { un.then((f) => f()); };
  }, []);
  const set = (patch: Partial<AppSettings["donate"]>) => props.saveNow({ ...draft, donate: { ...draft.donate, ...patch } });

  return (
    <>
      <div className="prop-info">
        Donations raise alerts in the Alert Box (set their look in Settings → Alerts → Donation / New patron). Everything runs on
        this PC; no Streamtubah account or server.
      </div>

      <h3>Ko-fi</h3>
      <label className="prop-check">
        <input type="checkbox" checked={draft.donate.kofi_enabled} onChange={(e) => set({ kofi_enabled: e.target.checked })} />
        Receive Ko-fi donations
      </label>
      {draft.donate.kofi_enabled && (
        <>
          <div className="prop-row"><span className="prop-label">Status</span><span>{st?.kofi.state || "…"}</span></div>
          {st?.kofi.webhook_url && (
            <div className="prop-row">
              <span className="prop-label">Webhook URL</span>
              <div className="prop-path">
                <code className="grow url">{st.kofi.webhook_url}</code>
                <button onClick={() => navigator.clipboard.writeText(st.kofi.webhook_url!).then(() => toast("Webhook URL copied"))}>Copy</button>
              </div>
            </div>
          )}
          <SecretFields prefix="donate.kofi" secrets={secrets} />
          <ol className="muted small steps">
            <li>Open <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://ko-fi.com/manage/webhooks"); }}>Ko-fi → Settings → API / Webhooks</a>.</li>
            <li>Paste the Webhook URL above and press Update.</li>
            <li>Open "Advanced", copy the Verification Token and save it with 🛡 Edit securely above.</li>
            <li>Use "Send Single Donation Test" on Ko-fi to check.</li>
          </ol>
          <div className="prop-info warning small">
            The webhook URL is a temporary Cloudflare tunnel to this PC and changes each time Streamtubah starts: paste the new one into
            Ko-fi before each stream. Only Ko-fi webhooks are reachable through it (the remote control API is not exposed).
            The first start downloads Cloudflare's <code>cloudflared</code> (~60 MB).
          </div>
        </>
      )}

      <h3>Patreon</h3>
      <label className="prop-check">
        <input type="checkbox" checked={draft.donate.patreon_enabled} onChange={(e) => set({ patreon_enabled: e.target.checked })} />
        Alert when someone becomes a patron (checked every minute)
      </label>
      {draft.donate.patreon_enabled && (
        <>
          <div className="prop-row"><span className="prop-label">Status</span><span>{st?.patreon.state || "…"}</span></div>
          <SecretFields prefix="donate.patreon" secrets={secrets} />
          <ol className="muted small steps">
            <li>Open <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://www.patreon.com/portal/registration/register-clients"); }}>Patreon → Clients & API Keys</a> and create a client.</li>
            <li>Copy its "Creator's Access Token" and save it with 🛡 Edit securely above.</li>
          </ol>
        </>
      )}

      <h3>StreamElements</h3>
      <label className="prop-check">
        <input type="checkbox" checked={draft.donate.streamelements_enabled} onChange={(e) => set({ streamelements_enabled: e.target.checked })} />
        Receive StreamElements events (tips, merch, follows, subs, cheers, raids…)
      </label>
      {draft.donate.streamelements_enabled && (
        <>
          <div className="prop-row"><span className="prop-label">Status</span><span>{st?.streamelements.state || "…"}</span></div>
          <SecretFields prefix="donate.streamelements" secrets={secrets} />
          <ol className="muted small steps">
            <li>Open <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://streamelements.com/dashboard/account/channels"); }}>StreamElements → Account → Channels</a> (signed in with the channel you stream on).</li>
            <li>Press "Show secrets", copy the JWT Token and save it with 🛡 Edit securely above.</li>
          </ol>
        </>
      )}

      <h3>Streamlabs</h3>
      <label className="prop-check">
        <input type="checkbox" checked={draft.donate.streamlabs_enabled} onChange={(e) => set({ streamlabs_enabled: e.target.checked })} />
        Receive Streamlabs events (donations, merch, follows, subs, bits, raids…)
      </label>
      {draft.donate.streamlabs_enabled && (
        <>
          <div className="prop-row"><span className="prop-label">Status</span><span>{st?.streamlabs.state || "…"}</span></div>
          <SecretFields prefix="donate.streamlabs" secrets={secrets} />
          <ol className="muted small steps">
            <li>Open <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://streamlabs.com/dashboard#/settings/api-settings"); }}>Streamlabs → Settings → API Settings → API Tokens</a>.</li>
            <li>Copy "Your Socket API Token" and save it with 🛡 Edit securely above.</li>
          </ol>
        </>
      )}
      {(draft.donate.streamelements_enabled || draft.donate.streamlabs_enabled) && (
        <div className="prop-info small">
          Follows, subs, bits, raids and Super Chats that Streamtubah already reads from chat are shown once: the same event from a
          second source within 30 seconds is dropped.
        </div>
      )}
    </>
  );
}
