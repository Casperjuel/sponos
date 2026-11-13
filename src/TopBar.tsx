import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { api, fmt, playModes, type Group, type PlayerState } from "./api";
import type { Run } from "./App";
import * as Icon from "./icons";

type Props = {
  group: Group | null;
  state: PlayerState | null;
  fetchedAt: MutableRefObject<number>;
  run: Run;
  /** open (or close) the rooms panel under this button */
  onRooms: (button: HTMLElement) => void;
  roomsOpen: boolean;
  /** the Inspiration page: describe the music, get a playlist */
  onMood: () => void;
  moodOpen: boolean;
  query: string;
  onQuery: (q: string) => void;
  onSettings: () => void;
  onMini: () => void;
  view: "player" | "overview";
  onToggleView: () => void;
};

/** Volume that follows the speaker, but stays local while dragging and is throttled to the speaker. */
export function useVolume(g: string | undefined, polled: number | undefined) {
  const [vol, setVol] = useState<number | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const onVol = (v: number) => {
    setVol(v);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (g) api.setVolume(g, v).finally(() => setTimeout(() => setVol(null), 1200));
    }, 120);
  };
  return [vol ?? polled ?? 0, onVol] as const;
}

/** Track position interpolated between 1s polls so progress bars move smoothly. */
export function usePosition(state: PlayerState | null, playing: boolean, fetchedAt: MutableRefObject<number>) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, []);
  const dur = state?.duration ?? 0;
  const pos = Math.min(dur || Infinity, (state?.position ?? 0) + (playing ? (now - fetchedAt.current) / 1000 : 0));
  return { pos, dur };
}

export function TopBar({ group, state, fetchedAt, run, onRooms, roomsOpen, onMood, moodOpen, query, onQuery, onSettings, onMini, view, onToggleView }: Props) {
  const g = group?.id;
  const playing = state?.transport === "PLAYING" || state?.transport === "TRANSITIONING";
  const mode = playModes.decode(state?.playMode ?? "NORMAL");
  const [shownVol, onVol] = useVolume(g, state?.volume);
  const [scrub, setScrub] = useState<number | null>(null);
  const live = usePosition(state, playing, fetchedAt);
  const dur = live.dur;
  const pos = scrub ?? live.pos;

  return (
    <header className="topbar" data-tauri-drag-region>
      <div className="tb-left" data-tauri-drag-region>
        <button className="icon-btn" disabled={!g} onClick={() => g && run(() => api.setMute(g, !state?.muted))}
          title={state?.muted ? "Unmute" : "Mute"}>
          {state?.muted ? <Icon.Mute /> : <Icon.Volume />}
        </button>
        <input type="range" className="slider vol" min={0} max={100} value={shownVol} disabled={!g}
          style={{ "--p": `${shownVol}%` } as React.CSSProperties}
          onChange={(e) => onVol(Number(e.target.value))} />
        <span className="vol-num">{shownVol}</span>
        {group && (
          <button className={`rooms-chip ${roomsOpen ? "on" : ""}`} data-rooms-trigger onClick={(e) => onRooms(e.currentTarget)}
            title="Choose which rooms play together, and each room's volume">
            <Icon.Speaker width={13} height={13} />
            {group.members.length > 1 ? `${group.members.length} rooms` : "1 room"}
            <span className="caret">▾</span>
          </button>
        )}
      </div>

      <div className="tb-center">
        <div className="transport">
          <button className={`icon-btn sm ${mode.shuffle ? "on" : ""}`} disabled={!g} title="Shuffle"
            onClick={() => g && run(() => api.setPlayMode(g, playModes.encode(!mode.shuffle, mode.repeat)))}>
            <Icon.Shuffle />
          </button>
          <button className="icon-btn lg" disabled={!g} onClick={() => g && run(() => api.control(g, "previous"))} title="Previous">
            <Icon.Prev />
          </button>
          <button className="play-btn" disabled={!g} onClick={() => g && run(() => api.control(g, playing ? "pause" : "play"))}
            title={playing ? "Pause" : "Play"}>
            {playing ? <Icon.Pause width={20} height={20} /> : <Icon.Play width={20} height={20} />}
          </button>
          <button className="icon-btn lg" disabled={!g} onClick={() => g && run(() => api.control(g, "next"))} title="Next">
            <Icon.Next />
          </button>
          <button className={`icon-btn sm ${mode.repeat !== "off" ? "on" : ""}`} disabled={!g}
            title={`Repeat: ${mode.repeat}`}
            onClick={() => {
              const next = mode.repeat === "off" ? "all" : mode.repeat === "all" ? "one" : "off";
              g && run(() => api.setPlayMode(g, playModes.encode(mode.shuffle, next)));
            }}>
            <Icon.Repeat />
            {mode.repeat === "one" && <span className="badge">1</span>}
          </button>
          <button className={`icon-btn sm ${state?.crossfade ? "on" : ""}`} disabled={!g}
            title={state?.crossfade ? "Crossfade is on" : "Crossfade is off"}
            onClick={() => g && run(() => api.setCrossfade(g, !state?.crossfade), state?.crossfade ? "Crossfade off" : "Crossfade on")}>
            <Icon.Crossfade />
          </button>
        </div>
        <div className="progress">
          <span>{fmt(pos)}</span>
          <input type="range" className="slider" min={0} max={dur || 1} value={dur ? pos : 0} disabled={!dur}
            style={{ "--p": `${dur ? (pos / dur) * 100 : 0}%` } as React.CSSProperties}
            onChange={(e) => setScrub(Number(e.target.value))}
            onPointerUp={() => {
              if (g && scrub != null) run(() => api.seek(g, Math.round(scrub))).then(() => setScrub(null));
            }} />
          <span>{dur ? fmt(dur) : "--:--"}</span>
        </div>
      </div>

      <div className="tb-right" data-tauri-drag-region>
        <label className="search">
          <Icon.Search />
          <input id="search" placeholder="Search or paste a Spotify link  ⌘F" value={query} onChange={(e) => onQuery(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && onQuery("")} spellCheck={false} />
          {query && <button className="icon-btn xs" onClick={() => onQuery("")}><Icon.X /></button>}
        </label>
        <button className={`icon-btn ${moodOpen ? "on" : ""}`} onClick={onMood} title="Inspiration: make a playlist from a sentence">
          <Icon.Sparkle />
        </button>
        <button className={`icon-btn ${view === "overview" ? "on" : ""}`} onClick={onToggleView}
          title={view === "overview" ? "Back to the player (⌘1)" : "Floorplan overview (⌘2)"}>
          {view === "overview" ? <Icon.Player /> : <Icon.Map />}
        </button>
        <button className="icon-btn" onClick={onMini} title="Mini player (⌘⇧M)"><Icon.Minimize /></button>
        <button className="icon-btn" onClick={onSettings} title="Settings"><Icon.Gear /></button>
      </div>
    </header>
  );
}
