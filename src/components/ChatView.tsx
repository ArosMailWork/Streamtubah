// Unified chat (Twitch + YouTube + Kick): read, search, mention, send and moderate.
// Shared by the chat drawer and the pop-out window.
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { AccountInfo, api, ChatAction, ChatMessage, errorText, Platform } from "../api";
import { accountsStore, chatStatusStore, chatStore, toast, useStore } from "../store";

export const PLATFORMS: { id: Platform; label: string }[] = [
  { id: "twitch", label: "Twitch" },
  { id: "youtube", label: "YouTube" },
  { id: "kick", label: "Kick" },
];

export function overlayUrl(port: number) {
  return `http://127.0.0.1:${port}/overlay/chat?fade=0&size=18`;
}

/** Only roles worth showing; platform cosmetic badges are dropped. */
const ROLE_ICONS: [RegExp, string, string][] = [
  [/broadcaster|owner/i, "🎙", "Broadcaster"],
  [/moderator/i, "🔧", "Moderator"],
  [/vip/i, "💎", "VIP"],
  [/subscriber|member|founder/i, "★", "Subscriber / member"],
];
const roleIcons = (badges: string[]) => ROLE_ICONS.filter(([re]) => badges.some((b) => re.test(b)));

/** What each platform's API lets a signed-in moderator do. */
const SUPPORTED: Record<Platform, ChatAction["kind"][]> = {
  twitch: ["timeout", "ban", "unban", "delete", "mod", "vip"],
  youtube: ["timeout", "ban", "delete", "mod"],
  kick: ["timeout", "ban", "unban", "delete"],
};

function Highlight({ text, q }: { text: string; q: string }) {
  if (!q) return <>{text}</>;
  const i = text.toLowerCase().indexOf(q.toLowerCase());
  if (i < 0) return <>{text}</>;
  return <>{text.slice(0, i)}<mark>{text.slice(i, i + q.length)}</mark>{text.slice(i + q.length)}</>;
}

function UserCard(props: {
  msg: ChatMessage;
  account: AccountInfo | undefined;
  onMention: () => void;
  onFilter: () => void;
  onClose: () => void;
}) {
  const { msg, account } = props;
  const [confirm, setConfirm] = useState<string | null>(null);
  const can = (k: ChatAction["kind"]) => !!account && SUPPORTED[msg.platform].includes(k);
  const why = (k: ChatAction["kind"]) =>
    !account ? `Sign in to ${msg.platform} (Settings → Accounts)` : !SUPPORTED[msg.platform].includes(k) ? `Not available on ${msg.platform}` : undefined;

  const run = async (action: ChatAction, label: string, dangerous = false) => {
    if (dangerous && confirm !== label) {
      setConfirm(label);
      return;
    }
    setConfirm(null);
    try {
      await api.chatAction(msg.platform, action);
      toast(`${label}: ${msg.author}`);
      props.onClose();
    } catch (e) {
      toast(errorText(e), "error");
    }
  };
  const uid = msg.author_id ?? "";
  const btn = (k: ChatAction["kind"], label: string, action: ChatAction, dangerous = false, cls = "") => (
    <button className={cls} disabled={!can(k) || (k !== "delete" && !uid) || (k === "delete" && !msg.msg_id)}
      title={why(k)} onClick={() => run(action, label, dangerous)}>
      {confirm === label ? "Click again to confirm" : label}
    </button>
  );

  return (
    <div className="user-card" onClick={(e) => e.stopPropagation()}>
      <header className="row">
        <span className={`dot ${msg.platform}`} />
        <b className="grow" style={msg.color ? { color: msg.color } : undefined}>{msg.author}</b>
        <button className="icon small" onClick={props.onClose}>✕</button>
      </header>
      <div className="muted small">{msg.platform}{msg.badges.length ? ` · ${msg.badges.slice(0, 4).join(", ")}` : ""}</div>
      <div className="user-card-grid">
        <button onClick={props.onMention}>@ Mention</button>
        <button onClick={props.onFilter}>🔍 Their messages</button>
        {btn("timeout", "Timeout 1m", { kind: "timeout", user_id: uid, seconds: 60 })}
        {btn("timeout", "Timeout 10m", { kind: "timeout", user_id: uid, seconds: 600 })}
        {btn("timeout", "Timeout 1h", { kind: "timeout", user_id: uid, seconds: 3600 })}
        {btn("delete", "Delete message", { kind: "delete", message_id: msg.msg_id ?? "" })}
        {btn("ban", "Ban", { kind: "ban", user_id: uid }, true, "danger")}
        {btn("unban", "Unban", { kind: "unban", user_id: uid })}
        {btn("mod", "🔧 Make moderator", { kind: "mod", user_id: uid }, true)}
        {btn("vip", "💎 Make VIP", { kind: "vip", user_id: uid }, true)}
      </div>
    </div>
  );
}

export function ChatView(props: { compact?: boolean; toolbar?: React.ReactNode }) {
  const messages = useStore(chatStore);
  const status = useStore(chatStatusStore);
  const accounts = useStore(accountsStore);
  const [query, setQuery] = useState("");
  const [card, setCard] = useState<ChatMessage | null>(null);
  const [text, setText] = useState("");
  const [target, setTarget] = useState<Platform | "all">("all");
  const [unseen, setUnseen] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const stick = useRef(true);
  const lastCount = useRef(messages.length);

  const q = query.trim();
  const authorFilter = q.startsWith("from:") ? q.slice(5).trim().toLowerCase() : null;
  const shown = useMemo(() => {
    if (!q) return messages;
    if (authorFilter !== null) return messages.filter((m) => m.author.toLowerCase() === authorFilter);
    const lq = q.toLowerCase();
    return messages.filter((m) => m.author.toLowerCase().includes(lq) || m.text.toLowerCase().includes(lq));
  }, [messages, q, authorFilter]);

  // Stick to the newest message unless the user scrolled up to read.
  useLayoutEffect(() => {
    const el = list.current;
    const added = Math.max(0, messages.length - lastCount.current);
    lastCount.current = messages.length;
    if (!el) return;
    if (stick.current) el.scrollTop = el.scrollHeight;
    else if (added) setUnseen((n) => n + added);
  }, [shown, messages.length]);

  const toBottom = () => {
    const el = list.current;
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
    stick.current = true;
    setUnseen(0);
  };

  const signedIn = accounts.filter((a) => status[a.platform]?.state === "connected");
  useEffect(() => {
    if (target !== "all" && !signedIn.some((a) => a.platform === target)) setTarget("all");
  }, [signedIn.length]);

  const send = async () => {
    const t = text.trim();
    if (!t) return;
    try {
      await api.chatSend(t, target === "all" ? null : target);
      setText("");
    } catch (e) {
      toast(errorText(e), "error");
    }
  };

  const active = PLATFORMS.filter((p) => status[p.id] && status[p.id]!.state !== "off");

  return (
    <div className={`chat-view ${props.compact ? "compact" : ""}`} onClick={() => setCard(null)}>
      <div className="chat-top">
        <div className="chat-status">
          {active.length === 0 && <span className="muted small">No chat connected</span>}
          {active.map((p) => (
            <span key={p.id} className={`chip-status ${status[p.id]!.state}`} title={status[p.id]!.detail}>
              <span className={`dot ${p.id}`} /> {p.label}
            </span>
          ))}
          <span className="grow" />
          {props.toolbar}
        </div>
        <div className="chat-search">
          <input placeholder="Search name or message…  (from:name)" value={query} onChange={(e) => setQuery(e.target.value)} />
          {q && <span className="muted small">{shown.length}</span>}
          {q && <button className="icon small" onClick={() => setQuery("")}>✕</button>}
        </div>
      </div>

      <div className="chat-list" ref={list}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          if (stick.current) setUnseen(0);
        }}>
        {messages.length === 0 && <div className="empty">Add your Twitch, YouTube or Kick channel in Settings → Chat.</div>}
        {shown.map((m) => (
          <div key={m.id} className={`chat-msg ${m.kind} ${card?.id === m.id ? "selected" : ""}`}>
            <span className={`dot ${m.platform}`} title={m.platform} />
            {roleIcons(m.badges).map(([, icon, title]) => <span key={title} className="chat-role" title={title}>{icon}</span>)}
            <button className="chat-author" style={m.color ? { color: m.color } : undefined}
              onClick={(e) => { e.stopPropagation(); setCard(m); }}>
              <Highlight text={m.author} q={authorFilter === null ? q : ""} />
            </button>
            {m.amount && <span className="chat-amount">{m.amount}</span>}
            <span className="chat-text"><Highlight text={m.text} q={authorFilter === null ? q : ""} /></span>
          </div>
        ))}
      </div>
      {unseen > 0 && <button className="chat-new" onClick={toBottom}>↓ {unseen} new message{unseen > 1 ? "s" : ""}</button>}

      {card && (
        <UserCard msg={card} account={accounts.find((a) => a.platform === card.platform)}
          onClose={() => setCard(null)}
          onMention={() => { setText((t) => `${t}@${card.author} `); setCard(null); input.current?.focus(); }}
          onFilter={() => { setQuery(`from:${card.author}`); setCard(null); }} />
      )}

      <div className="chat-input">
        <select value={target} onChange={(e) => setTarget(e.target.value as Platform | "all")} disabled={signedIn.length === 0}>
          <option value="all">All</option>
          {signedIn.map((a) => <option key={a.platform} value={a.platform}>{a.platform}</option>)}
        </select>
        <input ref={input} value={text} maxLength={500} disabled={signedIn.length === 0}
          placeholder={signedIn.length ? `Chat as ${signedIn.map((a) => a.display_name).join(", ")}` : "Sign in (Settings → Accounts) to chat"}
          onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && send()} />
        <button className="primary" disabled={!text.trim() || signedIn.length === 0} onClick={send}>Send</button>
      </div>
    </div>
  );
}
