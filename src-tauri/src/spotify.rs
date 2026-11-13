//! Spotify Web API.
//!
//! Two kinds of token:
//! - client credentials (ID + secret) — catalogue search only, no login
//! - user login (authorization code + PKCE) — your own playlists and liked songs
//!
//! Sonos does the actual playback through its own linked Spotify account, so
//! none of this needs streaming scopes.
//!
//! Development-mode apps created after Feb 2026 are limited: search returns at
//! most 10 per type, artist top-tracks is gone, and playlist contents are only
//! returned for playlists the logged-in user owns or collaborates on.

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};

use crate::sonos::Res;

pub const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const SCOPES: &str =
    "playlist-read-private playlist-read-collaborative user-library-read user-read-playback-state user-modify-playback-state";
const B64URL: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

type Cached = Mutex<Option<(String, Instant)>>;

pub struct Spotify {
    app_token: Cached,
    user_token: Cached,
    /// refresh token lives in its own file so saving settings can't clobber it
    refresh_path: PathBuf,
    /// wakes a browser login that is still waiting, so it can be given up or started over
    login_cancel: tokio::sync::Notify,
}

pub struct Creds<'a> {
    pub id: &'a str,
    pub secret: &'a str,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpItem {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub image: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Default, Debug)]
pub struct SearchResult {
    pub tracks: Vec<SpItem>,
    pub albums: Vec<SpItem>,
    pub artists: Vec<SpItem>,
    pub playlists: Vec<SpItem>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub name: String,
    pub image: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct PlayerQueue {
    pub current: Option<SpItem>,
    pub queue: Vec<SpItem>,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    refresh_token: String,
}

impl Spotify {
    pub fn new(refresh_path: PathBuf) -> Self {
        Self {
            app_token: Default::default(),
            user_token: Default::default(),
            refresh_path,
            login_cancel: Default::default(),
        }
    }

    /// Stop waiting for the browser (the tab was closed, or the wrong account was picked).
    pub fn cancel_login(&self) {
        self.login_cancel.notify_waiters();
    }

    pub async fn reset_app_token(&self) {
        *self.app_token.lock().await = None;
    }

    fn refresh_token(&self) -> Option<String> {
        let s: Stored =
            serde_json::from_str(&std::fs::read_to_string(&self.refresh_path).ok()?).ok()?;
        Some(s.refresh_token).filter(|t| !t.is_empty())
    }

    fn store_refresh(&self, token: Option<&str>) -> Res<()> {
        match token {
            Some(t) => {
                let json = serde_json::to_string(&Stored {
                    refresh_token: t.into(),
                })
                .unwrap();
                std::fs::write(&self.refresh_path, json).map_err(|e| e.to_string())
            }
            None => {
                let _ = std::fs::remove_file(&self.refresh_path);
                Ok(())
            }
        }
    }

    pub fn logged_in(&self) -> bool {
        self.refresh_token().is_some()
    }

    pub async fn logout(&self) -> Res<()> {
        *self.user_token.lock().await = None;
        self.store_refresh(None)
    }

    // -------------------------------------------------------------- tokens

    async fn token_request(
        http: &reqwest::Client,
        body: String,
        basic: Option<&Creds<'_>>,
    ) -> Res<Value> {
        let mut req = http
            .post("https://accounts.spotify.com/api/token")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body);
        if let Some(c) = basic {
            let b =
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", c.id, c.secret));
            req = req.header("Authorization", format!("Basic {b}"));
        }
        let v: Value = req
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if v["access_token"].is_null() {
            return Err(format!(
                "Spotify auth failed: {}",
                v["error_description"]
                    .as_str()
                    .or(v["error"].as_str())
                    .unwrap_or("unknown error")
            ));
        }
        Ok(v)
    }

    fn cache(slot: &mut Option<(String, Instant)>, v: &Value) -> String {
        let t = v["access_token"].as_str().unwrap_or_default().to_string();
        let ttl = v["expires_in"].as_u64().unwrap_or(3600).saturating_sub(60);
        *slot = Some((t.clone(), Instant::now() + Duration::from_secs(ttl)));
        t
    }

    async fn app_token(&self, http: &reqwest::Client, c: &Creds<'_>) -> Res<String> {
        let mut slot = self.app_token.lock().await;
        if let Some((t, exp)) = slot.as_ref().filter(|(_, e)| Instant::now() < *e) {
            let _ = exp;
            return Ok(t.clone());
        }
        // no secret in the app: the sync service makes catalogue tokens for us
        let v = if c.secret.is_empty() {
            crate::sync::spotify_token(http).await.map_err(|e| format!("Spotify search is unavailable right now ({e})"))?
        } else {
            Self::token_request(http, "grant_type=client_credentials".into(), Some(c)).await?
        };
        Ok(Self::cache(&mut slot, &v))
    }

    /// `None` when not logged in.
    async fn user_token(&self, http: &reqwest::Client, c: &Creds<'_>) -> Res<Option<String>> {
        let mut slot = self.user_token.lock().await;
        if let Some((t, _)) = slot.as_ref().filter(|(_, e)| Instant::now() < *e) {
            return Ok(Some(t.clone()));
        }
        let Some(refresh) = self.refresh_token() else {
            return Ok(None);
        };
        let body = format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}",
            urlencoding::encode(&refresh),
            urlencoding::encode(c.id)
        );
        let v = match Self::token_request(http, body, None).await {
            Ok(v) => v,
            Err(e) => {
                // revoked or expired login — forget it so the UI offers a fresh one
                if e.contains("invalid_grant")
                    || e.contains("revoked")
                    || e.contains("Invalid refresh")
                {
                    self.store_refresh(None)?;
                }
                return Err(e);
            }
        };
        // PKCE refresh tokens rotate
        if let Some(r) = v["refresh_token"].as_str() {
            self.store_refresh(Some(r))?;
        }
        Ok(Some(Self::cache(&mut slot, &v)))
    }

    /// Browser login. Opens the consent page via `open`, waits for Spotify to
    /// redirect back to the loopback listener, then swaps the code for tokens.
    pub async fn login(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        open: impl FnOnce(String) -> Res<()>,
    ) -> Res<()> {
        if c.id.is_empty() {
            return Err("Add your Spotify client ID in Settings first".into());
        }
        let verifier = random_string(64);
        let challenge = B64URL.encode(sha2::Sha256::digest(verifier.as_bytes()));
        let state = random_string(16);

        let listener = TcpListener::bind("127.0.0.1:8888").await.map_err(|e| {
            format!("Couldn't listen on 127.0.0.1:8888 for the login redirect: {e}")
        })?;

        open(format!(
            "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&state={state}&code_challenge_method=S256&code_challenge={challenge}",
            urlencoding::encode(c.id),
            urlencoding::encode(SCOPES),
            urlencoding::encode(REDIRECT_URI),
        ))?;

        let wait = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("");
                if !path.starts_with("/callback") {
                    // favicon etc.
                    let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                    continue;
                }
                let q = query_params(path);
                let result = match (q.get("code"), q.get("error")) {
                    _ if q.get("state").map(String::as_str) != Some(state.as_str()) => Err("Login state mismatch".to_string()),
                    (Some(code), _) => Ok(code.clone()),
                    (_, Some(err)) => Err(format!("Spotify login cancelled ({err})")),
                    _ => Err("Spotify didn't return a code".to_string()),
                };
                let msg = if result.is_ok() { "Logged in. You can close this tab and go back to Sponos." } else { "Login failed. Check the app for details." };
                let html = format!(
                    "<!doctype html><meta charset=utf-8><title>Sponos</title><body style=\"font:16px -apple-system,sans-serif;background:#121212;color:#eee;display:grid;place-items:center;height:100vh;margin:0\"><p>{msg}</p>"
                );
                let _ = sock
                    .write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()).as_bytes())
                    .await;
                return result;
            }
        });
        let code = tokio::select! {
            r = wait => r.map_err(|_| "Login timed out".to_string())??,
            _ = self.login_cancel.notified() => return Err(LOGIN_CANCELLED.into()),
        };

        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={verifier}",
            urlencoding::encode(&code),
            urlencoding::encode(REDIRECT_URI),
            urlencoding::encode(c.id),
        );
        let v = Self::token_request(http, body, None).await?;
        self.store_refresh(v["refresh_token"].as_str())?;
        Self::cache(&mut *self.user_token.lock().await, &v);
        Ok(())
    }

    // -------------------------------------------------------------- requests

    async fn get(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        path: &str,
        need_user: bool,
    ) -> Res<Value> {
        let token = match self.user_token(http, c).await {
            Ok(Some(t)) => t,
            Ok(None) | Err(_) if need_user => {
                return Err("Log in to Spotify in Settings to see your library".into())
            }
            _ => self.app_token(http, c).await?,
        };
        let res = http
            .get(format!("https://api.spotify.com/v1{path}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            if status.as_u16() == 401 {
                *self.app_token.lock().await = None;
                *self.user_token.lock().await = None;
            }
            // development-mode apps only let allow-listed accounts in (plain-text body)
            if status.as_u16() == 403 && text.to_lowercase().contains("registered") {
                return Err(NOT_REGISTERED.into());
            }
            return Err(format!(
                "Spotify {status}: {}",
                v["error"]["message"].as_str().unwrap_or("")
            ));
        }
        Ok(v)
    }

    pub async fn me(&self, http: &reqwest::Client, c: &Creds<'_>) -> Res<Option<Me>> {
        if !self.logged_in() {
            return Ok(None);
        }
        let v = match self.get(http, c, "/me", true).await {
            Ok(v) => v,
            Err(e) if e == NOT_REGISTERED => {
                // don't stay half logged in: search would use this token and fail too
                self.logout().await?;
                return Err(e);
            }
            Err(e) => return Err(e),
        };
        Ok(Some(Me {
            name: v["display_name"]
                .as_str()
                .or(v["id"].as_str())
                .unwrap_or("Spotify")
                .into(),
            image: image(&v),
        }))
    }

    /// The song Claude named, as Spotify has it. `None` when Spotify doesn't know it.
    pub async fn find_track(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        title: &str,
        artist: &str,
        market: &str,
    ) -> Res<Option<SpItem>> {
        // exact fields first; a plain search catches odd punctuation and featured artists
        let queries = [format!("track:{title} artist:{artist}"), format!("{title} {artist}")];
        for q in queries {
            let path = format!("/search?q={}&type=track&limit=3&market={market}", urlencoding::encode(&q));
            let v = self.get(http, c, &path, false).await?;
            if let Some(found) = list(&v["tracks"], |t| track(t, None)).into_iter().next() {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    pub async fn search(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        q: &str,
        market: &str,
    ) -> Res<SearchResult> {
        let v = self
            .get(
                http,
                c,
                &format!(
                    "/search?q={}&type=track,album,artist,playlist&limit=10&market={market}",
                    urlencoding::encode(q)
                ),
                false,
            )
            .await?;
        Ok(SearchResult {
            tracks: list(&v["tracks"], |t| track(t, None)),
            albums: list(&v["albums"], album),
            artists: list(&v["artists"], |a| {
                Some(SpItem {
                    kind: "artist".into(),
                    id: a["id"].as_str()?.into(),
                    name: a["name"].as_str()?.into(),
                    subtitle: "Artist".into(),
                    image: image(a),
                    duration_ms: None,
                })
            }),
            playlists: list(&v["playlists"], playlist),
        })
    }

    pub async fn my_playlists(&self, http: &reqwest::Client, c: &Creds<'_>) -> Res<Vec<SpItem>> {
        let mut out = vec![];
        let mut path = "/me/playlists?limit=50".to_string();
        while out.len() < 500 {
            let v = self.get(http, c, &path, true).await?;
            // pages can overlap if the library changes mid-fetch
            for p in list(&v, playlist) {
                if !out.iter().any(|o: &SpItem| o.id == p.id) {
                    out.push(p);
                }
            }
            match v["next"].as_str() {
                Some(next) => {
                    path = next
                        .trim_start_matches("https://api.spotify.com/v1")
                        .to_string()
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// What Spotify Connect is playing for the logged-in user. Only meaningful
    /// when that user is the one casting to Sonos. `None` when not logged in.
    pub async fn player_queue(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
    ) -> Res<Option<PlayerQueue>> {
        if !self.logged_in() {
            return Ok(None);
        }
        let v = self.get(http, c, "/me/player/queue", true).await?;
        Ok(Some(PlayerQueue {
            current: track(&v["currently_playing"], None),
            queue: v["queue"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| track(t, None)).collect())
                .unwrap_or_default(),
        }))
    }

    /// Add a track to the logged-in user's Spotify Connect queue — the only
    /// way to make something "play next" while that user is casting.
    pub async fn add_to_player_queue(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        track_id: &str,
    ) -> Res<()> {
        let Some(token) = self.user_token(http, c).await? else {
            return Err("Log in to Spotify first".into());
        };
        let uri = urlencoding::encode(&format!("spotify:track:{track_id}")).into_owned();
        let res = http
            .post(format!(
                "https://api.spotify.com/v1/me/player/queue?uri={uri}"
            ))
            .bearer_auth(token)
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if res.status().is_success() {
            return Ok(());
        }
        let status = res.status();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        let msg = v["error"]["message"].as_str().unwrap_or("");
        Err(
            if status.as_u16() == 403 && msg.to_lowercase().contains("scope") {
                "Log out and in to Spotify again to allow queueing (new permission)".into()
            } else {
                format!("Spotify {status}: {msg}")
            },
        )
    }

    /// Tracks inside an album / playlist / artist / liked songs, for drill-in.
    pub async fn children(
        &self,
        http: &reqwest::Client,
        c: &Creds<'_>,
        kind: &str,
        id: &str,
        name: &str,
        market: &str,
    ) -> Res<Vec<SpItem>> {
        match kind {
            "album" => {
                let v = self
                    .get(http, c, &format!("/albums/{id}?market={market}"), false)
                    .await?;
                let img = image(&v);
                Ok(list(&v["tracks"], |t| track(t, img.as_ref())))
            }
            "playlist" => {
                let v = match self
                    .get(
                        http,
                        c,
                        &format!("/playlists/{id}/items?limit=100&market={market}"),
                        true,
                    )
                    .await
                {
                    Ok(v) => v,
                    // dev-mode apps only get contents of playlists the user owns or collaborates on
                    // (and need a login at all) — the public embed player isn't restricted
                    Err(e) if e.contains("403") || e.contains("Log in") => {
                        return embed_tracks(http, "playlist", id).await
                    }
                    Err(e) => return Err(e),
                };
                // renamed in 2026: items[].item, older shape items[].track
                Ok(v["items"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|i| {
                                track(
                                    if i["item"].is_object() {
                                        &i["item"]
                                    } else {
                                        &i["track"]
                                    },
                                    None,
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default())
            }
            "liked" => {
                let mut out = vec![];
                for offset in [0, 50] {
                    let v = self
                        .get(
                            http,
                            c,
                            &format!("/me/tracks?limit=50&offset={offset}&market={market}"),
                            true,
                        )
                        .await?;
                    let page: Vec<_> = v["items"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|i| track(&i["track"], None)).collect())
                        .unwrap_or_default();
                    let done = page.len() < 50;
                    out.extend(page);
                    if done {
                        break;
                    }
                }
                Ok(out)
            }
            // /artists/{id}/top-tracks was removed for dev-mode apps; search by artist instead
            "artist" => {
                let q = format!("artist:\"{}\"", name.replace('"', ""));
                let v = self
                    .get(
                        http,
                        c,
                        &format!(
                            "/search?q={}&type=track&limit=10&market={market}",
                            urlencoding::encode(&q)
                        ),
                        false,
                    )
                    .await?;
                Ok(list(&v["tracks"], |t| track(t, None)))
            }
            _ => Err(format!("no children for {kind}")),
        }
    }
}

// ------------------------------------------------------------------ embed fallback

/// Track list from Spotify's public embed player (open.spotify.com/embed/…),
/// which any browser can load without auth. It carries the first 100 tracks
/// in its `__NEXT_DATA__` JSON, without per-track art — the caller's cover
/// fills in. Unofficial, so it may break if Spotify changes the page.
async fn embed_tracks(http: &reqwest::Client, kind: &str, id: &str) -> Res<Vec<SpItem>> {
    let html = http
        .get(format!("https://open.spotify.com/embed/{kind}/{id}"))
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let start = html
        .find(r#"<script id="__NEXT_DATA__""#)
        .ok_or("NOT_OWNER")?;
    let json = &html[start..];
    let json = &json[json.find('>').ok_or("NOT_OWNER")? + 1..];
    let json = &json[..json.find("</script>").ok_or("NOT_OWNER")?];
    let v: Value = serde_json::from_str(json).map_err(|_| "NOT_OWNER")?;
    let entity = &v["props"]["pageProps"]["state"]["data"]["entity"];
    let cover = entity["visualIdentity"]["image"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|i| i["maxWidth"].as_u64().unwrap_or(0) >= 64)
                .or(a.first())
        })
        .and_then(|i| i["url"].as_str())
        .map(String::from);
    let tracks: Vec<SpItem> = entity["trackList"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| t["entityType"].as_str().unwrap_or("track") == "track")
                .filter_map(|t| {
                    Some(SpItem {
                        kind: "track".into(),
                        id: t["uri"].as_str()?.strip_prefix("spotify:track:")?.into(),
                        name: t["title"].as_str()?.into(),
                        subtitle: t["subtitle"].as_str().unwrap_or("").into(),
                        image: cover.clone(),
                        duration_ms: t["duration"].as_u64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if tracks.is_empty() {
        return Err("NOT_OWNER".into());
    }
    Ok(tracks)
}

pub const LOGIN_CANCELLED: &str = "Login cancelled";
pub const NOT_REGISTERED: &str = "Your Spotify account isn't on this Spotify app's list of 5 users. Set up your own free Spotify app under Settings → Spotify, or ask the owner to add you.";

/// Catch typos in the setup flow before the browser round-trip: the ID's
/// shape, and with a secret, that Spotify accepts the pair.
pub async fn check_app(http: &reqwest::Client, id: &str, secret: &str) -> Res<()> {
    let hex32 = |s: &str| s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit());
    if !hex32(id) {
        return Err("A client ID is 32 letters and numbers. Copy it from your app's Settings page.".into());
    }
    if secret.is_empty() {
        return Ok(());
    }
    if !hex32(secret) {
        return Err("A client secret is 32 letters and numbers. Click \"View client secret\" to see it.".into());
    }
    Spotify::token_request(http, "grant_type=client_credentials".into(), Some(&Creds { id, secret }))
        .await
        .map(|_| ())
        .map_err(|_| "Spotify didn't accept that client ID and secret together. Check you copied both from the same app.".into())
}

/// A pasted Spotify link or URI (open.spotify.com/playlist/…, spotify:album:…)
/// as something the app can open. Uses Spotify's public oEmbed for the name and
/// cover, so it works without logging in.
pub async fn resolve(http: &reqwest::Client, link: &str) -> Res<SpItem> {
    let re = regex::Regex::new(r"(playlist|album|artist|track)[/:]([A-Za-z0-9]{22})").unwrap();
    let c = re.captures(link).ok_or("That doesn't look like a Spotify link")?;
    let (kind, id) = (c[1].to_string(), c[2].to_string());
    let page = format!("https://open.spotify.com/{kind}/{id}");
    let v: Value = http
        .get(format!("https://open.spotify.com/oembed?url={}", urlencoding::encode(&page)))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|_| "Spotify couldn't find that (is it private?)")?;
    let name = v["title"].as_str().filter(|t| !t.is_empty()).ok_or("Spotify couldn't find that (is it private?)")?;
    let subtitle = match kind.as_str() {
        "playlist" => "Playlist",
        "album" => "Album",
        "artist" => "Artist",
        _ => "Song",
    };
    Ok(SpItem {
        kind,
        id,
        name: name.to_string(),
        subtitle: subtitle.into(),
        image: v["thumbnail_url"].as_str().map(String::from),
        duration_ms: None,
    })
}

#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore]
    async fn resolve_a_pasted_link() {
        let http = reqwest::Client::new();
        let it = super::resolve(&http, "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M?si=abc").await.unwrap();
        assert_eq!((it.kind.as_str(), it.name.as_str()), ("playlist", "Today’s Top Hits"));
        assert!(super::resolve(&http, "hello").await.is_err());
    }

    /// set SPOTIFY_TEST_SECRET to the built-in app's secret
    #[tokio::test]
    #[ignore]
    async fn check_app_catches_typos() {
        let http = reqwest::Client::new();
        let id = "f8140c478dbe48838c5d51272939d2ea";
        assert!(super::check_app(&http, "nope", "").await.is_err(), "bad id shape");
        assert!(super::check_app(&http, id, "").await.is_ok(), "id alone is fine");
        assert!(super::check_app(&http, id, &"0".repeat(32)).await.is_err(), "wrong secret");
        if let Ok(secret) = std::env::var("SPOTIFY_TEST_SECRET") {
            assert!(super::check_app(&http, id, &secret).await.is_ok(), "right pair");
        }
    }

    /// a colleague's setup: no secret, not logged in
    #[tokio::test]
    #[ignore]
    async fn search_without_secret_or_login() {
        let sp = super::Spotify::new(std::env::temp_dir().join("sponos-test-spotify.json"));
        let c = super::Creds { id: "f8140c478dbe48838c5d51272939d2ea", secret: "" };
        let r = sp.search(&reqwest::Client::new(), &c, "odesza", "DK").await.unwrap();
        assert!(!r.tracks.is_empty() && !r.playlists.is_empty());
    }

    /// network test: `cargo test --lib embed -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn embed_lists_a_playlist_we_dont_own() {
        let t = super::embed_tracks(
            &reqwest::Client::new(),
            "playlist",
            "6vDGVr652ztNWKZuHvsFvx",
        )
        .await
        .unwrap();
        println!(
            "{} tracks, first: {} — {} ({:?})",
            t.len(),
            t[0].name,
            t[0].subtitle,
            t[0].image
        );
        assert!(t.len() > 10 && t[0].image.is_some());
    }
}

// ------------------------------------------------------------------ helpers

fn random_string(n: usize) -> String {
    use rand::RngExt;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..n)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

fn query_params(path: &str) -> std::collections::HashMap<String, String> {
    path.split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| {
            (
                k.to_string(),
                urlencoding::decode(v)
                    .map(|s| s.into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect()
}

fn image(v: &Value) -> Option<String> {
    // images are largest-first; take the smallest that's still ≥ 64px
    let imgs = v["images"].as_array()?;
    imgs.iter()
        .rev()
        .find(|i| i["width"].as_u64().unwrap_or(300) >= 64)
        .or(imgs.first())
        .and_then(|i| i["url"].as_str())
        .map(String::from)
}

fn names(v: &Value) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn track(v: &Value, album_img: Option<&String>) -> Option<SpItem> {
    if v["type"].as_str().is_some_and(|t| t != "track") {
        return None; // podcast episodes in playlists
    }
    Some(SpItem {
        kind: "track".into(),
        id: v["id"].as_str()?.into(),
        name: v["name"].as_str()?.into(),
        subtitle: names(&v["artists"]),
        image: image(&v["album"]).or(album_img.cloned()),
        duration_ms: v["duration_ms"].as_u64(),
    })
}

fn album(a: &Value) -> Option<SpItem> {
    Some(SpItem {
        kind: "album".into(),
        id: a["id"].as_str()?.into(),
        name: a["name"].as_str()?.into(),
        subtitle: format!(
            "{} · {}",
            names(&a["artists"]),
            a["release_date"]
                .as_str()
                .unwrap_or("")
                .get(..4)
                .unwrap_or("")
        ),
        image: image(a),
        duration_ms: None,
    })
}

fn playlist(p: &Value) -> Option<SpItem> {
    Some(SpItem {
        kind: "playlist".into(),
        id: p["id"].as_str()?.into(),
        name: p["name"].as_str()?.into(),
        subtitle: format!("by {}", p["owner"]["display_name"].as_str().unwrap_or("?")),
        image: image(p),
        duration_ms: None,
    })
}

fn list(v: &Value, f: impl Fn(&Value) -> Option<SpItem>) -> Vec<SpItem> {
    // search can return `null` entries — filter_map drops them
    v["items"]
        .as_array()
        .map(|a| a.iter().filter_map(&f).collect())
        .unwrap_or_default()
}
