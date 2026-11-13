import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type Me, type Settings } from "./api";
import { errText } from "./App";
import * as Icon from "./icons";

const REDIRECT = "http://127.0.0.1:8888/callback";
const DASHBOARD = "https://developer.spotify.com/dashboard/create";
const MY_APPS = "https://developer.spotify.com/dashboard";
const isKey = (s: string) => /^[0-9a-f]{32}$/i.test(s);

type Step = "intro" | "create" | "keys" | "connect" | "done";
const STEPS: { id: Step; label: string }[] = [
  { id: "create", label: "Create app" },
  { id: "keys", label: "Paste keys" },
  { id: "connect", label: "Connect" },
];

type Props = {
  me: Me | null;
  /** logs in with whatever client ID is saved; throws on failure */
  connect: () => Promise<Me | null>;
  onClose: () => void;
};

/**
 * Guided setup for your own Spotify app. Spotify only lets 5 accounts log in
 * to an app in development mode, so everyone gets their own app: then they're
 * its owner and can use every feature.
 */
export function SpotifySetup({ me, connect, onClose }: Props) {
  const [step, setStep] = useState<Step>(me ? "done" : "intro");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [id, setId] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [who, setWho] = useState<Me | null>(me);
  /** Spotify turned the account away: it isn't one of the app's 5 allowed users */
  const [notListed, setNotListed] = useState(false);
  /** the keys were just accepted: open the browser without another click */
  const [autoLogin, setAutoLogin] = useState(false);

  useEffect(() => {
    api.getSettings().then((s) => {
      setSettings(s);
      setId(s.spotifyClientId);
      setSecret(s.spotifyClientSecret);
      // set up before: just log in again
      if (s.spotifyClientId) setStep((cur) => (cur === "intro" ? "connect" : cur));
    });
  }, []);

  useEffect(() => (setError(null), setNotListed(false)), [step]);

  const copy = () => {
    navigator.clipboard.writeText(REDIRECT);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  /** save the pair, then log in through the browser */
  const save = async (clientId: string, clientSecret: string) => {
    if (!settings) return;
    const next = { ...settings, spotifyClientId: clientId, spotifyClientSecret: clientSecret };
    await api.saveSettings(next);
    setSettings(next);
  };

  const checkKeys = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.checkSpotifyApp(id.trim(), secret.trim());
      await save(id.trim(), secret.trim());
      setStep("connect");
      setAutoLogin(true);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  /** the login in flight, so a restart can wait for it to let go of the redirect port */
  const pending = useRef<Promise<void> | null>(null);
  const login = () => (pending.current = runLogin());
  const runLogin = async () => {
    setBusy(true);
    setError(null);
    setNotListed(false);
    try {
      const m = await connect();
      if (m) {
        setWho(m);
        setStep("done");
      }
    } catch (e) {
      const msg = errText(e);
      if (/login cancelled/i.test(msg)) return;
      if (/list of 5 users/i.test(msg)) return setNotListed(true);
      setError(
        /redirect/i.test(msg)
          ? `Spotify didn't accept the redirect URI. In your app's settings, add exactly ${REDIRECT} under Redirect URIs and save.`
          : msg,
      );
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    if (!autoLogin || step !== "connect") return;
    setAutoLogin(false);
    login();
  }, [autoLogin, step]);

  /** the tab was closed or the wrong account picked: stop waiting so the login can start over */
  const cancel = () => api.cancelLogin().catch(() => {});
  const close = () => (cancel(), onClose());

  /** people on the owner's list of 5 can use the built-in app */
  const useBuiltIn = async () => {
    setBusy(true);
    try {
      await save("", settings?.spotifyClientSecret ?? "");
      setStep("connect");
      setAutoLogin(true);
    } finally {
      setBusy(false);
    }
  };

  const ownApp = !!settings?.spotifyClientId;
  // the built-in app has nothing to create or paste, so it gets no step counter
  const index = step === "connect" && !ownApp ? -1 : STEPS.findIndex((s) => s.id === step);

  return (
    <div className="modal-bg" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal setup">
        <button className="icon-btn sm setup-close" onClick={close} title="Close"><Icon.X /></button>

        {index >= 0 && (
          <ol className="setup-steps">
            {STEPS.map((s, i) => (
              <li key={s.id} className={i < index ? "done" : i === index ? "now" : ""}>
                <span>{i < index ? "✓" : i + 1}</span>
                {s.label}
              </li>
            ))}
          </ol>
        )}

        {step === "intro" && (
          <section>
            <div className="setup-hero"><Icon.Music width={28} height={28} /></div>
            <h2>Connect Spotify</h2>
            <p>
              Search and queueing already work. Connect your Spotify account to also get <b>your playlists</b>,{" "}
              <b>liked songs</b>, and your <b>face on songs you add</b>.
            </p>
            <p className="muted">How do you want to connect?</p>
            <div className="setup-choices">
              <button className="setup-choice" onClick={() => setStep("create")}>
                <b>Make my own Spotify app <span className="tag">Works for everyone</span></b>
                <small>Free and takes about 3 minutes. We walk you through it. You need Spotify Premium.</small>
                <span className="go">›</span>
              </button>
              <button className="setup-choice" disabled={busy || !settings} onClick={useBuiltIn}>
                <b>Someone added me already</b>
                <small>
                  Only if the person who runs Sponos here added your Spotify account. Spotify allows just 5 people this way.
                </small>
                <span className="go">›</span>
              </button>
            </div>
            <div className="setup-actions">
              <button className="btn" onClick={onClose}>Not now</button>
            </div>
          </section>
        )}

        {step === "create" && (
          <section>
            <h2>Create your Spotify app</h2>
            <p className="muted">
              Log in to Spotify's developer site with your normal Spotify account. The first time, Spotify asks you to accept
              its developer terms.
            </p>
            <button className="btn primary wide" onClick={() => openUrl(DASHBOARD)}>
              Open Spotify developer dashboard ↗
            </button>
            <ol className="checklist">
              <li><b>App name</b> and <b>description</b>: write anything, for example Sponos. Leave <b>Website</b> empty.</li>
              <li>
                <b>Redirect URI</b>: paste this and click <b>Add</b>
                <div className="copy-row">
                  <code>{REDIRECT}</code>
                  <button className="btn small" onClick={copy}>{copied ? "Copied" : "Copy"}</button>
                </div>
              </li>
              <li>Under "Which API/SDKs are you planning to use?", tick <b>Web API</b>.</li>
              <li>Accept the terms and click <b>Save</b>.</li>
            </ol>
            <div className="setup-actions">
              <button className="btn" onClick={() => setStep("intro")}>Back</button>
              <button className="btn primary" onClick={() => setStep("keys")}>I've created it</button>
            </div>
          </section>
        )}

        {step === "keys" && (
          <section>
            <h2>Paste your app's keys</h2>
            <p className="muted">
              In your new app, open <b>Settings</b>. Copy the <b>Client ID</b>, then click <b>View client secret</b> and copy that too.
            </p>
            <button className="link small" onClick={() => openUrl(MY_APPS)}>Closed the tab? Open your Spotify apps ↗</button>
            <label>
              <span>Client ID{isKey(id) && <span className="field-ok">✓ looks right</span>}</span>
              <input autoFocus value={id} placeholder="32 letters and numbers" spellCheck={false}
                onChange={(e) => setId(e.target.value.trim())} />
            </label>
            <label>
              <span>Client secret{isKey(secret) && <span className="field-ok">✓ looks right</span>}</span>
              <input type="password" value={secret} placeholder="32 letters and numbers" spellCheck={false}
                onChange={(e) => setSecret(e.target.value.trim())} />
            </label>
            <p className="muted small">The keys stay on this Mac. The secret lets search use your own app.</p>
            {error && <div className="setup-error">{error}</div>}
            <div className="setup-actions">
              <button className="btn" onClick={() => setStep("create")}>Back</button>
              <button className="btn primary" disabled={busy || !id} onClick={checkKeys}>
                {busy ? "Checking…" : "Continue to log in"}
              </button>
            </div>
          </section>
        )}

        {step === "connect" && (
          <section>
            <h2>Connect your account</h2>
            <p className="muted">
              Your browser opens Spotify. Click <b>Agree</b>, then come back here.
              {ownApp && <> Use the same Spotify account you made the app with.</>}
            </p>
            {busy && <div className="setup-waiting">Waiting for you to finish in the browser…</div>}
            {error && <div className="setup-error">{error}</div>}
            {notListed && (
              <div className="setup-error">
                {ownApp ? (
                  <>
                    Spotify logged you in with an account that isn't allowed to use this Spotify app. Log in with the
                    account that created the app (log out of Spotify in your browser first if it picks the wrong one),
                    or add this account under <b>User Management</b> in the app's dashboard.
                  </>
                ) : (
                  <>
                    The built-in Spotify app only has room for 5 people, and your account isn't one of them. Make your
                    own free Spotify app instead. It takes about 3 minutes.
                  </>
                )}
              </div>
            )}
            {ownApp && !busy && (
              <button className="link small" onClick={() => setStep("create")}>Set up with a different Spotify app</button>
            )}
            <div className="setup-actions">
              <button className="btn" onClick={() => (cancel(), setStep(ownApp ? "keys" : "intro"))}>Back</button>
              {busy ? (
                // closing the tab would otherwise leave this waiting for minutes
                <button className="btn" onClick={async () => (await cancel(), await pending.current, login())}>Open the login page again</button>
              ) : notListed && !ownApp ? (
                <>
                  <button className="btn" onClick={login}>Try again</button>
                  <button className="btn primary" onClick={() => setStep("create")}>Set up my own app</button>
                </>
              ) : (
                <button className="btn primary" onClick={login}>{notListed ? "Try again" : "Log in to Spotify"}</button>
              )}
            </div>
          </section>
        )}

        {step === "done" && (
          <section className="setup-done">
            {who?.image ? <img className="avatar xl" src={who.image} alt="" /> : <div className="setup-hero">✓</div>}
            <h2>You're all set{who ? `, ${who.name.split(" ")[0]}` : ""}</h2>
            <p className="muted">Your playlists and liked songs are in the sidebar. Songs you add show your face to others in the queue.</p>
            <div className="setup-actions">
              <button className="btn primary" onClick={onClose}>Done</button>
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
