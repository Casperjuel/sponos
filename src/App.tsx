import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, isSpotifyLink, sameTitle, type AiStatus, type Discovery, type Group, type Household, type Item, type Me, type Member, type PlayerQueue, type PlayerState, type Queued, type QueueMode, type SpItem } from "./api";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { Mini, type NextUp } from "./Mini";
import { Overview } from "./Overview";
import { accentFrom, setAccent } from "./theme";
import { TopBar } from "./TopBar";
import { UpdatePill } from "./Updater";
import { SocialProvider, useSocialState, type SongRef } from "./Social";

const song = (i: SpItem): SongRef => ({ title: i.name, artist: i.subtitle, art: i.image });
import { Sidebar } from "./Sidebar";
import { Browse, setArtHosts } from "./Browse";
import { Queue } from "./Queue";
import { SettingsModal } from "./SettingsModal";
import type { Mood } from "./Mood";
import { RoomsPanel, type Grouping, type PanelAnchor } from "./Rooms";
import { SpotifySetup } from "./SpotifySetup";
import { About } from "./About";
import "./App.css";

type Toast = { id: number; text: string; error?: boolean };

const errText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));

export default function App() {
  const [groups, setGroups] = useState<Group[]>([]);
  const [groupId, setGroupId] = useState<string | null>(null);
  const [state, setState] = useState<PlayerState | null>(null);
  const [queue, setQueue] = useState<Item[]>([]);
  const [discoverError, setDiscoverError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  // ---- AI playlists: the Inspiration page, the last playlist it made, and whether a key is set up
  const [moodOpen, setMoodOpen] = useState<{ start: string | null } | null>(null);
  const [mood, setMood] = useState<Mood | null>(null);
  const [ai, setAi] = useState<AiStatus | null>(null);
  const [open, setOpen] = useState<SpItem | null>(null);
  const [searchNonce, setSearchNonce] = useState(0);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [me, setMe] = useState<Me | null>(null);
  const [playlists, setPlaylists] = useState<SpItem[]>([]);
  // playlists opened from pasted links, kept on this Mac: the way in for people who can't log in
  const [saved, setSaved] = useState<SpItem[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("saved") ?? "[]");
    } catch {
      return [];
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem("saved", JSON.stringify(saved));
    } catch {
      // private window etc.: it just won't be remembered
    }
  }, [saved]);
  const [spQueue, setSpQueue] = useState<PlayerQueue | null>(null);
  const fetchedAt = useRef(0);

  const toast = useCallback((text: string, error = false) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text, error }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), error ? 6000 : 2500);
  }, []);

  const group = groups.find((g) => g.id === groupId) ?? null;
  /** the rooms panel: where it opened from, or closed */
  const [roomsPanel, setRoomsPanel] = useState<(PanelAnchor & { from: string }) | null>(null);

  // ---- systems + rooms
  const [households, setHouseholds] = useState<Household[]>([]);
  const [household, setHousehold] = useState<string | null>(null);
  const householdRef = useRef<string | null>(null);

  const applyDiscovery = useCallback(
    (d: Discovery) => {
      const switched = householdRef.current !== null && householdRef.current !== d.active;
      householdRef.current = d.active;
      setHouseholds(d.households);
      setHousehold(d.active);
      setGroups(d.groups);
      setArtHosts(d.groups.flatMap((g) => g.members.map((m) => m.ip)));
      setDiscoverError(null);
      setGroupId((cur) => {
        // each system remembers its own last room
        if (switched || cur === null) cur = localStorage.getItem(`group:${d.active}`);
        if (cur && d.groups.some((g) => g.id === cur)) return cur;
        // regrouped: follow the coordinator if it still leads a group
        const prev = d.groups.find((g) => g.members.some((m) => cur?.startsWith(m.uuid)));
        return (prev ?? d.groups[0])?.id ?? null;
      });
      if (switched) {
        setOpen(null);
        toast(`Switched to ${d.households.find((h) => h.id === d.active)?.name ?? "another Sonos system"}`);
      }
    },
    [toast],
  );

  const discover = useCallback(async () => {
    try {
      applyDiscovery(await api.discover());
    } catch (e) {
      setDiscoverError(errText(e));
    }
  }, [applyDiscovery]);

  const switchHousehold = useCallback(
    (id: string) => api.setHousehold(id).then(applyDiscovery).catch((e) => toast(errText(e), true)),
    [applyDiscovery, toast],
  );

  useEffect(() => {
    discover();
    const t = setInterval(discover, 15000);
    // waking the laptop somewhere else: look again as soon as the window is used
    const onFocus = () => discover();
    window.addEventListener("focus", onFocus);
    return () => (clearInterval(t), window.removeEventListener("focus", onFocus));
  }, [discover]);

  // floorplan, system name and Spotify link shared by everyone on this system
  const [syncTick, setSyncTick] = useState(0);
  useEffect(() => {
    if (!household) return;
    const pull = () =>
      api.syncPull().then((changed) => {
        if (changed) (setSyncTick((t) => t + 1), discover());
      }).catch(() => {});
    pull();
    const t = setInterval(pull, 30000);
    window.addEventListener("focus", pull);
    return () => (clearInterval(t), window.removeEventListener("focus", pull));
  }, [household, discover]);

  useEffect(() => {
    if (groupId && household) localStorage.setItem(`group:${household}`, groupId);
  }, [groupId, household]);

  // the AI key belongs to the Sonos system
  useEffect(() => {
    if (household) api.aiStatus().then(setAi).catch(() => {});
  }, [household]);

  // ---- spotify account
  const loadAccount = useCallback(async () => {
    try {
      const m = await api.me();
      setMe(m);
      setPlaylists(m ? await api.playlists() : []);
    } catch (e) {
      setMe(null);
      setPlaylists([]);
    }
  }, []);

  useEffect(() => {
    loadAccount();
  }, [loadAccount]);

  /** log in with the saved Spotify app; throws so the setup flow can show what went wrong */
  const connectSpotify = useCallback(async () => {
    const m = await api.login();
    setMe(m);
    setPlaylists(await api.playlists());
    return m;
  }, []);
  const [setupOpen, setSetupOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const login = useCallback(() => setSetupOpen(true), []);

  const logout = useCallback(async () => {
    await api.logout();
    setMe(null);
    setPlaylists([]);
    setSpQueue(null);
  }, []);

  // ---- playback state + queue polling
  const failures = useRef(0);
  const refreshState = useCallback(async () => {
    if (!groupId) return;
    try {
      setState(await api.state(groupId));
      fetchedAt.current = Date.now();
      failures.current = 0;
    } catch {
      // a few misses in a row usually means we changed network (office → home): rediscover now
      if (++failures.current === 3) discover();
    }
  }, [groupId, discover]);

  const refreshQueue = useCallback(async () => {
    if (!groupId) return;
    try {
      setQueue(await api.queue(groupId));
    } catch {
      /* ignore */
    }
  }, [groupId]);

  useEffect(() => {
    setState(null);
    setQueue([]);
    refreshState();
    refreshQueue();
    const a = setInterval(refreshState, 1000);
    const b = setInterval(refreshQueue, 4000);
    return () => {
      clearInterval(a);
      clearInterval(b);
    };
  }, [refreshState, refreshQueue]);

  // Spotify Connect bypasses the Sonos queue — show Spotify's own queue instead
  const connect = state?.source === "Spotify Connect";
  const trackTitle = state?.track?.title;
  useEffect(() => {
    if (!connect || !me) return setSpQueue(null);
    const load = () => api.playerQueue().then(setSpQueue).catch(() => setSpQueue(null));
    load();
    const t = setInterval(load, 10000);
    return () => clearInterval(t);
  }, [connect, me, trackTitle]);

  /** run a speaker command, surface errors, then re-poll */
  const run = useCallback(
    async (fn: () => Promise<unknown>, ok?: string) => {
      try {
        await fn();
        if (ok) toast(ok);
      } catch (e) {
        toast(errText(e), true);
      }
      refreshState();
      refreshQueue();
    },
    [toast, refreshState, refreshQueue],
  );

  // ---- which rooms play together
  const [changing, setChanging] = useState<Grouping["changing"]>({});
  const live = useRef({ groups, groupId });
  live.current = { groups, groupId };

  const regroup = useCallback(
    async (join: Member[], into: Group | null, leave: Member[], done: string) => {
      if (!join.length && !leave.length) return;
      const marks = Object.fromEntries([
        ...join.map((m) => [m.uuid, "join"] as const),
        ...leave.map((m) => [m.uuid, "leave"] as const),
      ]);
      setChanging((c) => ({ ...c, ...marks }));
      // stay with the music: when the selected group's lead leaves, follow a room that keeps playing
      const cur = live.current.groups.find((g) => g.id === live.current.groupId);
      const gone = new Set(leave.map((m) => m.uuid));
      const stays = cur?.members.find((m) => !gone.has(m.uuid))?.uuid;
      const follow = cur && gone.has(cur.coordinatorUuid) && stays ? stays : cur?.coordinatorUuid;
      try {
        const next = await api.regroup(join.map((m) => m.ip), into?.coordinatorUuid ?? null, leave.map((m) => m.ip));
        setGroups(next);
        const mine = next.find((g) => g.members.some((m) => m.uuid === follow));
        if (mine) setGroupId(mine.id);
        toast(done);
      } catch (e) {
        toast(errText(e), true);
        await discover();
      } finally {
        setChanging((c) => Object.fromEntries(Object.entries(c).filter(([uuid]) => !(uuid in marks))));
        refreshState();
        refreshQueue();
      }
    },
    [toast, discover, refreshState, refreshQueue],
  );

  const grouping = useMemo<Grouping>(() => {
    const several = (rooms: Member[], one: string, many: string) =>
      rooms.length === 1 ? `${rooms[0].name} ${one}` : `${rooms.length} rooms ${many}`;
    return {
      groups,
      changing,
      add: (rooms, into) => {
        const here = new Set(into.members.map((m) => m.uuid));
        const lead = into.members.find((m) => m.uuid === into.coordinatorUuid)?.name ?? into.name;
        const fresh = rooms.filter((m) => !here.has(m.uuid));
        regroup(fresh, into, [], several(fresh, `now plays with ${lead}`, `now play with ${lead}`));
      },
      remove: (rooms) => regroup([], null, rooms, several(rooms, "plays on its own", "play on their own")),
    };
  }, [groups, changing, regroup]);

  const toggleRooms = (from: string, el: HTMLElement, side: PanelAnchor["side"]) =>
    setRoomsPanel((cur) => (cur?.from === from ? null : { from, side, rect: el.getBoundingClientRect() }));

  // the logged-in user is the one casting → their Spotify queue is the live one
  const castingMine = connect && !!spQueue && sameTitle(spQueue.current?.name, state?.track?.title);

  const report = useCallback(
    (label: string, mode: QueueMode, q: Queued) => {
      if (q.placed === "top" && mode === "next")
        toast(`${label} added to the top of the Sonos queue. This room is playing ${state?.source ?? "something else"}, so it plays once you switch to the queue.`);
      else toast(`${mode === "now" ? "Playing" : mode === "next" ? "Playing next:" : "Added"} ${label}`);
    },
    [toast, state?.source],
  );

  // ---- votes + who added what, shared with everyone on this Sonos system
  const social = useSocialState(household, me, toast);
  /** put a face on what was just queued; albums and playlists are expanded to their songs */
  const credit = useCallback(
    (item: SpItem) => {
      if (!me) return;
      if (item.kind === "track") return social.markAdded([song(item)]);
      api.children(item).then((songs) => social.markAdded(songs.map(song))).catch(() => {});
    },
    [me, social],
  );

  const enqueue = useCallback(
    (item: SpItem, mode: QueueMode) => {
      if (!groupId) return toast("Pick a room first", true);
      if (castingMine && mode !== "now" && item.kind === "track")
        return run(async () => (await api.addToSpotifyQueue(item.id), credit(item)), `Added ${item.name} to your Spotify queue`);
      run(async () => {
        report(item.name, mode, await api.queueSpotify(groupId, item, mode));
        credit(item);
      });
    },
    [groupId, run, toast, report, castingMine, credit],
  );

  const enqueueMany = useCallback(
    (items: SpItem[], mode: QueueMode) => {
      if (!groupId) return toast("Pick a room first", true);
      run(async () => {
        report(`${items.length} songs`, mode, await api.queueTracks(groupId, items, mode));
        social.markAdded(items.map(song));
      });
    },
    [groupId, run, toast, report, social],
  );

  // ---- mini player
  const [mini, setMini] = useState(() => localStorage.getItem("mini") === "1");
  const [view, setView] = useState<"player" | "overview">("player");
  const [pinned, setPinned] = useState(() => localStorage.getItem("pinned") !== "0");
  const toggleMini = useCallback((on?: boolean) => setMini((m) => on ?? !m), []);

  useEffect(() => {
    localStorage.setItem("mini", mini ? "1" : "0");
    localStorage.setItem("pinned", pinned ? "1" : "0");
    resizeFor(mini, pinned);
  }, [mini, pinned]);

  // what the queue holds after the current song, so suggestions don't repeat it
  const upcoming = useMemo(
    () => (state?.queueActive ? queue.slice(state.trackNo, state.trackNo + 15).map((t) => `${t.title}${t.artist ? ` by ${t.artist}` : ""}`) : []),
    [queue, state?.queueActive, state?.trackNo],
  );

  const nextUp: NextUp =
    state?.queueActive && queue[state.trackNo] // trackNo is 1-based, so this is the following item
      ? { title: queue[state.trackNo].title, artist: queue[state.trackNo].artist }
      : castingMine && spQueue?.queue[0]
        ? { title: spQueue.queue[0].name, artist: spQueue.queue[0].subtitle }
        : null;

  // ---- menu bar: mirror the track, take transport commands
  const playing = state?.transport === "PLAYING" || state?.transport === "TRANSITIONING";
  useEffect(() => {
    invoke("tray_update", { title: state?.track?.title || null, artist: state?.track?.artist ?? null, playing }).catch(() => {});
  }, [state?.track?.title, state?.track?.artist, playing]);

  const transport = useCallback(
    (action: "toggle" | "next" | "previous") => {
      if (!groupId) return;
      const a = action === "toggle" ? (playing ? "pause" : "play") : action;
      run(() => api.control(groupId, a));
    },
    [groupId, playing, run],
  );

  useEffect(() => {
    const un = listen<string>("tray", ({ payload }) => {
      if (payload === "mini") toggleMini(true);
      else if (payload === "settings") (toggleMini(false), setSettingsOpen(true));
      else if (payload === "about") (toggleMini(false), setAboutOpen(true));
      else if (payload === "full") toggleMini(false);
      else if (payload === "toggle" || payload === "next" || payload === "previous") transport(payload);
    });
    return () => void un.then((f) => f());
  }, [transport, toggleMini]);

  // ---- keyboard
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.shiftKey && e.key.toLowerCase() === "m") return e.preventDefault(), toggleMini();
      if (cmd && (e.key === "1" || e.key === "2")) return e.preventDefault(), setView(e.key === "1" ? "player" : "overview");
      if (cmd && e.key.toLowerCase() === "f") {
        e.preventDefault();
        if (mini) toggleMini(false);
        setTimeout(() => document.getElementById("search")?.focus(), 50);
        return;
      }
      if (typing) return;
      if (e.key === " ") return e.preventDefault(), transport("toggle");
      if (cmd && e.key === "ArrowRight") return e.preventDefault(), transport("next");
      if (cmd && e.key === "ArrowLeft") return e.preventDefault(), transport("previous");
      if (cmd && (e.key === "ArrowUp" || e.key === "ArrowDown") && groupId && state) {
        e.preventDefault();
        const v = Math.max(0, Math.min(100, state.volume + (e.key === "ArrowUp" ? 5 : -5)));
        run(() => api.setVolume(groupId, v));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [mini, toggleMini, transport, groupId, state, run]);

  // glass theme: accent colour follows the cover; play state drives the glow animations
  const art = state?.track?.art;
  useEffect(() => {
    if (!art) return;
    let live = true;
    accentFrom(art).then((c) => live && c && setAccent(c));
    return () => void (live = false);
  }, [art]);
  useEffect(() => {
    document.documentElement.dataset.playing = playing ? "1" : "0";
  }, [playing]);

  const ambient = <Ambient art={art} />;

  if (mini)
    return (
      <SocialProvider value={social}>
        {ambient}
        <Mini groups={groups} group={group} onSelect={setGroupId} state={state} nextUp={nextUp} fetchedAt={fetchedAt}
          run={run} pinned={pinned} onPin={() => setPinned((p) => !p)} onExpand={() => toggleMini(false)} />
        <div className="toasts mini-toasts">
          {toasts.slice(-1).map((t) => <div key={t.id} className={`toast ${t.error ? "error" : ""}`}>{t.text}</div>)}
        </div>
      </SocialProvider>
    );

  return (
    <SocialProvider value={social}>
      <div className="app">
        {ambient}
        <TopBar
          onMini={() => toggleMini(true)}
          group={group}
          state={state}
          fetchedAt={fetchedAt}
          run={run}
          onRooms={(el) => toggleRooms("topbar", el, "below")}
          roomsOpen={roomsPanel?.from === "topbar"}
          moodOpen={!!moodOpen && view === "player"}
          onMood={() => (setView("player"), setOpen(null), setQuery(""), setMoodOpen((m) => (m && view === "player" ? null : { start: null })))}
          query={query}
          onQuery={(q) => {
            if (q) (setView("player"), setMoodOpen(null));
            if (!isSpotifyLink(q)) return (setQuery(q), setOpen(null));
            setQuery("");
            api.resolve(q)
              .then((item) => {
                setOpen(item);
                if (item.kind !== "track") setSaved((s) => [item, ...s.filter((x) => x.id !== item.id)]);
              })
              .catch((e) => toast(errText(e), true));
          }}
          onSettings={() => setSettingsOpen(true)}
          view={view}
          onToggleView={() => setView((v) => (v === "player" ? "overview" : "player"))}
        />
        {view === "overview" ? (
          <div className="body overview-body">
            {/* keyed per system so each loads its own floorplan */}
            <Overview key={household ?? "none"} syncTick={syncTick} run={run} toast={toast} onRegrouped={discover}
              onOpenRoom={(id) => (setGroupId(id), setView("player"))} />
          </div>
        ) : (
        <div className="body">
          <Sidebar
            households={households}
            household={household}
            onHousehold={switchHousehold}
            groups={groups}
            selected={groupId}
            onSelect={setGroupId}
            grouping={grouping}
            onRooms={(id, el) => (setGroupId(id), toggleRooms(id, el, "right"))}
            onRefresh={discover}
            error={discoverError}
            me={me}
            playlists={playlists}
            saved={saved}
            onForget={(id) => setSaved((s) => s.filter((x) => x.id !== id))}
            open={open}
            onOpen={(item) => (setOpen(item), setQuery(""), setMoodOpen(null))}
            onLogin={login}
          />
          <Browse
            query={query}
            nonce={searchNonce}
            open={open}
            setOpen={setOpen}
            state={state}
            group={group}
            enqueue={enqueue}
            enqueueMany={enqueueMany}
            onSettings={() => setSettingsOpen(true)}
            onLogin={login}
            loggedIn={!!me}
            ai={ai}
            moodOpen={moodOpen}
            onMood={(start) => (setOpen(null), setMoodOpen(start === undefined ? null : { start }))}
            mood={mood}
            setMood={setMood}
            upcoming={upcoming}
          />
          <Queue group={group} state={state} queue={queue} spQueue={spQueue} castingMine={castingMine} run={run} />
        </div>
        )}
        {roomsPanel && group && (
          <RoomsPanel group={group} grouping={grouping} anchor={roomsPanel} onClose={() => setRoomsPanel(null)} />
        )}
        {settingsOpen && (
          <SettingsModal
            groupId={groupId}
            household={households.find((h) => h.id === household) ?? null}
            me={me}
            onLogin={() => (setSettingsOpen(false), login())}
            onLogout={logout}
            onClose={() => setSettingsOpen(false)}
            onSaved={() => {
              setSettingsOpen(false);
              toast("Settings saved");
              setSearchNonce((n) => n + 1); // re-run a search that failed for lack of credentials
              discover();
              loadAccount();
            }}
            toast={toast}
            ai={ai}
            setAi={setAi}
          />
        )}
        {aboutOpen && <About onClose={() => setAboutOpen(false)} />}
        {setupOpen && (
          <SpotifySetup me={me} connect={connectSpotify} onClose={() => (setSetupOpen(false), loadAccount())} />
        )}
        <UpdatePill toast={toast} />
        <div className="toasts">
          {toasts.map((t) => (
            <div key={t.id} className={`toast ${t.error ? "error" : ""}`}>
              {t.text}
            </div>
          ))}
        </div>
      </div>
    </SocialProvider>
  );
}

/**
 * Glass theme backdrop: the current cover blown up and blurred behind the UI,
 * cross-fading between tracks, plus SVG gradients the glossy icons fill with.
 * Hidden by CSS in the classic theme.
 */
function Ambient({ art }: { art?: string }) {
  const [layers, setLayers] = useState<{ src?: string; key: number }[]>([{ src: art, key: 0 }]);
  useEffect(() => {
    setLayers((l) => (l[l.length - 1].src === art ? l : [...l.slice(-1), { src: art, key: Date.now() }]));
  }, [art]);
  return (
    <>
      <div className="ambient" aria-hidden>
        <div className="ambient-blobs" />
        {layers.map((l) =>
          l.src ? <div key={l.key} className="ambient-art" style={{ backgroundImage: `url("${l.src}")` }} /> : null,
        )}
        <div className="ambient-tint" />
      </div>
      <svg className="svg-defs" aria-hidden width="0" height="0">
        <defs>
          <linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#ffffff" />
            <stop offset=".55" stopColor="#e9eaf0" />
            <stop offset="1" stopColor="#a9abb8" />
          </linearGradient>
          <linearGradient id="gloss-dark" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#3a3b44" />
            <stop offset="1" stopColor="#0d0d12" />
          </linearGradient>
        </defs>
      </svg>
    </>
  );
}

const MINI = { width: 400, height: 176 };
const FULL_MIN = { width: 1000, height: 600 };

/** Shrink the window into the tile, or restore whatever size it had before. */
async function resizeFor(mini: boolean, pinned: boolean) {
  const win = getCurrentWindow();
  try {
    if (mini) {
      const size = (await win.innerSize()).toLogical(await win.scaleFactor());
      if (size.width >= FULL_MIN.width) localStorage.setItem("fullSize", JSON.stringify({ width: size.width, height: size.height }));
      await win.setMinSize(new LogicalSize(320, MINI.height));
      await win.setSize(new LogicalSize(MINI.width, MINI.height));
      await win.setAlwaysOnTop(pinned);
    } else {
      const full = JSON.parse(localStorage.getItem("fullSize") ?? "null") ?? { width: 1320, height: 840 };
      await win.setAlwaysOnTop(false);
      await win.setMinSize(new LogicalSize(FULL_MIN.width, FULL_MIN.height));
      const now = (await win.innerSize()).toLogical(await win.scaleFactor());
      if (now.width < FULL_MIN.width) await win.setSize(new LogicalSize(full.width, full.height));
    }
  } catch (e) {
    console.error("resize failed", e);
  }
}

export type Run =(fn: () => Promise<unknown>, ok?: string) => Promise<void>;
export type ToastFn = (text: string, error?: boolean) => void;
export { errText };
