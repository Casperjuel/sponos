import { Fragment, useEffect, useState } from "react";
import { api, type AiStatus, type Household, type Me, type Settings, type SpotifyLink } from "./api";
import { errText, type ToastFn } from "./App";
import { AiKeySettings } from "./Mood";
import { applyGlass, applyTheme, DEFAULT_GLASS, savedGlass, savedTheme, THEMES, type GlassPrefs } from "./theme";

type Props = {
  groupId: string | null;
  household: Household | null;
  me: Me | null;
  /** opens the Spotify setup flow */
  onLogin: () => void;
  onLogout: () => Promise<void>;
  onClose: () => void;
  onSaved: () => void;
  toast: ToastFn;
  ai: AiStatus | null;
  setAi: (s: AiStatus) => void;
};

export function SettingsModal({ groupId, household, me, onLogin, onLogout, onClose, onSaved, toast, ai, setAi }: Props) {
  const [s, setS] = useState<Settings | null>(null);
  const [link, setLink] = useState<SpotifyLink | null>(null);
  const [theme, setTheme] = useState(savedTheme);
  const [sysName, setSysName] = useState(household?.name ?? "");
  const [glass, setGlass] = useState<GlassPrefs>(savedGlass);
  // live preview: every slider move restyles the app behind the dialog
  const tune = (patch: Partial<GlassPrefs>) => {
    const next = { ...glass, ...patch };
    setGlass(next);
    applyGlass(next);
  };

  useEffect(() => {
    api.getSettings().then(setS);
    if (groupId) api.spotifyLink(groupId).then(setLink).catch(() => {});
  }, [groupId]);

  if (!s) return null;
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS({ ...s, [k]: v });
  const num = (v: string) => (v.trim() === "" ? null : Number(v));

  return (
    <div className="modal-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal" onSubmit={(e) => {
        e.preventDefault();
        const rename = household && sysName.trim() !== household.name ? api.renameHousehold(household.id, sysName) : Promise.resolve();
        rename.then(() => api.saveSettings(s)).then(onSaved).catch((err) => toast(errText(err), true));
      }}>
        <h2>Settings</h2>

        <fieldset>
          <legend>Theme</legend>
          <div className="theme-grid">
            {THEMES.map((t) => (
              <button type="button" key={t.id} className={`theme-card ${theme === t.id ? "active" : ""}`}
                onClick={() => (setTheme(t.id), applyTheme(t.id))}>
                <div className={`theme-swatch ${t.id}`} />
                <b>{t.name}</b>
                <small>{t.description}</small>
              </button>
            ))}
          </div>
          {theme === "glass" && (
            <div className="glass-tune">
              {([
                ["transparency", "Transparency"],
                ["blur", "Frosting"],
                ["cover", "Cover backdrop"],
              ] as const).map(([k, label]) => (
                <Fragment key={k}>
                  <span>{label}</span>
                  <input type="range" className="slider" min={0} max={100} value={glass[k]}
                    style={{ "--p": `${glass[k]}%` } as React.CSSProperties}
                    onChange={(e) => tune({ [k]: Number(e.target.value) })} />
                  <output>{glass[k]}</output>
                </Fragment>
              ))}
              <span>Animations</span>
              <button type="button" className={`toggle ${glass.motion ? "on" : ""}`} aria-pressed={glass.motion}
                onClick={() => tune({ motion: !glass.motion })} />
              <button type="button" className="link" onClick={() => tune(DEFAULT_GLASS)}>Reset</button>
            </div>
          )}
        </fieldset>

        <fieldset>
          <legend>Spotify</legend>
          {me ? (
            <div className="account">
              {me.image && <img className="avatar" src={me.image} alt="" />}
              <span>Connected as <b>{me.name}</b></span>
              <button type="button" className="btn" onClick={onLogout}>Log out</button>
            </div>
          ) : (
            <>
              <p className="muted">Connect to see your playlists and liked songs, and to show your face on songs you add.</p>
              <div>
                <button type="button" className="btn primary" onClick={onLogin}>Connect Spotify</button>
              </div>
            </>
          )}
        </fieldset>

        <AiKeySettings ai={ai && { ...ai, shared: !s.localOnly }} onChange={setAi} toast={toast} />

        <fieldset>
          <legend>Sonos</legend>
          {household && (
            <label>
              Name of this system ({household.speakers} speakers). Floorplan and Spotify link are kept per system.
              <input value={sysName} placeholder="e.g. Office or Home" onChange={(e) => setSysName(e.target.value)} />
            </label>
          )}
          <div className="share-row">
            <span>
              Share floorplan, system name and Spotify link with others on this network
              <small className="muted">Stored encrypted. Only devices that can reach these speakers can read it.</small>
            </span>
            <button type="button" className={`toggle ${!s.localOnly ? "on" : ""}`} aria-pressed={!s.localOnly}
              onClick={() => set("localOnly", !s.localOnly)} />
          </div>
          <label>
            Speaker IPs (optional, comma separated). Use these if discovery is blocked.
            <input value={s.seedIps.join(", ")} placeholder="192.168.1.20"
              onChange={(e) => set("seedIps", e.target.value.split(/[,\s]+/).filter(Boolean))} />
          </label>
          <div className="inline">
            <label>Spotify sid<input value={s.spotifySid ?? ""} placeholder={String(link?.sid ?? "auto")} onChange={(e) => set("spotifySid", num(e.target.value))} /></label>
            <label>Spotify sn<input value={s.spotifySn ?? ""} placeholder={String(link?.sn ?? "auto")} onChange={(e) => set("spotifySn", num(e.target.value))} /></label>
          </div>
          {link && (
            <p className="muted">
              {link.detected ? "Detected" : "Not detected, using a guess:"} sid={link.sid}, sn={link.sn}
              {!link.detected && ". Play one Spotify track from the Sonos app so it shows up in the queue, then reopen this dialog."}
            </p>
          )}
        </fieldset>

        <div className="modal-actions">
          <button type="button" className="btn" onClick={onClose}>Cancel</button>
          <button type="submit" className="btn primary">Save</button>
        </div>
      </form>
    </div>
  );
}
