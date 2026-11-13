import { LIKED, type Group, type Household, type Me, type SpItem } from "./api";
import { Art } from "./Browse";
import { RoomList, type Grouping } from "./Rooms";
import * as Icon from "./icons";

type Props = {
  households: Household[];
  household: string | null;
  onHousehold: (id: string) => void;
  groups: Group[];
  selected: string | null;
  onSelect: (id: string) => void;
  grouping: Grouping;
  /** open the rooms panel for a group, next to its entry */
  onRooms: (id: string, entry: HTMLElement) => void;
  onRefresh: () => void;
  error: string | null;
  me: Me | null;
  playlists: SpItem[];
  /** opened from pasted links; kept on this Mac */
  saved: SpItem[];
  onForget: (id: string) => void;
  open: SpItem | null;
  onOpen: (item: SpItem) => void;
  onLogin: () => void;
};

export function Sidebar({ households, household, onHousehold, groups, selected, onSelect, grouping, onRooms, onRefresh, error, me, playlists, saved, onForget, open, onOpen, onLogin }: Props) {
  const current = households.find((h) => h.id === household);
  return (
    <aside className="sidebar">
      <div className="panel-head">
        {households.length > 1 ? (
          // two systems on one network: let the user pick
          <select className="system-pick" value={household ?? ""} onChange={(e) => onHousehold(e.target.value)} title="Sonos system">
            {households.map((h) => <option key={h.id} value={h.id}>{h.name}</option>)}
          </select>
        ) : (
          <h2 title={current ? `${current.name} · ${current.speakers} speakers` : undefined}>{current?.name ?? "Rooms"}</h2>
        )}
        <button className="icon-btn xs" onClick={onRefresh} title="Rediscover"><Icon.Refresh /></button>
      </div>
      <RoomList grouping={grouping} selected={selected} onSelect={onSelect} onRooms={onRooms} />
      {!groups.length && <p className="muted pad">{error ?? "Looking for speakers…"}</p>}

      <div className="panel-head library-head">
        <h2>Your library</h2>
        {me?.image && <img className="avatar" src={me.image} alt="" title={me.name} />}
      </div>
      {(() => {
        const ids = new Set(playlists.map((p) => p.id));
        const extra = saved.filter((p) => !ids.has(p.id));
        const items = me ? [LIKED, ...playlists, ...extra] : extra;
        return (
          <>
            {!me && (
              <div className="pad">
                <p className="muted small">
                  Connect Spotify to see your playlists and liked songs, or paste a playlist link in the search box.
                </p>
                <button className="btn primary" onClick={onLogin}>Connect Spotify</button>
              </div>
            )}
            <ul className="lib-list">
              {items.map((p) => (
                <li key={p.kind + p.id} className="lib-row">
                  <button className={`lib-item ${open?.id === p.id ? "active" : ""}`} onClick={() => onOpen(p)}>
                    {p.kind === "liked" ? <div className="art liked"><Icon.Heart /></div> : <Art src={p.image} />}
                    <span className="room-text">
                      <span className="room-name">{p.name}</span>
                      <span className="room-sub">{p.subtitle}</span>
                    </span>
                  </button>
                  {extra.includes(p) && (
                    <button className="icon-btn xs forget" title="Remove from library" onClick={() => onForget(p.id)}>
                      <Icon.X />
                    </button>
                  )}
                </li>
              ))}
            </ul>
          </>
        );
      })()}
    </aside>
  );
}
