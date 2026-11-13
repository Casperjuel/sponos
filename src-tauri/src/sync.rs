//! Shares things between everyone on the same Sonos system, through a tiny
//! blob store (`sync/` in this repo):
//! - the floorplan, system name and Spotify link (one document, last write wins)
//! - song votes and who added which song (merged per change on the server)
//!
//! Everything is derived from the household ID, which you can only read from
//! the speakers themselves: document keys are hashes of it and the content is
//! AES-256-GCM encrypted with another hash of it. The server never sees which
//! system it stores, which songs, or who voted.

use crate::sonos::Res;
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, time::Duration};

/// base of the sync service
const SYNC_URL: &str = match option_env!("SPONOS_SYNC_URL") {
    Some(url) if !url.is_empty() => url,
    _ => "https://sponos-sync.vercel.app/api",
};

// ------------------------------------------------------------------ crypto

fn digest(label: &str, household: &str) -> [u8; 32] {
    Sha256::digest(format!("sponos-{label}:{household}").as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn doc_key(household: &str) -> String {
    hex(&digest("doc", household))
}

/// 32 hex chars, unlinkable between systems
fn short_hash(label: &str, household: &str, value: &str) -> String {
    hex(&digest(label, &format!("{household}:{value}"))[..16])
}

fn cipher(label: &str, household: &str) -> Aes256Gcm {
    Aes256Gcm::new(&digest(label, household).into())
}

fn encrypt(household: &str, plain: &[u8]) -> Res<Vec<u8>> {
    encrypt_as("key", household, plain)
}

fn decrypt(household: &str, data: &[u8]) -> Res<Vec<u8>> {
    decrypt_as("key", household, data)
}

/// `label` picks the key: each kind of document has its own.
fn encrypt_as(label: &str, household: &str, plain: &[u8]) -> Res<Vec<u8>> {
    use rand::RngExt;
    let nonce: [u8; 12] = rand::rng().random();
    let mut out = nonce.to_vec();
    out.extend(cipher(label, household).encrypt(&Nonce::from(nonce), plain).map_err(|_| "encrypt failed")?);
    Ok(out)
}

fn decrypt_as(label: &str, household: &str, data: &[u8]) -> Res<Vec<u8>> {
    if data.len() < 12 {
        return Err("sync: short document".into());
    }
    let (nonce, body) = data.split_at(12);
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| "sync: bad nonce")?;
    cipher(label, household).decrypt(&Nonce::from(nonce), body).map_err(|_| "sync: can't decrypt".into())
}

fn seal_json<T: Serialize>(household: &str, value: &T) -> Res<String> {
    Ok(B64.encode(encrypt(household, &serde_json::to_vec(value).map_err(|e| e.to_string())?)?))
}

fn open_json<T: for<'de> Deserialize<'de>>(household: &str, text: &str) -> Option<T> {
    serde_json::from_slice(&decrypt(household, &B64.decode(text).ok()?).ok()?).ok()
}

fn etag_of(r: &reqwest::Response) -> String {
    r.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string()
}

// ------------------------------------------------------------------ floorplan

/// What everyone on a system shares. Personal settings (theme, Spotify login) stay local.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Shared {
    /// floorplan JSON as the frontend stores it (image data URL + pins)
    pub floorplan: Option<String>,
    pub name: Option<String>,
    /// Sonos ↔ Spotify account override (sid, sn)
    pub spotify: Option<(u32, u32)>,
}

impl Shared {
    pub fn is_empty(&self) -> bool {
        *self == Shared::default()
    }
}

pub enum Pulled {
    Unchanged,
    /// nobody has shared this system yet
    Missing,
    Doc { etag: String, shared: Shared },
}

fn plan_url(household: &str) -> String {
    format!("{SYNC_URL}/plan?key={}", doc_key(household))
}

pub async fn pull(http: &reqwest::Client, household: &str, etag: Option<&str>) -> Res<Pulled> {
    let mut req = http.get(plan_url(household)).timeout(Duration::from_secs(15));
    if let Some(etag) = etag {
        req = req.header("if-none-match", etag);
    }
    let r = req.send().await.map_err(|e| e.to_string())?;
    match r.status().as_u16() {
        304 => Ok(Pulled::Unchanged),
        404 => Ok(Pulled::Missing),
        200 => {
            let etag = etag_of(&r);
            let body = r.bytes().await.map_err(|e| e.to_string())?;
            let plain = decrypt(household, &body)?;
            Ok(Pulled::Doc { etag, shared: serde_json::from_slice(&plain).map_err(|e| e.to_string())? })
        }
        s => Err(format!("sync: HTTP {s}")),
    }
}

/// Returns the new etag.
pub async fn push(http: &reqwest::Client, household: &str, shared: &Shared) -> Res<String> {
    let body = encrypt(household, &serde_json::to_vec(shared).map_err(|e| e.to_string())?)?;
    let r = http
        .put(plan_url(household))
        .body(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("sync: HTTP {}", r.status()));
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    Ok(v["etag"].as_str().unwrap_or_default().to_string())
}

// ------------------------------------------------------------------ secrets

/// Keys a system shares, such as the Anthropic API key for AI playlists. They
/// live in a document of their own, under a different name and encryption key
/// than the floorplan, so an app version that doesn't know about them can't
/// overwrite them when it saves the floorplan.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Secrets {
    anthropic_key: Option<String>,
}

fn secrets_url(household: &str) -> String {
    format!("{SYNC_URL}/plan?key={}", hex(&digest("secrets-doc", household)))
}

/// The system's shared Anthropic key, if someone saved one.
pub async fn pull_ai_key(http: &reqwest::Client, household: &str) -> Res<Option<String>> {
    let r = http.get(secrets_url(household)).timeout(Duration::from_secs(15)).send().await.map_err(|e| e.to_string())?;
    match r.status().as_u16() {
        404 => Ok(None),
        200 => {
            let body = r.bytes().await.map_err(|e| e.to_string())?;
            let secrets: Secrets = serde_json::from_slice(&decrypt_as("secrets-key", household, &body)?).map_err(|e| e.to_string())?;
            Ok(secrets.anthropic_key.filter(|k| !k.is_empty()))
        }
        s => Err(format!("sync: HTTP {s}")),
    }
}

/// Share the key with the system; `None` takes it back for everyone.
pub async fn push_ai_key(http: &reqwest::Client, household: &str, key: Option<&str>) -> Res<()> {
    let secrets = Secrets { anthropic_key: key.map(str::to_string) };
    let body = encrypt_as("secrets-key", household, &serde_json::to_vec(&secrets).map_err(|e| e.to_string())?)?;
    let r = http.put(secrets_url(household)).body(body).timeout(Duration::from_secs(30)).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("Couldn't share the key (HTTP {})", r.status()));
    }
    Ok(())
}

// ------------------------------------------------------------------ votes + added by

/// A song, as the people voting see it. Encrypted before it leaves the machine.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackMeta {
    /// normalised "title|artist", so the same song matches from Spotify Connect or the queue
    pub key: String,
    pub title: String,
    pub artist: Option<String>,
    pub art: Option<String>,
}

/// Who someone is: their Spotify name and avatar when logged in.
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    pub image: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AddedBy {
    #[serde(flatten)]
    pub person: Person,
    pub at: u64,
    /// added from this device
    pub me: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TrackSocial {
    #[serde(flatten)]
    pub meta: TrackMeta,
    pub up: u32,
    pub down: u32,
    /// this device's vote: 1, -1 or 0
    pub mine: i8,
    pub up_by: Vec<String>,
    pub down_by: Vec<String>,
    pub added_by: Option<AddedBy>,
    /// fun emoji reactions ("poo", "unicorn", …), most popular first
    pub reactions: Vec<Reaction>,
    /// last change, ms since epoch
    pub at: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub emoji: String,
    pub by: Vec<String>,
    pub mine: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SocialDoc {
    tracks: HashMap<String, RawTrack>,
    people: HashMap<String, String>,
}

#[derive(Deserialize)]
struct RawTrack {
    meta: String,
    at: u64,
    #[serde(default)]
    votes: HashMap<String, i8>,
    added: Option<RawAdded>,
    #[serde(default)]
    reacts: HashMap<String, String>,
}

#[derive(Deserialize)]
struct RawAdded {
    by: String,
    at: u64,
}

fn social_url(household: &str) -> String {
    format!("{SYNC_URL}/votes?key={}", doc_key(household))
}

fn decode(household: &str, device: &str, doc: SocialDoc) -> Vec<TrackSocial> {
    let me = short_hash("device", household, device);
    let people: HashMap<_, Person> = doc.people.iter().filter_map(|(d, p)| Some((d.clone(), open_json(household, p)?))).collect();
    let name = |d: &String| {
        if *d == me {
            "You".to_string()
        } else {
            people.get(d).map(|p| p.name.clone()).unwrap_or_else(|| "Someone".into())
        }
    };
    let mut out: Vec<TrackSocial> = doc
        .tracks
        .into_values()
        .filter_map(|t| {
            let meta: TrackMeta = open_json(household, &t.meta)?;
            let by = |v: i8| t.votes.iter().filter(|(_, x)| **x == v).map(|(d, _)| name(d)).collect::<Vec<_>>();
            let (up_by, down_by) = (by(1), by(-1));
            // songs added by someone not logged in to Spotify have no person: show nothing
            let added_by = t.added.and_then(|a| Some(AddedBy { person: people.get(&a.by)?.clone(), at: a.at, me: a.by == me }));
            let mut reactions: Vec<Reaction> = Vec::new();
            for (d, emoji) in &t.reacts {
                match reactions.iter_mut().find(|r| &r.emoji == emoji) {
                    Some(r) => r.by.push(name(d)),
                    None => reactions.push(Reaction { emoji: emoji.clone(), by: vec![name(d)], mine: false }),
                }
                if *d == me {
                    reactions.iter_mut().find(|r| &r.emoji == emoji).unwrap().mine = true;
                }
            }
            reactions.sort_by(|a, b| b.by.len().cmp(&a.by.len()).then(a.emoji.cmp(&b.emoji)));
            Some(TrackSocial {
                meta,
                up: up_by.len() as u32,
                down: down_by.len() as u32,
                mine: t.votes.get(&me).copied().unwrap_or(0),
                up_by,
                down_by,
                added_by,
                reactions,
                at: t.at,
            })
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

/// `None` when nothing changed since `etag`.
pub async fn social(
    http: &reqwest::Client,
    household: &str,
    device: &str,
    etag: Option<&str>,
) -> Res<Option<(String, Vec<TrackSocial>)>> {
    let mut req = http.get(social_url(household)).timeout(Duration::from_secs(10));
    if let Some(etag) = etag {
        req = req.header("if-none-match", etag);
    }
    let r = req.send().await.map_err(|e| e.to_string())?;
    match r.status().as_u16() {
        304 => Ok(None),
        200 => {
            let etag = etag_of(&r);
            let doc: SocialDoc = r.json().await.map_err(|e| e.to_string())?;
            Ok(Some((etag, decode(household, device, doc))))
        }
        s => Err(format!("votes: HTTP {s}")),
    }
}

/// What changed: a vote (1, -1, or 0 to take it back) and/or songs just added
/// to the queue.
pub struct Change<'a> {
    pub person: Option<&'a Person>,
    pub vote: Option<(&'a TrackMeta, i8)>,
    /// an emoji reaction; `None` takes yours back
    pub react: Option<(&'a TrackMeta, Option<&'a str>)>,
    pub added: &'a [TrackMeta],
}

/// Returns everything, updated.
pub async fn change(http: &reqwest::Client, household: &str, device: &str, c: Change<'_>) -> Res<(String, Vec<TrackSocial>)> {
    let track = |m: &TrackMeta| -> Res<serde_json::Value> {
        Ok(serde_json::json!({ "track": short_hash("track", household, &m.key), "meta": seal_json(household, m)? }))
    };
    let mut body = serde_json::json!({
        "device": short_hash("device", household, device),
        "person": c.person.map(|p| seal_json(household, p)).transpose()?,
        "added": c.added.iter().map(track).collect::<Res<Vec<_>>>()?,
    });
    if let Some((m, value)) = c.vote {
        let mut v = track(m)?;
        v["vote"] = value.into();
        body["vote"] = v;
    }
    if let Some((m, emoji)) = c.react {
        let mut v = track(m)?;
        v["emoji"] = emoji.into();
        body["react"] = v;
    }
    let mut last = String::new();
    for attempt in 0..4u64 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(200 * attempt)).await;
        }
        match http.post(social_url(household)).json(&body).timeout(Duration::from_secs(15)).send().await {
            Ok(r) if r.status().is_success() => {
                let etag = etag_of(&r);
                let doc: SocialDoc = r.json().await.map_err(|e| e.to_string())?;
                return Ok((etag, decode(household, device, doc)));
            }
            Ok(r) => last = format!("Couldn't save (HTTP {})", r.status()),
            Err(e) => last = format!("Couldn't save: {e}"),
        }
    }
    Err(last)
}

// ------------------------------------------------------------------ spotify

/// A client-credentials token for Spotify's public catalogue (search, albums,
/// artists), made by the sync service so the client secret isn't in the app.
pub async fn spotify_token(http: &reqwest::Client) -> Res<serde_json::Value> {
    let r = http
        .get(format!("{SYNC_URL}/spotify-token"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    r.json().await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Shared { floorplan: Some("{\"pins\":{}}".into()), name: Some("Office".into()), spotify: Some((9, 34)) };
        let sealed = encrypt("Sonos_abc", &serde_json::to_vec(&s).unwrap()).unwrap();
        let back: Shared = serde_json::from_slice(&decrypt("Sonos_abc", &sealed).unwrap()).unwrap();
        assert!(back == s);
        assert!(decrypt("Sonos_other", &sealed).is_err(), "another household can't read it");
        assert_eq!(doc_key("Sonos_abc").len(), 64);
        // secrets use their own document and key
        assert!(!secrets_url("Sonos_abc").contains(&doc_key("Sonos_abc")));
        let secret = encrypt_as("secrets-key", "Sonos_abc", b"sk-ant-x").unwrap();
        assert!(decrypt("Sonos_abc", &secret).is_err(), "the floorplan key can't open the secrets");
        assert_eq!(decrypt_as("secrets-key", "Sonos_abc", &secret).unwrap(), b"sk-ant-x");
        assert!(decrypt_as("secrets-key", "Sonos_other", &secret).is_err());
        assert_eq!(short_hash("track", "Sonos_abc", "x").len(), 32);
    }

    /// cargo test --lib sync -- --ignored  (talks to the real service)
    #[tokio::test]
    #[ignore]
    async fn live_plan() {
        let http = reqwest::Client::new();
        let hh = format!("test-{}", rand::random::<u64>());
        assert!(matches!(pull(&http, &hh, None).await.unwrap(), Pulled::Missing));
        let s = Shared { name: Some("Test".into()), ..Default::default() };
        let etag = push(&http, &hh, &s).await.unwrap();
        assert!(matches!(pull(&http, &hh, Some(&etag)).await.unwrap(), Pulled::Unchanged));
        match pull(&http, &hh, None).await.unwrap() {
            Pulled::Doc { shared, .. } => assert!(shared == s),
            _ => panic!("expected doc"),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn live_spotify_token() {
        let v = spotify_token(&reqwest::Client::new()).await.unwrap();
        assert!(v["access_token"].as_str().is_some_and(|t| !t.is_empty()));
    }

    #[tokio::test]
    #[ignore]
    async fn live_social() {
        let http = reqwest::Client::new();
        let hh = format!("test-{}", rand::random::<u64>());
        let song = TrackMeta { key: "song|artist".into(), title: "Song".into(), artist: Some("Artist".into()), art: None };
        let ann = Person { name: "Ann".into(), image: Some("https://example.com/a.jpg".into()) };
        let bob = Person { name: "Bob".into(), image: None };

        change(&http, &hh, "dev-a", Change { person: Some(&ann), vote: None, react: None, added: &[song.clone()] }).await.unwrap();
        change(&http, &hh, "dev-a", Change { person: Some(&ann), vote: Some((&song, 1)), react: None, added: &[] }).await.unwrap();
        let (_, all) = change(&http, &hh, "dev-b", Change { person: Some(&bob), vote: Some((&song, -1)), react: Some((&song, Some("unicorn"))), added: &[] }).await.unwrap();
        let t = &all[0];
        assert_eq!((t.up, t.down, t.mine), (1, 1, -1));
        assert_eq!(t.up_by, vec!["Ann".to_string()]);
        assert_eq!(t.down_by, vec!["You".to_string()]);
        let added = t.added_by.as_ref().unwrap();
        assert_eq!((added.person.name.as_str(), added.me), ("Ann", false));

        let (_, all) = change(&http, &hh, "dev-b", Change { person: None, vote: Some((&song, 0)), react: None, added: &[] }).await.unwrap();
        assert_eq!((all[0].up, all[0].down, all[0].mine), (1, 0, 0));
        assert!(all[0].added_by.is_some(), "taking a vote back keeps the 'added by'");
        let r = &all[0].reactions[0];
        assert_eq!((r.emoji.as_str(), r.mine), ("unicorn", true), "and the reaction");
    }
}
