import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { api, fmt, trackKey, type AiFollowUp, type AiPick, type AiPlaylist, type AiProgress, type AiStatus, type QueueMode, type SpItem } from "./api";
import { errText, type ToastFn } from "./App";
import { Art, TrackRow } from "./Browse";
import * as Icon from "./icons";

const KEYS_URL = "https://platform.claude.com/settings/keys";
const NO_KEY = "NO_AI_KEY";

const IDEAS = [
  "A Friday playlist for the office: chill, no heavy bass, for people in their early 30s",
  "Monday morning, slow start, mostly instrumental",
  "Focus music without vocals for two hours of deep work",
  "Lunch in the kitchen: warm, upbeat, a little funky",
  "Friday bar at four: feel-good songs everyone knows",
  "Danish indie from the last five years",
];

/** A playlist together with what was asked for, so it can be changed or made again. */
export type Mood = { prompt: string; playlist: AiPlaylist };

/** what Claude should know about the moment: the weekday and time, and what is on */
function context(playing?: string) {
  const now = new Date();
  const when = now.toLocaleString("en-GB", { weekday: "long", hour: "2-digit", minute: "2-digit" });
  return `Local time: ${when}${playing ? `\nPlaying right now: ${playing}` : ""}`;
}

const recent = {
  read: (): string[] => {
    try {
      return JSON.parse(localStorage.getItem("moods") ?? "[]");
    } catch {
      return [];
    }
  },
  add: (prompt: string) => {
    const next = [prompt, ...recent.read().filter((p) => p !== prompt)].slice(0, 6);
    try {
      localStorage.setItem("moods", JSON.stringify(next));
    } catch {
      /* storage unavailable: recents just won't persist */
    }
    return next;
  },
};

/** Easter egg: mention Søren anywhere and there is only one band. No AI involved. */
const SOREN = /(^|[^\p{L}])s[øöo]ren([^\p{L}]|$)/iu;
const OASIS = ["Wonderwall", "Don't Look Back in Anger", "Champagne Supernova", "Live Forever", "Stop Crying Your Heart Out"];

async function oasis(): Promise<AiPlaylist> {
  const found = await Promise.all(
    OASIS.map((title) =>
      api.search(`${title} Oasis`).then((r) => r.tracks.find((t) => /oasis/i.test(t.subtitle)) ?? null, () => null)),
  );
  const tracks = found.filter((t): t is SpItem => !!t);
  if (!tracks.length) throw "Spotify couldn't find Oasis. Søren is devastated.";
  return {
    title: "Søren's Choice",
    description: "You said Søren. There was never going to be anything but Oasis.",
    tracks,
    picks: tracks.map((t) => ({ title: t.name, artist: "Oasis" })),
    missing: OASIS.length - tracks.length,
  };
}

function PooRain({ onDone }: { onDone: () => void }) {
  useEffect(() => {
    const t = setTimeout(onDone, 7000);
    return () => clearTimeout(t);
  }, []);
  return createPortal(
    <div className="poo-rain" aria-hidden>
      {Array.from({ length: 48 }, (_, i) => (
        <span key={i} style={{
          left: `${(i * 61) % 100}%`,
          fontSize: 22 + ((i * 13) % 26),
          animationDelay: `${((i * 37) % 30) / 10}s`,
          animationDuration: `${2.6 + ((i * 7) % 14) / 10}s`,
        }}>💩</span>
      ))}
    </div>,
    document.body,
  );
}

/** a playlist on its way: what was asked, then the songs Claude named, then each one as Spotify finds it */
type Making = {
  prompt: string;
  changing: boolean;
  since: number;
  /** as far as Claude has written it; `complete` once every song is named */
  plan?: { title: string | null; description: string | null; picks: AiPick[]; complete: boolean };
  /** by position in `plan.picks`; null: Spotify doesn't have it */
  found: Record<number, SpItem | null>;
};

/** numbers requests across pages, so progress from an abandoned request is never shown */
let runs = Math.floor(Math.random() * 1e6);

type Props = {
  ai: AiStatus | null;
  /** a prompt handed over from the search box: start on it right away */
  start: string | null;
  mood: Mood | null;
  setMood: (m: Mood | null) => void;
  playing?: string;
  enqueue: (item: SpItem, mode: QueueMode) => void;
  enqueueMany: (items: SpItem[], mode: QueueMode) => void;
  onSettings: () => void;
  onClose: () => void;
};

/** Describe the music you want and get a playlist: the mood finder. */
export function Inspiration({ ai, start, mood, setMood, playing, enqueue, enqueueMany, onSettings, onClose }: Props) {
  const [text, setText] = useState("");
  const [change, setChange] = useState("");
  const [making, setMaking] = useState<Making | null>(null);
  const busy = !!making;
  const [error, setError] = useState<string | null>(null);
  const [recents, setRecents] = useState(recent.read);
  const [raining, setRaining] = useState(false);
  const run = useRef(0);

  const make = async (prompt: string, changing?: Mood) => {
    const request = prompt.trim();
    if (!request || busy) return;
    const id = (run.current = ++runs);
    setMaking({ prompt: request, changing: !!changing, since: Date.now(), found: {} });
    setError(null);
    try {
      if (SOREN.test(request)) {
        const playlist = await oasis();
        if (run.current !== id) return;
        setMood({ prompt: request, playlist });
        setChange("");
        setRaining(true);
        enqueueMany(playlist.tracks, "now");
        return;
      }
      const playlist = await api.aiPlaylist(
        request,
        context(playing),
        changing ? { prompt: changing.prompt, title: changing.playlist.title, description: changing.playlist.description, tracks: changing.playlist.picks } : null,
        id,
      );
      if (run.current !== id) return;
      setMood({ prompt: changing ? `${changing.prompt}\nThen: ${request}` : request, playlist });
      if (!changing) setRecents(recent.add(request));
      setChange("");
    } catch (e) {
      if (run.current === id) setError(errText(e));
    } finally {
      if (run.current === id) setMaking(null);
    }
  };
  const cancel = () => {
    run.current = ++runs;
    setMaking(null);
  };

  // the backend reports each step of the request this page is waiting for
  useEffect(() => {
    const un = listen<AiProgress>("ai-playlist", ({ payload: e }) => {
      if (e.run !== run.current) return;
      setMaking((m) => {
        if (!m) return m;
        if (e.step === "found") return { ...m, found: { ...m.found, [e.index]: e.item } };
        // the list normally only grows; if it was started over, what was found no longer lines up
        const same = (m.plan?.picks ?? []).every((p, i) => p.title === e.picks[i]?.title && p.artist === e.picks[i]?.artist);
        return { ...m, plan: { title: e.title, description: e.description, picks: e.picks, complete: e.complete }, found: same ? m.found : {} };
      });
    });
    return () => void un.then((f) => f());
  }, []);

  // a prompt from the search box starts once
  const started = useRef<string | null>(null);
  useEffect(() => {
    if (!start || started.current === start || !ai?.ready) return;
    started.current = start;
    setText(start);
    make(start);
  }, [start, ai?.ready]);
  // leaving the page abandons a request that is still running
  useEffect(() => () => void (run.current = ++runs), []);

  const needsKey = ai != null && (!ai.ready || error === NO_KEY);
  const p = mood?.playlist;
  const minutes = p ? Math.round(p.tracks.reduce((n, t) => n + (t.durationMs ?? 0), 0) / 60000) : 0;

  return (
    <main className="browse mood">
      {raining && <PooRain onDone={() => setRaining(false)} />}
      <div className="scroll">
        <button className="link back" onClick={onClose}><Icon.Back /> Back</button>

        {needsKey ? (
          <div className="mood-setup">
            <div className="setup-hero"><Icon.Sparkle width={28} height={28} /></div>
            <h1>Playlists from a sentence</h1>
            <p>
              Describe the music you want, like <i>“a chill Friday playlist for the office, no heavy bass”</i>, and get a
              playlist you can play right away.
            </p>
            <p className="muted">
              It needs an Anthropic API key or a Vercel AI Gateway key. One person adds it, and {ai?.shared ? "everyone on this Sonos system can use it" : "it stays on this Mac"}.
            </p>
            <button className="btn primary" onClick={onSettings}>Add a key in Settings</button>
          </div>
        ) : making ? (
          <MakingView making={making} onCancel={cancel} />
        ) : p ? (
          <>
            <div className="detail-head mood-head">
              <div className="mood-cover">
                {[0, 1, 2, 3].map((i) => <Art key={i} src={p.tracks[i % p.tracks.length]?.image} />)}
              </div>
              <div>
                <div className="eyebrow"><Icon.Sparkle width={12} height={12} /> Made for this moment</div>
                <h1>{p.title}</h1>
                <div className="sub">{p.description}</div>
                <div className="sub muted">
                  {p.tracks.length} songs{minutes > 0 && ` · ${minutes >= 60 ? `${Math.floor(minutes / 60)} h ${minutes % 60} min` : `${minutes} min`}`}
                  {p.missing > 0 && ` · ${p.missing} not on Spotify`}
                </div>
                <div className="detail-actions">
                  <button className="btn primary" onClick={() => enqueueMany(p.tracks, "now")}><Icon.Play /> Play</button>
                  <button className="btn" onClick={() => enqueueMany(p.tracks, "next")}><Icon.PlayNext /> Play next</button>
                  <button className="btn" onClick={() => enqueueMany(p.tracks, "end")}><Icon.Plus /> Add to queue</button>
                </div>
              </div>
            </div>
            <form className="mood-change" onSubmit={(e) => (e.preventDefault(), mood && make(change, mood))}>
              <Icon.Sparkle width={15} height={15} />
              <input value={change} onChange={(e) => setChange(e.target.value)} spellCheck={false}
                placeholder="Change it: more upbeat, fewer 80s songs, make it two hours…" />
              <button className="btn small" disabled={!change.trim()}>Change</button>
              <button type="button" className="btn small" onClick={() => (setMood(null), setText(""))}>Start over</button>
            </form>
            {error && <div className="setup-error">{error}</div>}
            <div className="rows">
              {p.tracks.map((t) => <TrackRow key={t.id} item={t} enqueue={enqueue} />)}
            </div>
          </>
        ) : (
          <div className="mood-ask">
            <div className="eyebrow"><Icon.Sparkle width={12} height={12} /> Inspiration</div>
            <h1>What should it sound like?</h1>
            <form onSubmit={(e) => (e.preventDefault(), make(text))}>
              <textarea autoFocus rows={3} value={text} spellCheck={false}
                placeholder="Describe the mood, the people, the moment, and what you don't want to hear."
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && (e.preventDefault(), make(text))} />
              <div className="mood-go">
                <span className="muted small">Enter to make it · Shift-Enter for a new line</span>
                <button className="btn primary" disabled={!text.trim()}><Icon.Sparkle width={15} height={15} /> Make playlist</button>
              </div>
            </form>
            {error && error !== NO_KEY && <div className="setup-error">{error}</div>}
            {recents.length > 0 && (
              <section>
                <h3>Asked before</h3>
                <div className="mood-ideas">
                  {recents.map((r) => <button key={r} className="mood-idea" onClick={() => (setText(r), make(r))}>{r}</button>)}
                </div>
              </section>
            )}
            <section>
              <h3>Ideas</h3>
              <div className="mood-ideas">
                {IDEAS.map((r) => <button key={r} className="mood-idea" onClick={() => (setText(r), make(r))}>{r}</button>)}
              </div>
            </section>
          </div>
        )}
      </div>
    </main>
  );
}

/** The wait, in the shape of the playlist it becomes: the title and songs appear
 * when Claude has named them, and each cover when Spotify has found the song. */
function MakingView({ making: m, onCancel }: { making: Making; onCancel: () => void }) {
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, []);
  const secs = Math.floor((Date.now() - m.since) / 1000);
  const picks = m.plan?.picks.length ? m.plan.picks : undefined;
  const writing = !m.plan?.complete;
  const done = Object.keys(m.found).length;
  const covers = Object.values(m.found).flatMap((t) => (t?.image ? [t.image] : []));
  const status = picks && !writing
    ? `Finding them on Spotify · ${done} of ${picks.length}`
    : picks ? `Picking songs · ${picks.length} so far`
    : secs >= 25 ? "Still picking, long lists take a little longer…"
    : m.changing ? "Reworking the list…" : "Picking songs that fit…";

  return (
    <div className="mood-making" aria-busy="true">
      <div className="detail-head mood-head">
        <div className="mood-cover">
          {[0, 1, 2, 3].map((i) => covers[i] ? <Art key={i} src={covers[i]} /> : <span key={i} className="art skel" style={{ animationDelay: `${i * -0.35}s` }} />)}
          {!picks && <div className="eq"><i /><i /><i /></div>}
        </div>
        <div className="mood-making-text">
          <div className="eyebrow"><Icon.Sparkle width={12} height={12} /> {m.changing ? "Changing your playlist" : "Making your playlist"}</div>
          {m.plan?.title ? <h1 className="reveal">{m.plan.title}</h1> : <span className="skel skel-title" />}
          {m.plan?.description ? <div className="sub reveal">{m.plan.description}</div> : (
            <>
              <span className="skel skel-line" />
              <span className="skel skel-line short" />
            </>
          )}
          <div className="mood-status">
            <span className="setup-waiting" role="status">{status}</span>
            <span className="muted">{fmt(secs)}</span>
            <button className="btn small" onClick={onCancel}>Cancel</button>
          </div>
          {picks && !writing && <div className="mood-bar"><i style={{ width: `${(done / picks.length) * 100}%` }} /></div>}
        </div>
      </div>
      <div className="mood-quote">“{m.prompt}”</div>
      <div className="rows">
        {picks
          ? picks.map((pick, i) => {
              const item = m.found[i];
              return (
                <div key={i} className={`row pick ${item === null ? "missing" : ""}`}>
                  {item ? <Art src={item.image} /> : <span className={`art ${item === null ? "placeholder" : "skel"}`} />}
                  <div className="row-text">
                    <div className="title">{item?.name ?? pick.title}</div>
                    <div className="sub">{item?.subtitle ?? pick.artist}</div>
                  </div>
                  <span className="dur">{item?.durationMs ? fmt(item.durationMs / 1000) : item === null ? "Not on Spotify" : ""}</span>
                  <span />
                </div>
              );
            })
          : null}
        {/* still writing: the next songs are on their way */}
        {writing && Array.from({ length: picks ? 2 : 8 }, (_, i) => (
          <div key={`ghost${i}`} className="row pick ghost" style={{ animationDelay: `${i * -0.12}s` }}>
            <span className="art skel" />
            <div className="row-text">
              <span className="skel skel-line" style={{ width: `${34 + ((i * 37) % 40)}%` }} />
              <span className="skel skel-line thin" style={{ width: `${18 + ((i * 23) % 22)}%` }} />
            </div>
            <span />
            <span />
          </div>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ what next

/** Suggestions already asked for, by song, kept across restarts so the same song
 * doesn't ask (or cost) again. Keyed by the song alone: that is all the answer
 * depends on, and what is already queued is filtered out when showing them. */
const KEEP_DAYS = 30;
const KEEP_SONGS = 300;
const asked = new Map<string, { at: number; list: AiFollowUp[] }>();
try {
  const old = Date.now() - KEEP_DAYS * 864e5;
  for (const [k, v] of JSON.parse(localStorage.getItem("follow-ups") ?? "[]")) if (v.at > old) asked.set(k, v);
} catch {
  /* nothing saved, or unreadable: start empty */
}
const remember = (key: string, list: AiFollowUp[] | null) => {
  asked.delete(key);
  if (list?.length) asked.set(key, { at: Date.now(), list });
  try {
    // a Map keeps insertion order, so the tail is the most recently asked
    localStorage.setItem("follow-ups", JSON.stringify([...asked].slice(-KEEP_SONGS)));
  } catch {
    /* storage unavailable: the cache just lasts until the app closes */
  }
};

/** Under the playing song: songs that would follow it well, one click from being next. */
export function FollowUps({ song, upcoming, enqueue }: {
  song: { title: string; artist?: string };
  upcoming: string[];
  enqueue: (item: SpItem, mode: QueueMode) => void;
}) {
  const key = trackKey(song.title, song.artist);
  const [list, setList] = useState<AiFollowUp[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [queued, setQueued] = useState<Set<string>>(new Set());
  const current = useRef(key);
  current.current = key;
  useEffect(() => (setList(null), setError(null), setBusy(false), setQueued(new Set())), [key]);

  const ask = async (fresh = false) => {
    const saved = !fresh && asked.get(key)?.list;
    if (saved) return setList(saved);
    setBusy(true);
    setError(null);
    try {
      const found = await api.aiFollowUps(song.title, song.artist ?? "", upcoming);
      remember(key, found);
      if (current.current === key) (setList(found), setQueued(new Set()));
    } catch (e) {
      if (current.current === key) setError(errText(e));
    } finally {
      if (current.current === key) setBusy(false);
    }
  };

  if (!list)
    return (
      <div className="follow-ask">
        <button className="chip follow-chip" disabled={busy} onClick={() => ask()}>
          {busy ? <span className="spinner" /> : <Icon.Sparkle width={13} height={13} />}
          {busy ? "Finding songs that fit…" : "What should play next?"}
        </button>
        {error && <div className="muted small">{error === NO_KEY ? "Add an AI key in Settings first." : error}</div>}
      </div>
    );

  // saved suggestions can be older than the queue: leave out what is coming up anyway
  const coming = upcoming.map((u) => u.toLowerCase());
  const shown = list.filter(({ item }) => {
    const name = item.name.toLowerCase().replace(/\s*[-(\[].*$/, "");
    return queued.has(item.id) || !coming.some((u) => u.startsWith(name));
  });

  return (
    <div className="follow-ups">
      <div className="section-head">
        <h3><Icon.Sparkle width={14} height={14} /> Good after this</h3>
        <span className="follow-links">
          <button className="link" disabled={busy} onClick={() => ask(true)}>{busy ? "Asking…" : "New ideas"}</button>
          <button className="link" onClick={() => setList(null)}>Hide</button>
        </span>
      </div>
      {shown.map(({ item, why }) => (
        <div key={item.id} className={`row ${queued.has(item.id) ? "queued" : ""}`} onDoubleClick={() => enqueue(item, "now")}>
          <Art src={item.image} />
          <div className="row-text">
            <div className="title">{item.name}</div>
            <div className="sub">{item.subtitle} · {why}</div>
          </div>
          <button className="btn small" disabled={queued.has(item.id)}
            onClick={() => (enqueue(item, "next"), setQueued((q) => new Set(q).add(item.id)))}>
            {queued.has(item.id) ? <><Icon.Check width={13} height={13} /> Up next</> : <><Icon.PlayNext width={13} height={13} /> Play next</>}
          </button>
          <button className="icon-btn sm" title="Add to end of queue" onClick={() => enqueue(item, "end")}><Icon.Plus /></button>
        </div>
      ))}
      {error && <p className="muted small">{error}</p>}
      {!shown.length && <p className="muted small">{list.length ? "These are all in the queue already." : "Nothing came back for this one."}</p>}
    </div>
  );
}

// ------------------------------------------------------------------ settings

/** The Anthropic key for this Sonos system. Saved (and shared) right away, not with the dialog. */
export function AiKeySettings({ ai, onChange, toast }: { ai: AiStatus | null; onChange: (s: AiStatus) => void; toast: ToastFn }) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  /** the webview has no confirm(): a second click confirms */
  const [askRemove, setAskRemove] = useState(false);

  const save = async (value: string, done: string) => {
    setBusy(true);
    try {
      onChange(await api.aiSetKey(value));
      setKey("");
      setEditing(false);
      toast(done);
    } catch (e) {
      toast(errText(e), true);
    } finally {
      setBusy(false);
    }
  };

  const where = ai?.shared
    ? "Shared, encrypted, with everyone on this Sonos system. Anyone who can reach these speakers can use it, so give the key a spend limit."
    : "Kept on this Mac only, because sharing is turned off below.";

  return (
    <fieldset>
      <legend>AI playlists</legend>
      <p className="muted">
        Make a playlist from a sentence and get suggestions for what to play next. This uses Claude, and needs an Anthropic API key or a Vercel AI Gateway key.
      </p>
      {ai?.ready && !editing ? (
        <div className="account">
          <span><b>A key is saved for this system.</b></span>
          <button type="button" className="btn" onClick={() => setEditing(true)}>Replace</button>
          <button type="button" className={`btn ${askRemove ? "danger" : ""}`} disabled={busy}
            onClick={() => (askRemove ? (setAskRemove(false), save("", ai.shared ? "Key removed for everyone" : "Key removed")) : setAskRemove(true))}
            onMouseLeave={() => setAskRemove(false)}>
            {askRemove ? (ai.shared ? "Remove for everyone?" : "Remove?") : "Remove"}
          </button>
        </div>
      ) : (
        <div className="copy-row">
          <input type="password" value={key} placeholder="sk-ant-… or a Vercel AI Gateway key" spellCheck={false} autoComplete="off"
            onChange={(e) => setKey(e.target.value.trim())}
            onKeyDown={(e) => e.key === "Enter" && (e.preventDefault(), key && save(key, "Key saved"))} />
          <button type="button" className="btn" disabled={busy || !key} onClick={() => save(key, "Key saved")}>
            {busy ? "Checking…" : "Save key"}
          </button>
          {editing && <button type="button" className="btn" onClick={() => (setEditing(false), setKey(""))}>Cancel</button>}
        </div>
      )}
      <p className="muted small">
        {where}{" "}
        <button type="button" className="link" onClick={() => openUrl(KEYS_URL)}>Get a key ↗</button>
      </p>
    </fieldset>
  );
}
