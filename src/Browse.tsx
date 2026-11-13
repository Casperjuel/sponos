import { useEffect, useRef, useState } from "react";
import { api, fmt, type AiStatus, type Group, type PlayerState, type QueueMode, type SearchResult, type SpItem } from "./api";
import { errText } from "./App";
import { FollowUps, Inspiration, type Mood } from "./Mood";
import { AddedByLine, Reactions, VoteButtons } from "./Social";
import * as Icon from "./icons";

type Props = {
  query: string;
  nonce: number;
  open: SpItem | null;
  setOpen: (item: SpItem | null) => void;
  state: PlayerState | null;
  group: Group | null;
  enqueue: (item: SpItem, mode: QueueMode) => void;
  enqueueMany: (items: SpItem[], mode: QueueMode) => void;
  onSettings: () => void;
  onLogin: () => void;
  loggedIn: boolean;
  ai: AiStatus | null;
  /** the Inspiration page is open; `start` is a prompt handed over from the search box */
  moodOpen: { start: string | null } | null;
  /** open Inspiration (with a prompt to start on, or null for the empty page); undefined closes it */
  onMood: (start?: string | null) => void;
  mood: Mood | null;
  setMood: (m: Mood | null) => void;
  /** what the queue holds after the current song */
  upcoming: string[];
};

type Tab = "all" | "tracks" | "albums" | "artists" | "playlists";

export function Browse({ query, nonce, open, setOpen, state, group, enqueue, enqueueMany, onSettings, onLogin, loggedIn, ai, moodOpen, onMood, mood, setMood, upcoming }: Props) {
  const [results, setResults] = useState<SearchResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<Tab>("all");
  // drill-ins opened from search results go "back" to the results, library ones to now playing
  const [fromSearch, setFromSearch] = useState(false);

  useEffect(() => {
    const q = query.trim();
    if (!q) return setResults(null);
    setLoading(true);
    let live = true;
    const t = setTimeout(() => {
      api.search(q)
        .then((r) => live && (setResults(r), setError(null)))
        .catch((e) => live && setError(errText(e)))
        .finally(() => live && setLoading(false));
    }, 300);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [query, nonce]);

  if (open)
    return (
      <Detail item={open} onBack={() => setOpen(null)} backLabel={fromSearch && query ? "Back to results" : "Back"}
        enqueue={enqueue} enqueueMany={enqueueMany} onLogin={onLogin} loggedIn={loggedIn} />
    );

  if (moodOpen) {
    const t = state?.track;
    return (
      <Inspiration ai={ai} start={moodOpen.start} mood={mood} setMood={setMood}
        playing={t?.title ? `${t.title}${t.artist ? ` by ${t.artist}` : ""}` : undefined}
        enqueue={enqueue} enqueueMany={enqueueMany} onSettings={onSettings} onClose={() => onMood()} />
    );
  }

  if (!query.trim()) return <NowPlaying state={state} group={group} ai={ai} upcoming={upcoming} enqueue={enqueue} />;

  const r = results;
  const section = (key: Exclude<Tab, "all">, title: string, items: SpItem[] | undefined, grid: boolean) => {
    if (!items?.length) return null;
    const shown = tab === "all" ? items.slice(0, grid ? 6 : 5) : items;
    return (
      <section key={key}>
        {tab === "all" && (
          <div className="section-head">
            <h3>{title}</h3>
            <button className="link" onClick={() => setTab(key)}>Show all</button>
          </div>
        )}
        {grid ? (
          <div className="grid">
            {shown.map((it) => <Card key={it.id} item={it} onOpen={() => (setFromSearch(true), setOpen(it))} enqueue={enqueue} />)}
          </div>
        ) : (
          <div className="rows">
            {shown.map((it) => <TrackRow key={it.id} item={it} enqueue={enqueue} />)}
          </div>
        )}
      </section>
    );
  };

  return (
    <main className="browse">
      <nav className="tabs">
        {(["all", "tracks", "albums", "artists", "playlists"] as Tab[]).map((t) => (
          <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        ))}
        {loading && <span className="spinner" />}
      </nav>
      <div className="scroll">
        {ai && query.trim().length > 2 && (
          <button className="mood-cta" onClick={() => onMood(query.trim())}>
            <Icon.Sparkle />
            <span className="row-text">
              <span className="title">Make a playlist for “{query.trim()}”</span>
              <span className="sub">Describe a mood, a moment or the people listening, and get songs that fit</span>
            </span>
            <span className="go">›</span>
          </button>
        )}
        {error && (
          <div className="empty">
            <p>{error}</p>
            {/settings/i.test(error) && <button className="btn" onClick={onSettings}>Open settings</button>}
          </div>
        )}
        {r && !error && (
          <>
            {(tab === "all" || tab === "tracks") && section("tracks", "Songs", r.tracks, false)}
            {(tab === "all" || tab === "artists") && section("artists", "Artists", r.artists, true)}
            {(tab === "all" || tab === "albums") && section("albums", "Albums", r.albums, true)}
            {(tab === "all" || tab === "playlists") && section("playlists", "Playlists", r.playlists, true)}
          </>
        )}
      </div>
    </main>
  );
}

function Actions({ item, enqueue }: { item: SpItem; enqueue: Props["enqueue"] }) {
  return (
    <div className="actions" onClick={(e) => e.stopPropagation()}>
      <button className="icon-btn sm" title="Play now" onClick={() => enqueue(item, "now")}><Icon.Play /></button>
      <button className="icon-btn sm" title="Play next" onClick={() => enqueue(item, "next")}><Icon.PlayNext /></button>
      <button className="icon-btn sm" title="Add to end of queue" onClick={() => enqueue(item, "end")}><Icon.Plus /></button>
    </div>
  );
}

export function TrackRow({ item, enqueue, index }: { item: SpItem; enqueue: Props["enqueue"]; index?: number }) {
  return (
    <div className="row" onDoubleClick={() => enqueue(item, "now")} title="Double-click to play now">
      {index != null ? <span className="idx">{index}</span> : <Art src={item.image} />}
      <div className="row-text">
        <div className="title">{item.name}</div>
        <div className="sub">{item.subtitle}</div>
      </div>
      <span className="dur">{item.durationMs ? fmt(item.durationMs / 1000) : ""}</span>
      <Actions item={item} enqueue={enqueue} />
    </div>
  );
}

function Card({ item, onOpen, enqueue }: { item: SpItem; onOpen: () => void; enqueue: Props["enqueue"] }) {
  return (
    <div className={`card ${item.kind}`} onClick={onOpen}>
      <div className="card-art">
        <Art src={item.image} />
        <Actions item={item} enqueue={enqueue} />
      </div>
      <div className="title">{item.name}</div>
      <div className="sub">{item.subtitle}</div>
    </div>
  );
}

type DetailProps = {
  item: SpItem;
  onBack: () => void;
  backLabel: string;
  enqueue: Props["enqueue"];
  enqueueMany: Props["enqueueMany"];
  onLogin: () => void;
  loggedIn: boolean;
};

function Detail({ item, onBack, backLabel, enqueue, enqueueMany, onLogin, loggedIn }: DetailProps) {
  const [tracks, setTracks] = useState<SpItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setTracks(null);
    setError(null);
    api.children(item).then(setTracks).catch((e) => setError(errText(e)));
  }, [item, loggedIn]);

  // Liked songs has no Sonos container, so queue the loaded tracks one by one.
  // Everything else is queued as a whole container and Sonos expands it.
  const liked = item.kind === "liked";
  const queueAll = (mode: QueueMode) => (liked ? tracks?.length && enqueueMany(tracks, mode) : enqueue(item, mode));
  const label = item.kind === "artist" ? "top tracks" : liked ? "all" : item.kind;
  const needsLogin = !loggedIn && /log in/i.test(error ?? "");

  return (
    <main className="browse">
      <div className="scroll">
        <button className="link back" onClick={onBack}><Icon.Back /> {backLabel}</button>
        <div className={`detail-head ${item.kind}`}>
          {liked ? <div className="art liked"><Icon.Heart width={56} height={56} /></div> : <Art src={item.image} />}
          <div>
            <div className="eyebrow">{liked ? "Library" : item.kind}</div>
            <h1>{item.name}</h1>
            <div className="sub">{tracks ? `${item.subtitle} · ${tracks.length} songs` : item.subtitle}</div>
            <div className="detail-actions">
              <button className="btn primary" disabled={liked && !tracks?.length} onClick={() => queueAll("now")}><Icon.Play /> Play {label}</button>
              <button className="btn" disabled={liked && !tracks?.length} onClick={() => queueAll("next")}><Icon.PlayNext /> Play next</button>
              <button className="btn" disabled={liked && !tracks?.length} onClick={() => queueAll("end")}><Icon.Plus /> Add to queue</button>
            </div>
          </div>
        </div>
        {needsLogin && (
          <div className="notice inline-notice">
            Spotify only lists a playlist's songs to a logged-in owner or collaborator. You can still queue the whole playlist with the buttons above.
            <button className="btn" onClick={onLogin}>Log in to Spotify</button>
          </div>
        )}
        {error === "NOT_OWNER" && (
          <p className="muted pad">
            Spotify won't share this playlist's songs (it may be private). Play it or add it to the queue anyway and Sonos will load the whole thing.
          </p>
        )}
        {error && error !== "NOT_OWNER" && !needsLogin && (
          <p className="muted pad">
            Couldn't list the songs ({error}).{!liked && ` You can still queue the whole ${label}.`}
          </p>
        )}
        {!tracks && !error && <span className="spinner" />}
        <div className="rows">
          {tracks?.map((t, i) => (
            <TrackRow key={t.id + i} item={t} enqueue={enqueue} index={item.kind === "album" ? i + 1 : undefined} />
          ))}
        </div>
      </div>
    </main>
  );
}

function NowPlaying({ state, group, ai, upcoming, enqueue }: {
  state: PlayerState | null; group: Group | null; ai: AiStatus | null; upcoming: string[]; enqueue: Props["enqueue"];
}) {
  const t = state?.track;
  return (
    <main className="browse now">
      {t?.title ? (
        <div className="now-inner">
          {/* stays mounted, so it can play the record-change animation */}
          {/* the cover takes the height the text under it leaves over */}
          <div className="now-art-slot">
            <Vinyl id={`${t.title}|${t.artist}`} src={t.art} playing={state?.transport === "PLAYING"} />
          </div>
          {/* keyed on the track so a new song replays the text's entrance animation */}
          <div className="now-text" key={t.title}>
            <h1>{t.title}</h1>
            <div className="sub">{[t.artist, t.album].filter(Boolean).join(" — ")}</div>
            <div className="muted">
              {group?.name}
              {state?.source && <span className="chip">{state.source}</span>}
            </div>
            <VoteButtons song={t} />
            <Reactions song={t} />
            <AddedByLine song={t} />
          </div>
          {ai?.ready && <FollowUps song={t} upcoming={upcoming} enqueue={enqueue} />}
        </div>
      ) : (
        <div className="empty">
          <Icon.Music width={40} height={40} />
          <p>{group ? `Nothing playing in ${group.name}` : "Pick a room"}</p>
          <p className="muted">Search Spotify above to add something.</p>
        </div>
      )}
    </main>
  );
}

type Phase = "in" | "tuck" | "out" | "enter" | "settle";

/**
 * The cover as a record sleeve; the record slides out and spins while playing.
 * A new song plays a little routine: the record tucks into its sleeve, the sleeve
 * leaves to the left, the new one slides in from the right, then its record comes out.
 */
function Vinyl({ id, src, playing }: { id: string; src?: string; playing: boolean }) {
  const [shown, setShown] = useState({ id, src });
  const [phase, setPhase] = useState<Phase>("in");

  useEffect(() => {
    if (id === shown.id) {
      if (src !== shown.src) setShown({ id, src }); // same song, art just arrived
      return;
    }
    const timers: number[] = [];
    const at = (ms: number, fn: () => void) => timers.push(window.setTimeout(fn, ms));
    setPhase("tuck");
    at(380, () => setPhase("out"));
    at(760, () => (setShown({ id, src }), setPhase("enter")));
    at(800, () => setPhase("settle")); // a frame later, so it animates in from the right
    at(1250, () => setPhase("in"));
    return () => timers.forEach(clearTimeout);
  }, [id, src]);

  const spinning = playing && phase === "in";
  return (
    <div className={`now-art vinyl ${spinning ? "spinning" : ""} ${phase}`}>
      <div className="record">
        <div className="record-grooves">
          <div className="record-label"><Art src={shown.src} /></div>
        </div>
        <div className="record-sheen" />
      </div>
      <div className="sleeve"><Art src={shown.src} /></div>
    </div>
  );
}

/**
 * Speakers serve the covers of what they play, and a busy one can leave the
 * request hanging forever. Any speaker on the system can serve the same cover,
 * so a slow one is skipped for a while and the others are asked instead.
 */
const SPEAKER_ART = /^http:\/\/([\d.]+):1400(\/getaa.*)$/;
const ART_WAIT = 4000;
const SKIP_FOR = 10 * 60_000;
let artHosts: string[] = [];
const slowUntil = new Map<string, number>();
export const setArtHosts = (ips: string[]) => (artHosts = ips);

function artSources(src: string) {
  const m = SPEAKER_ART.exec(src);
  if (!m) return [src];
  const hosts = [m[1], ...artHosts.filter((h) => h !== m[1])];
  const fine = hosts.filter((h) => (slowUntil.get(h) ?? 0) < Date.now());
  return (fine.length ? fine : hosts).slice(0, 4).map((h) => `http://${h}:1400${m[2]}`);
}

export function Art({ src }: { src?: string }) {
  const [sources, setSources] = useState(() => (src ? artSources(src) : []));
  const [n, setN] = useState(0);
  const img = useRef<HTMLImageElement>(null);
  useEffect(() => (setSources(src ? artSources(src) : []), setN(0)), [src]);
  const url = sources[n];

  // an image that neither loads nor fails: give up on that speaker and ask the next
  useEffect(() => {
    if (!url || sources.length < 2) return;
    const t = setInterval(() => {
      const el = img.current;
      if (!el || el.complete) return clearInterval(t);
      const r = el.getBoundingClientRect();
      if (r.bottom < 0 || r.top > window.innerHeight) return; // lazy and off screen: not asked for yet
      const host = SPEAKER_ART.exec(url)?.[1];
      if (host) slowUntil.set(host, Date.now() + SKIP_FOR);
      setN((i) => i + 1);
    }, ART_WAIT);
    return () => clearInterval(t);
  }, [url, sources.length]);

  return url ? (
    <img ref={img} key={url} className="art" src={url} alt="" loading="lazy" onError={() => setN((i) => i + 1)} />
  ) : (
    <div className="art placeholder"><Icon.Music /></div>
  );
}
