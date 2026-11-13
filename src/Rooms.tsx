import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, type Group, type GroupOverview, type Member } from "./api";
import * as Icon from "./icons";

/** Everything that changes which rooms play together goes through here (see App). */
export type Grouping = {
  groups: Group[];
  /** rooms in the middle of joining or leaving, by UUID */
  changing: Record<string, "join" | "leave">;
  /** these rooms start playing together with `into` */
  add: (rooms: Member[], into: Group) => void;
  /** these rooms go back to playing on their own */
  remove: (rooms: Member[]) => void;
};

/** where the rooms panel opens: under the top bar button, or beside a sidebar entry */
export type PanelAnchor = { rect: DOMRect; side: "below" | "right" };

const leadOf = (g: Group) => g.members.find((m) => m.uuid === g.coordinatorUuid) ?? g.members[0];
/** by name, not counting emoji in front; never by the order the speakers happen to report */
const plain = (m: Member) => m.name.replace(/^[^\p{L}\p{N}]+/u, "").toLowerCase();
const byName = (a: Member, b: Member) => plain(a).localeCompare(plain(b)) || a.uuid.localeCompare(b.uuid);

// ------------------------------------------------------------------ sidebar list

type Drag = {
  rooms: Member[];
  from: Group;
  label: string;
  x: number;
  y: number;
  /** a group id, "solo", or nothing droppable under the pointer */
  over: string | null;
};

type Props = {
  grouping: Grouping;
  selected: string | null;
  onSelect: (id: string) => void;
  /** open the rooms panel for a group, next to its entry */
  onRooms: (id: string, entry: HTMLElement) => void;
};

/**
 * The rooms, one entry per group. A group lists every room in it. Rooms are
 * grouped by dragging one onto another, taken out with the × next to them (or
 * by dragging them out), and + opens the panel with every room and its volume.
 */
export function RoomList({ grouping, selected, onSelect, onRooms }: Props) {
  const { groups, changing } = grouping;
  const several = groups.reduce((n, g) => n + g.members.length, 0) > 1;
  const [drag, setDrag] = useState<Drag | null>(null);
  const press = useRef<Omit<Drag, "over"> | null>(null);
  const moved = useRef(false);

  const onDown = (e: React.PointerEvent, rooms: Member[], from: Group, label: string) => {
    if (e.button !== 0 || !several || (e.target as HTMLElement).closest(".room-act")) return;
    press.current = { rooms, from, label, x: e.clientX, y: e.clientY };
    moved.current = false;
  };

  useEffect(() => {
    /** what the pointer is over, if dropping there would change anything */
    const target = (x: number, y: number, d: Omit<Drag, "over">) => {
      const id = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop]")?.dataset.drop ?? null;
      if (id === "solo") return d.from.members.length > 1 ? id : null;
      return id && id !== d.from.id ? id : null;
    };
    const end = () => {
      press.current = null;
      setDrag(null);
      // the click that follows a drag must not select a room
      setTimeout(() => (moved.current = false));
    };
    const move = (e: PointerEvent) => {
      const d = press.current;
      if (!d) return;
      if (!moved.current && Math.hypot(e.clientX - d.x, e.clientY - d.y) < 6) return;
      moved.current = true;
      setDrag({ ...d, x: e.clientX, y: e.clientY, over: target(e.clientX, e.clientY, d) });
    };
    const up = (e: PointerEvent) => {
      const d = press.current;
      if (!d || !moved.current) return end();
      const over = target(e.clientX, e.clientY, d);
      end();
      if (over === "solo") {
        // a whole group dropped here splits up; its lead keeps the music
        const whole = d.rooms.length === d.from.members.length;
        grouping.remove(whole ? d.rooms.filter((m) => m.uuid !== d.from.coordinatorUuid) : d.rooms);
      } else if (over) {
        const into = groups.find((g) => g.id === over);
        if (into) grouping.add(d.rooms, into);
      }
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && press.current && end();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", end);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", end);
      window.removeEventListener("keydown", key);
    };
  }, [groups, grouping]);

  const dropName = drag?.over && drag.over !== "solo" ? leadOf(groups.find((g) => g.id === drag.over)!)?.name : null;

  return (
    <>
      <ul className={`room-list ${drag ? "dragging" : ""}`}
        onClickCapture={(e) => moved.current && (e.stopPropagation(), e.preventDefault())}>
        {groups.map((g) => {
          const lead = leadOf(g);
          const rest = g.members.filter((m) => m.uuid !== lead.uuid).sort(byName);
          const grouped = rest.length > 0;
          const lifted = drag?.from.id === g.id && drag.rooms.length === g.members.length;
          return (
            <li key={g.id} data-drop={g.id}
              className={`room-item ${grouped ? "grouped" : ""} ${g.id === selected ? "active" : ""} ${drag?.over === g.id ? "drop" : ""} ${lifted ? "lifted" : ""} ${changing[lead.uuid] ? "changing" : ""}`}>
              <div className="room-line" onPointerDown={(e) => onDown(e, g.members, g, grouped ? `${lead.name} + ${rest.length}` : lead.name)}>
                <button className="room" onClick={() => onSelect(g.id)}
                  title={several ? (grouped ? "Drag onto another room to merge the groups" : "Drag onto another room to play together") : undefined}>
                  <Icon.Speaker />
                  <span className="room-name">{lead.name}</span>
                </button>
                {several && (
                  <button className="icon-btn xs room-act room-add" data-rooms-trigger
                    onClick={(e) => onRooms(g.id, e.currentTarget.closest<HTMLElement>(".room-item")!)}
                    title={grouped ? "Add or remove rooms, set each room's volume" : `Play in more rooms together with ${lead.name}`}>
                    <Icon.Plus />
                  </button>
                )}
              </div>
              {grouped && (
                <div className="room-members">
                  {rest.map((m) => (
                    <div key={m.uuid} onPointerDown={(e) => onDown(e, [m], g, m.name)}
                      className={`room-member ${changing[m.uuid] ? "changing" : ""} ${drag?.from.id === g.id && drag.rooms.length === 1 && drag.rooms[0].uuid === m.uuid ? "lifted" : ""}`}>
                      <button className="room-member-name" onClick={() => onSelect(g.id)} title="Drag out to play on its own, or onto another room">
                        {m.name}
                      </button>
                      <button className="icon-btn xs room-act room-remove" onClick={() => grouping.remove([m])}
                        disabled={!!changing[m.uuid]} title={`Stop playing in ${m.name}`}>
                        <Icon.X />
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </li>
          );
        })}
        {drag && drag.from.members.length > 1 && (
          <li data-drop="solo" className={`room-solo-drop ${drag.over === "solo" ? "drop" : ""}`}>
            {drag.rooms.length > 1 ? "Drop here to split the group up" : "Drop here to play on its own"}
          </li>
        )}
      </ul>
      {drag && createPortal(
        <div className={`room-ghost ${drag.over ? "ok" : ""}`} style={{ left: drag.x, top: drag.y }}>
          <Icon.Speaker width={14} height={14} />
          <span>{drag.label}</span>
          {drag.over && <small>{drag.over === "solo" ? (drag.rooms.length > 1 ? "split up" : "on its own") : `with ${dropName}`}</small>}
        </div>,
        document.body,
      )}
    </>
  );
}

// ------------------------------------------------------------------ rooms panel

/** Every room on the system: tick the ones that play together, each with its own volume. */
export function RoomsPanel({ group, grouping, anchor, onClose }: {
  group: Group; grouping: Grouping; anchor: PanelAnchor; onClose: () => void;
}) {
  const { groups, changing } = grouping;
  const [data, setData] = useState<GroupOverview[]>([]);
  const ref = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  // volumes and what the other rooms are playing
  const refresh = useCallback(() => api.overview().then(setData).catch(() => {}), []);
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [refresh, groups]);

  useEffect(() => {
    const away = (e: PointerEvent) => {
      const t = e.target as HTMLElement;
      // the buttons that open the panel close or move it themselves
      if (!ref.current?.contains(t) && !t.closest("[data-rooms-trigger]")) close.current();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && close.current();
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => (document.removeEventListener("pointerdown", away), document.removeEventListener("keydown", key));
  }, []);

  // keep the panel on screen next to what opened it
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const place = () => {
      const { rect, side } = anchor;
      const w = el.offsetWidth, h = el.offsetHeight, pad = 10;
      const left = side === "right" ? rect.right + 12 : rect.left;
      const top = side === "right" ? rect.top : rect.bottom + 8;
      setPos({
        left: Math.max(pad, Math.min(left, window.innerWidth - w - pad)),
        top: Math.max(pad, Math.min(top, window.innerHeight - h - pad)),
      });
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(el);
    window.addEventListener("resize", place);
    return () => (ro.disconnect(), window.removeEventListener("resize", place));
  }, [anchor]);

  const here = new Set(group.members.map((m) => m.uuid));
  const rooms = groups.flatMap((g) => g.members).sort(byName);
  const lead = leadOf(group);
  const outside = rooms.filter((r) => !here.has(r.uuid));
  const volumes = Object.assign({}, ...data.map((o) => o.volumes)) as Record<string, number>;
  const busy = rooms.some((r) => changing[r.uuid]);

  /** what a room that isn't part of this group is up to */
  const doing = (r: Member) => {
    const home = groups.find((g) => g.members.some((m) => m.uuid === r.uuid));
    const st = data.find((o) => o.group.id === home?.id)?.state;
    const title = st?.transport === "PLAYING" ? st.track?.title : undefined;
    const others = (home?.members ?? []).filter((m) => m.uuid !== r.uuid).map((m) => m.name);
    if (others.length) return `With ${others.join(", ")}${title ? ` · ${title}` : ""}`;
    return title ? `Playing ${title}` : "Not playing";
  };

  return createPortal(
    <div ref={ref} className={`popover rooms-pop ${anchor.side}`}
      style={{ position: "fixed", left: pos?.left ?? 0, top: pos?.top ?? 0, visibility: pos ? "visible" : "hidden" }}>
      <div className="rooms-head">
        <div className="room-text">
          <b>{here.size > 1 ? `${here.size} rooms playing together` : `Only ${lead.name} is playing`}</b>
          <span className="room-sub">Tick the rooms that should play the same music</span>
        </div>
        <button className="icon-btn xs" onClick={onClose} title="Close"><Icon.X /></button>
      </div>
      <div className="rooms-rows">
        {rooms.map((r) => {
          const on = here.has(r.uuid);
          const pending = !!changing[r.uuid];
          const only = on && here.size === 1;
          return (
            <div key={r.uuid} className={`room-row ${on ? "on" : ""} ${pending ? "pending" : ""}`}>
              <button className="room-toggle" disabled={only || pending}
                onClick={() => (on ? grouping.remove([r]) : grouping.add([r], group))}
                title={only ? "At least one room has to play" : on ? `Stop playing in ${r.name}` : `Also play in ${r.name}`}>
                <span className="room-check">{pending ? <i className="spinner" /> : on && <Icon.Check width={12} height={12} />}</span>
                <span className="room-text">
                  <span className="room-name">{r.name}</span>
                  {!on && <span className="room-sub">{pending ? "Joining…" : doing(r)}</span>}
                </span>
              </button>
              {on && <MemberSlider ip={r.ip} polled={volumes[r.uuid]} />}
            </div>
          );
        })}
      </div>
      {rooms.length > 1 && (
        <div className="rooms-foot">
          <button className="btn small" disabled={busy || !outside.length} onClick={() => grouping.add(outside, group)}>
            Play everywhere
          </button>
          <button className="btn small" disabled={busy || here.size < 2}
            onClick={() => grouping.remove(group.members.filter((m) => m.uuid !== lead.uuid))}
            title={`Every room plays on its own again; the music stays in ${lead.name}`}>
            Ungroup all
          </button>
        </div>
      )}
    </div>,
    document.body,
  );
}

/** One room's own volume: stays local while dragging, then follows the speaker again. */
function MemberSlider({ ip, polled }: { ip: string; polled: number | undefined }) {
  const [vol, setVol] = useState<number | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const onVol = (v: number) => {
    setVol(v);
    clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      api.setMemberVolume(ip, v).catch(() => {}).finally(() => (timer.current = window.setTimeout(() => setVol(null), 2500)));
    }, 100);
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  const shown = vol ?? polled ?? 0;
  return (
    <>
      <input type="range" className="slider" min={0} max={100} value={shown} disabled={vol == null && polled == null}
        style={{ "--p": `${shown}%` } as React.CSSProperties} onChange={(e) => onVol(Number(e.target.value))} />
      <span className="vol-num">{vol == null && polled == null ? "" : shown}</span>
    </>
  );
}
