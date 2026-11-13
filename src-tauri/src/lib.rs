mod ai;
mod sonos;
mod spotify;
mod sync;
mod tray;

use serde::{Deserialize, Serialize};
use sonos::{Group, Item, PlayerState, Res, SpotifyLink, Svc};
use spotify::{Creds, Me, PlayerQueue, SearchResult, SpItem, Spotify};
use std::{collections::HashMap, path::PathBuf, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

/// Shared Spotify app, baked in at build time (`SPOTIFY_CLIENT_ID`). Logging in
/// uses PKCE, which needs only this public ID, never a secret. Dev-mode apps
/// allow 5 users, each added under User Management in the Spotify dashboard.
/// (CI passes an empty string when the repo variable isn't set, so treat that as unset.)
const DEFAULT_CLIENT_ID: &str = match option_env!("SPOTIFY_CLIENT_ID") {
    Some(id) if !id.is_empty() => id,
    _ => "f8140c478dbe48838c5d51272939d2ea",
};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    /// own Spotify app; empty uses the built-in one
    spotify_client_id: String,
    /// dev only (env `SPOTIFY_CLIENT_SECRET`): enables search without logging in
    spotify_client_secret: String,
    /// ISO country for Spotify search availability
    market: String,
    /// speakers to try when SSDP discovery is blocked
    seed_ips: Vec<String>,
    /// override the auto-detected Sonos ↔ Spotify account — for the *current*
    /// system; persisted per system in `spotify_overrides`
    spotify_sid: Option<u32>,
    spotify_sn: Option<u32>,
    /// household id → (sid, sn)
    spotify_overrides: HashMap<String, (u32, u32)>,
    /// household id → name the user gave it ("Office", "Home")
    household_names: HashMap<String, String>,
    /// the system picked by hand when several are visible at once
    preferred_household: Option<String>,
    /// don't share floorplan/name/Spotify link with others on the network
    local_only: bool,
    /// random id for this install; votes and "added by" are tied to it
    device_id: String,
    /// household id → Anthropic API key for AI playlists (also shared, encrypted, unless local only)
    ai_keys: HashMap<String, String>,
}

struct App {
    http: reqwest::Client,
    spotify: Spotify,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// rooms of the active system
    groups: Mutex<Vec<Group>>,
    /// active Sonos system (household id)
    household: Mutex<Option<String>>,
    /// detected Spotify link per household
    links: Mutex<HashMap<String, SpotifyLink>>,
    /// last synced version per household
    sync_etags: Mutex<HashMap<String, String>>,
    /// bumped on every local edit, so a pull that raced an edit doesn't overwrite it
    local_rev: std::sync::atomic::AtomicU64,
    /// one push or pull at a time
    sync_lock: Mutex<()>,
    /// when each system's shared AI key was last looked up
    ai_checked: Mutex<HashMap<String, std::time::Instant>>,
    /// votes and "added by" per household: (etag, tracks)
    social: Mutex<HashMap<String, (String, Vec<sync::TrackSocial>)>>,
}

impl App {
    async fn household(&self) -> String {
        self.household.lock().await.clone().unwrap_or_default()
    }
}

impl App {
    async fn group(&self, id: &str) -> Res<Group> {
        self.groups
            .lock()
            .await
            .iter()
            .find(|g| g.id == id || g.coordinator_uuid == id)
            .cloned()
            .ok_or_else(|| "That room is gone — it was probably regrouped".into())
    }
}

// ------------------------------------------------------------------ settings

#[tauri::command]
async fn get_settings(app: State<'_, App>) -> Res<Settings> {
    let mut s = app.settings.lock().await.clone();
    // surface the active system's Spotify override in the flat sid/sn fields
    let ov = s.spotify_overrides.get(&app.household().await).copied();
    (s.spotify_sid, s.spotify_sn) = (ov.map(|o| o.0), ov.map(|o| o.1));
    Ok(s)
}

fn write_settings(app: &App, s: &Settings) -> Res<()> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&app.settings_path, json).map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_settings(app: State<'_, App>, handle: AppHandle, mut settings: Settings) -> Res<()> {
    let hh = app.household().await;
    let switched_app;
    {
        // the dialog doesn't round-trip per-system maps, so carry them over
        let cur = app.settings.lock().await;
        // a login belongs to the Spotify app that made it
        switched_app = cur.spotify_client_id != settings.spotify_client_id;
        settings.household_names = cur.household_names.clone();
        settings.preferred_household = cur.preferred_household.clone();
        settings.spotify_overrides = cur.spotify_overrides.clone();
        settings.device_id = cur.device_id.clone();
        settings.ai_keys = cur.ai_keys.clone();
    }
    match (settings.spotify_sid, settings.spotify_sn) {
        (Some(sid), Some(sn)) => drop(settings.spotify_overrides.insert(hh.clone(), (sid, sn))),
        _ => drop(settings.spotify_overrides.remove(&hh)),
    }
    write_settings(&app, &settings)?;
    *app.settings.lock().await = settings;
    app.spotify.reset_app_token().await;
    if switched_app {
        app.spotify.logout().await?;
    }
    app.links.lock().await.remove(&hh);
    share(&app, handle, hh);
    Ok(())
}

// ------------------------------------------------------------------ rooms / systems

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HouseholdInfo {
    id: String,
    name: String,
    speakers: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Discovery {
    /// every Sonos system visible from this network (usually one)
    households: Vec<HouseholdInfo>,
    active: String,
    groups: Vec<Group>,
}

/// One speaker's view of its system: household id + full topology.
async fn probe(http: reqwest::Client, ip: String) -> Option<(String, Vec<Group>)> {
    tokio::time::timeout(Duration::from_millis(2500), async {
        let (hh, topo) = tokio::join!(sonos::household_id(&http, &ip), sonos::topology(&http, &ip));
        Some((hh.ok()?, topo.ok()?))
    })
    .await
    .ok()
    .flatten()
}

async fn probe_all(http: &reqwest::Client, ips: Vec<String>) -> Vec<(String, Vec<Group>)> {
    let mut set = tokio::task::JoinSet::new();
    for ip in ips {
        set.spawn(probe(http.clone(), ip));
    }
    set.join_all().await.into_iter().flatten().collect()
}

/// Find every reachable Sonos system and pick the active one. Known speakers
/// (last system + seed IPs) are probed in parallel with SSDP, so moving between
/// office and home switches over in a couple of seconds.
#[tauri::command]
async fn discover(app: State<'_, App>) -> Res<Discovery> {
    let s = app.settings.lock().await.clone();
    let mut known: Vec<String> = app
        .groups
        .lock()
        .await
        .iter()
        .flat_map(|g| g.members.iter().map(|m| m.ip.clone()))
        .collect();
    known.extend(s.seed_ips.iter().cloned());
    known.dedup();

    let (found_known, ssdp_ips) = tokio::join!(
        probe_all(&app.http, known.clone()),
        sonos::ssdp(Duration::from_millis(1500))
    );
    let mut systems: HashMap<String, Vec<Group>> = found_known.into_iter().collect();
    // only probe SSDP responders that aren't already part of a found system
    let covered: Vec<String> = systems
        .values()
        .flatten()
        .flat_map(|g| g.members.iter().map(|m| m.ip.clone()))
        .collect();
    let fresh: Vec<String> = ssdp_ips
        .into_iter()
        .filter(|ip| !covered.contains(ip) && !known.contains(ip))
        .collect();
    for (hh, groups) in probe_all(&app.http, fresh).await {
        systems.entry(hh).or_insert(groups);
    }
    if systems.is_empty() {
        return Err("No Sonos speakers found. If you're on the right network, add a speaker IP in Settings.".into());
    }

    let current = app.household.lock().await.clone();
    let active = [s.preferred_household.clone(), current]
        .into_iter()
        .flatten()
        .find(|h| systems.contains_key(h))
        // otherwise the biggest system on this network
        .unwrap_or_else(|| {
            systems
                .iter()
                .max_by_key(|(_, g)| g.iter().map(|x| x.members.len()).sum::<usize>())
                .unwrap()
                .0
                .clone()
        });

    let mut households: Vec<HouseholdInfo> = systems
        .iter()
        .map(|(id, groups)| HouseholdInfo {
            id: id.clone(),
            name: household_name(&s, id, groups),
            speakers: groups.iter().map(|g| g.members.len()).sum(),
        })
        .collect();
    households.sort_by(|a, b| a.name.cmp(&b.name));
    let groups = systems.remove(&active).unwrap_or_default();
    *app.groups.lock().await = groups.clone();
    *app.household.lock().await = Some(active.clone());
    Ok(Discovery {
        households,
        active,
        groups,
    })
}

fn household_name(s: &Settings, id: &str, groups: &[Group]) -> String {
    s.household_names.get(id).cloned().unwrap_or_else(|| {
        // until it's named: "Sonos · Kitchen" after its first room alphabetically
        let mut rooms: Vec<&str> = groups
            .iter()
            .flat_map(|g| g.members.iter().map(|m| m.name.as_str()))
            .collect();
        rooms.sort_unstable();
        format!("Sonos · {}", rooms.first().unwrap_or(&"?"))
    })
}

/// Pin a system when more than one is visible on the same network.
#[tauri::command]
async fn set_household(app: State<'_, App>, id: String) -> Res<Discovery> {
    {
        let mut s = app.settings.lock().await;
        s.preferred_household = Some(id.clone());
        write_settings(&app, &s)?;
    }
    *app.household.lock().await = Some(id);
    app.groups.lock().await.clear();
    discover(app).await
}

#[tauri::command]
async fn rename_household(
    app: State<'_, App>,
    handle: AppHandle,
    id: String,
    name: String,
) -> Res<()> {
    let mut s = app.settings.lock().await;
    let name = name.trim().to_string();
    if name.is_empty() {
        s.household_names.remove(&id);
    } else {
        s.household_names.insert(id.clone(), name);
    }
    write_settings(&app, &s)?;
    share(&app, handle, id);
    Ok(())
}

#[tauri::command]
async fn get_state(app: State<'_, App>, group: String) -> Res<PlayerState> {
    let g = app.group(&group).await?;
    sonos::state(&app.http, &g.coordinator_ip).await
}

#[tauri::command]
async fn get_queue(app: State<'_, App>, group: String) -> Res<Vec<Item>> {
    let g = app.group(&group).await?;
    sonos::queue(&app.http, &g.coordinator_ip).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MemberVol {
    uuid: String,
    ip: String,
    name: String,
    volume: u32,
}

#[tauri::command]
async fn get_members(app: State<'_, App>, group: String) -> Res<Vec<MemberVol>> {
    let g = app.group(&group).await?;
    let vols = sonos::member_volumes(&app.http, &g.members).await;
    Ok(g.members
        .into_iter()
        .map(|m| MemberVol {
            volume: vols.get(&m.uuid).copied().unwrap_or(0),
            uuid: m.uuid,
            ip: m.ip,
            name: m.name,
        })
        .collect())
}

#[tauri::command]
async fn set_member_volume(app: State<'_, App>, ip: String, volume: u32) -> Res<()> {
    sonos::set_member_volume(&app.http, &ip, volume).await
}

// ------------------------------------------------------------------ transport

#[tauri::command]
async fn control(app: State<'_, App>, group: String, action: String) -> Res<()> {
    let g = app.group(&group).await?;
    let ip = &g.coordinator_ip;
    let h = &app.http;
    match action.as_str() {
        "play" => {
            // nothing loaded (or a dead stream) → fall back to the queue
            if sonos::soap(h, ip, Svc::AVTransport, "Play", "<Speed>1</Speed>")
                .await
                .is_err()
            {
                sonos::use_queue(h, ip, &g.coordinator_uuid).await?;
                sonos::soap(h, ip, Svc::AVTransport, "Play", "<Speed>1</Speed>").await?;
            }
        }
        "pause" => {
            // streams can't pause, only stop
            if sonos::soap(h, ip, Svc::AVTransport, "Pause", "")
                .await
                .is_err()
            {
                sonos::soap(h, ip, Svc::AVTransport, "Stop", "").await?;
            }
        }
        "next" => drop(sonos::soap(h, ip, Svc::AVTransport, "Next", "").await?),
        "previous" => drop(sonos::soap(h, ip, Svc::AVTransport, "Previous", "").await?),
        _ => return Err(format!("unknown action {action}")),
    }
    Ok(())
}

#[tauri::command]
async fn seek(app: State<'_, App>, group: String, seconds: u32) -> Res<()> {
    let g = app.group(&group).await?;
    let t = format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "Seek",
        &format!("<Unit>REL_TIME</Unit><Target>{t}</Target>"),
    )
    .await
    .map(|_| ())
}

#[tauri::command]
async fn set_volume(app: State<'_, App>, group: String, volume: u32) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::set_group_volume(&app.http, &g.coordinator_ip, volume).await
}

#[tauri::command]
async fn set_mute(app: State<'_, App>, group: String, muted: bool) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::GroupRenderingControl,
        "SetGroupMute",
        &format!("<DesiredMute>{}</DesiredMute>", muted as u8),
    )
    .await
    .map(|_| ())
}

/// Crossfade belongs to the room (or the group, via its coordinator).
#[tauri::command]
async fn set_crossfade(app: State<'_, App>, group: String, on: bool) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "SetCrossfadeMode",
        &format!("<CrossfadeMode>{}</CrossfadeMode>", u8::from(on)),
    )
    .await
    .map(|_| ())
}

#[tauri::command]
async fn set_play_mode(app: State<'_, App>, group: String, mode: String) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "SetPlayMode",
        &format!("<NewPlayMode>{}</NewPlayMode>", sonos::esc(&mode)),
    )
    .await
    .map(|_| ())
}

// ------------------------------------------------------------------ queue

#[tauri::command]
async fn play_index(app: State<'_, App>, group: String, n: u32) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::play_track(&app.http, &g.coordinator_ip, &g.coordinator_uuid, n).await
}

#[tauri::command]
async fn remove_index(app: State<'_, App>, group: String, n: u32) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "RemoveTrackFromQueue",
        &format!("<ObjectID>Q:0/{n}</ObjectID><UpdateID>0</UpdateID>"),
    )
    .await
    .map(|_| ())
}

#[tauri::command]
async fn move_index(app: State<'_, App>, group: String, from: u32, before: u32) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "ReorderTracksInQueue",
        &format!("<StartingIndex>{from}</StartingIndex><NumberOfTracks>1</NumberOfTracks><InsertBefore>{before}</InsertBefore><UpdateID>0</UpdateID>"),
    )
    .await
    .map(|_| ())
}

#[tauri::command]
async fn clear_queue(app: State<'_, App>, group: String) -> Res<()> {
    let g = app.group(&group).await?;
    sonos::soap(
        &app.http,
        &g.coordinator_ip,
        Svc::AVTransport,
        "RemoveAllTracksFromQueue",
        "",
    )
    .await
    .map(|_| ())
}

// ------------------------------------------------------------------ spotify

/// How the active system reaches Spotify. Each household links its own
/// Spotify account, so sid/sn are detected and cached per system.
async fn link(app: &App, ip: &str) -> SpotifyLink {
    let hh = app.household().await;
    if let Some(&(sid, sn)) = app.settings.lock().await.spotify_overrides.get(&hh) {
        return SpotifyLink::fallback(sid, sn);
    }
    let mut links = app.links.lock().await;
    if let Some(l) = links.get(&hh) {
        return l.clone();
    }
    match sonos::detect_spotify(&app.http, ip).await {
        Some(l) => {
            links.insert(hh, l.clone());
            l
        }
        // S2 default; sn=1 is the first linked account. Not cached so we retry.
        None => SpotifyLink::fallback(9, 1),
    }
}

#[tauri::command]
async fn spotify_link(app: State<'_, App>, group: String) -> Res<SpotifyLink> {
    let g = app.group(&group).await?;
    Ok(link(&app, &g.coordinator_ip).await)
}

#[tauri::command]
async fn spotify_search(app: State<'_, App>, q: String) -> Res<SearchResult> {
    let s = app.settings.lock().await.clone();
    app.spotify
        .search(&app.http, &creds(&s), &q, market(&s))
        .await
}

#[tauri::command]
async fn spotify_children(
    app: State<'_, App>,
    kind: String,
    id: String,
    name: String,
) -> Res<Vec<SpItem>> {
    let s = app.settings.lock().await.clone();
    app.spotify
        .children(&app.http, &creds(&s), &kind, &id, &name, market(&s))
        .await
}

#[tauri::command]
async fn spotify_login(handle: tauri::AppHandle, app: State<'_, App>) -> Res<Option<Me>> {
    use tauri_plugin_opener::OpenerExt;
    let s = app.settings.lock().await.clone();
    let c = creds(&s);
    app.spotify
        .login(&app.http, &c, |url| {
            handle
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| e.to_string())
        })
        .await?;
    app.spotify.me(&app.http, &c).await
}

/// Give up on a browser login that is still waiting.
#[tauri::command]
async fn spotify_cancel_login(app: State<'_, App>) -> Res<()> {
    app.spotify.cancel_login();
    Ok(())
}

#[tauri::command]
async fn spotify_logout(app: State<'_, App>) -> Res<()> {
    app.spotify.logout().await
}

#[tauri::command]
async fn spotify_me(app: State<'_, App>) -> Res<Option<Me>> {
    let s = app.settings.lock().await.clone();
    app.spotify.me(&app.http, &creds(&s)).await
}

#[tauri::command]
async fn spotify_playlists(app: State<'_, App>) -> Res<Vec<SpItem>> {
    let s = app.settings.lock().await.clone();
    app.spotify.my_playlists(&app.http, &creds(&s)).await
}

#[tauri::command]
async fn spotify_player_queue(app: State<'_, App>) -> Res<Option<PlayerQueue>> {
    let s = app.settings.lock().await.clone();
    app.spotify.player_queue(&app.http, &creds(&s)).await
}

fn creds(s: &Settings) -> Creds<'_> {
    let id = if s.spotify_client_id.is_empty() {
        DEFAULT_CLIENT_ID
    } else {
        &s.spotify_client_id
    };
    Creds {
        id,
        secret: &s.spotify_client_secret,
    }
}

fn market(s: &Settings) -> &str {
    if s.market.len() == 2 {
        &s.market
    } else {
        "DK"
    }
}

/// mode: "end" (append), "next" (after current), "now" (insert after current and jump to it)
#[tauri::command]
async fn queue_spotify(
    app: State<'_, App>,
    group: String,
    kind: String,
    id: String,
    title: String,
    mode: String,
) -> Res<Queued> {
    enqueue_items(&app, &group, &[QueueItem { kind, id, title }], &mode).await
}

#[tauri::command]
async fn spotify_add_to_player_queue(app: State<'_, App>, id: String) -> Res<()> {
    let s = app.settings.lock().await.clone();
    app.spotify
        .add_to_player_queue(&app.http, &creds(&s), &id)
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Queued {
    added: u32,
    /// "afterCurrent" | "top" | "end" — top means the queue wasn't playing,
    /// so "next" could only mean the front of the queue
    placed: &'static str,
}

#[derive(Deserialize)]
struct QueueItem {
    kind: String,
    id: String,
    title: String,
}

/// Several items in order — e.g. a list of liked songs, which has no Sonos container.
#[tauri::command]
async fn queue_tracks(
    app: State<'_, App>,
    group: String,
    items: Vec<QueueItem>,
    mode: String,
) -> Res<Queued> {
    enqueue_items(&app, &group, &items, &mode).await
}

async fn enqueue_items(app: &App, group: &str, items: &[QueueItem], mode: &str) -> Res<Queued> {
    let g = app.group(group).await?;
    let (ip, h) = (&g.coordinator_ip, &app.http);
    let l = link(app, ip).await;
    let st = sonos::state(h, ip).await?;
    let (mut at, placed) = match mode {
        "end" => (0, "end"),
        _ if st.queue_active && st.track_no > 0 => (st.track_no + 1, "afterCurrent"),
        // queue not playing (Spotify Connect, radio, empty): front of the queue
        _ => (1, "top"),
    };
    let (mut first, mut total) = (0, 0);
    for it in items {
        let (uri, meta) = l.uri_and_meta(&it.kind, &it.id, &it.title)?;
        let (f, added) = sonos::enqueue(h, ip, &uri, &meta, at, at != 0).await.map_err(|e| {
            if l.detected {
                e
            } else {
                format!("{e}. Couldn't detect your Sonos Spotify account — play any Spotify track from the Sonos app once, or set sid/sn in Settings.")
            }
        })?;
        if first == 0 {
            first = f;
        }
        total += added;
        if at != 0 {
            at = f + added; // keep the batch in order
        }
    }
    if mode == "now" && first > 0 {
        sonos::play_track(h, ip, &g.coordinator_uuid, first).await?;
    }
    Ok(Queued {
        added: total,
        placed,
    })
}

// ------------------------------------------------------------------ overview / floorplan

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupOverview {
    group: Group,
    state: Option<PlayerState>,
    /// per-speaker volume by UUID
    volumes: HashMap<String, u32>,
}

/// Every group's playback state and every speaker's volume, fetched in parallel.
#[tauri::command]
async fn overview(app: State<'_, App>) -> Res<Vec<GroupOverview>> {
    let groups = app.groups.lock().await.clone();
    let mut set = tokio::task::JoinSet::new();
    for g in groups {
        let http = app.http.clone();
        set.spawn(async move {
            let (st, volumes) = tokio::join!(
                sonos::state(&http, &g.coordinator_ip),
                sonos::member_volumes(&http, &g.members)
            );
            GroupOverview {
                group: g,
                state: st.ok(),
                volumes,
            }
        });
    }
    let mut out = set.join_all().await;
    out.sort_by(|a, b| {
        a.group
            .name
            .to_lowercase()
            .cmp(&b.group.name.to_lowercase())
    });
    Ok(out)
}

/// Put a speaker into another group (it follows that group's coordinator).
#[tauri::command]
async fn join_group(app: State<'_, App>, member_ip: String, coordinator_uuid: String) -> Res<()> {
    sonos::soap(
        &app.http,
        &member_ip,
        Svc::AVTransport,
        "SetAVTransportURI",
        &format!(
            "<CurrentURI>x-rincon:{}</CurrentURI><CurrentURIMetaData></CurrentURIMetaData>",
            sonos::esc(&coordinator_uuid)
        ),
    )
    .await
    .map(|_| ())
}

/// Take a speaker out of its group so it plays on its own.
#[tauri::command]
async fn leave_group(app: State<'_, App>, member_ip: String) -> Res<()> {
    sonos::soap(
        &app.http,
        &member_ip,
        Svc::AVTransport,
        "BecomeCoordinatorOfStandaloneGroup",
        "",
    )
    .await
    .map(|_| ())
}

/// Change who plays together and come back with the new groups once the
/// speakers agree: `leave` speakers go solo, `join` speakers follow `into`.
/// Waiting here keeps the UI from showing the old grouping for a moment.
#[tauri::command]
async fn regroup(
    app: State<'_, App>,
    join: Vec<String>,
    into: Option<String>,
    leave: Vec<String>,
) -> Res<Vec<Group>> {
    let mut calls = tokio::task::JoinSet::new();
    for ip in leave.iter().cloned() {
        let http = app.http.clone();
        calls.spawn(async move {
            sonos::soap(&http, &ip, Svc::AVTransport, "BecomeCoordinatorOfStandaloneGroup", "").await
        });
    }
    if let Some(lead) = &into {
        let body = format!(
            "<CurrentURI>x-rincon:{}</CurrentURI><CurrentURIMetaData></CurrentURIMetaData>",
            sonos::esc(lead)
        );
        for ip in join.iter().cloned() {
            let (http, body) = (app.http.clone(), body.clone());
            calls.spawn(async move {
                sonos::soap(&http, &ip, Svc::AVTransport, "SetAVTransportURI", &body).await
            });
        }
    }
    let failed = calls.join_all().await.into_iter().find_map(|r| r.err());

    let settled = |groups: &[Group]| {
        let group_of = |ip: &String| groups.iter().find(|g| g.members.iter().any(|m| &m.ip == ip));
        leave.iter().all(|ip| group_of(ip).is_some_and(|g| g.members.len() == 1))
            && join.iter().all(|ip| {
                group_of(ip).is_some_and(|g| Some(&g.coordinator_uuid) == into.as_ref())
            })
    };
    // the speakers need a moment to agree on the new topology
    let mut groups = reread_groups(&app).await?;
    for _ in 0..16 {
        if failed.is_some() || settled(&groups) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        groups = reread_groups(&app).await?;
    }
    *app.groups.lock().await = groups.clone();
    match failed {
        Some(e) => Err(e),
        None => Ok(groups),
    }
}

/// Who is grouped with whom, asked from a speaker we already know (no network scan).
async fn reread_groups(app: &App) -> Res<Vec<Group>> {
    let ips: Vec<String> = app
        .groups
        .lock()
        .await
        .iter()
        .flat_map(|g| g.members.iter().map(|m| m.ip.clone()))
        .collect();
    for ip in ips {
        if let Ok(groups) = sonos::topology(&app.http, &ip).await {
            return Ok(groups);
        }
    }
    Err("No speaker answered".into())
}

/// One floorplan per Sonos system: floorplan-<household>.json
async fn floorplan_path(app: &App) -> PathBuf {
    floorplan_file(app, &app.household().await)
}

fn floorplan_file(app: &App, household: &str) -> PathBuf {
    let hh: String = household
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    app.settings_path
        .with_file_name(format!("floorplan-{hh}.json"))
}

/// Floorplan image (data URL) + speaker positions, stored as opaque JSON.
#[tauri::command]
async fn get_floorplan(app: State<'_, App>) -> Res<Option<String>> {
    let path = floorplan_path(&app).await;
    if let Ok(s) = std::fs::read_to_string(&path) {
        return Ok(Some(s));
    }
    // pre-multi-system single floorplan.json: adopt it for the system whose speakers it pins
    let legacy = app.settings_path.with_file_name("floorplan.json");
    if let Ok(s) = std::fs::read_to_string(&legacy) {
        let ours = app
            .groups
            .lock()
            .await
            .iter()
            .flat_map(|g| g.members.iter())
            .any(|m| s.contains(&m.uuid));
        if ours {
            let _ = std::fs::rename(&legacy, &path);
            return Ok(Some(s));
        }
    }
    Ok(None)
}

#[tauri::command]
async fn save_floorplan(app: State<'_, App>, handle: AppHandle, json: String) -> Res<()> {
    std::fs::write(floorplan_path(&app).await, json).map_err(|e| e.to_string())?;
    share(&app, handle, app.household().await);
    Ok(())
}

// ------------------------------------------------------------------ AI playlists

/// The Anthropic key for the active system: the one saved on this Mac, kept in
/// step with the one the system shares. `refresh` looks the shared one up now.
async fn ai_key(app: &App, refresh: bool) -> Option<String> {
    if ai::via_cli() {
        return Some("claude-cli".into()); // development: answered by the local Claude Code CLI
    }
    let hh = app.household().await;
    if hh.is_empty() {
        return None;
    }
    let (local, local_only) = {
        let s = app.settings.lock().await;
        (s.ai_keys.get(&hh).cloned(), s.local_only)
    };
    if local_only {
        return local;
    }
    let stale = app
        .ai_checked
        .lock()
        .await
        .get(&hh)
        .is_none_or(|at| at.elapsed() > Duration::from_secs(300));
    if !(refresh || stale) {
        return local;
    }
    match sync::pull_ai_key(&app.http, &hh).await {
        Ok(shared) => {
            app.ai_checked.lock().await.insert(hh.clone(), std::time::Instant::now());
            if shared != local {
                let mut s = app.settings.lock().await;
                match &shared {
                    Some(k) => drop(s.ai_keys.insert(hh, k.clone())),
                    None => drop(s.ai_keys.remove(&hh)),
                }
                let _ = write_settings(app, &s);
            }
            shared
        }
        Err(e) => {
            eprintln!("ai key: {e}"); // offline: the saved one still works
            local
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiStatus {
    /// a key is saved for this system
    ready: bool,
    /// the key is shared with everyone on this Sonos system
    shared: bool,
}

#[tauri::command]
async fn ai_status(app: State<'_, App>) -> Res<AiStatus> {
    Ok(AiStatus {
        ready: ai_key(&app, false).await.is_some(),
        shared: !app.settings.lock().await.local_only,
    })
}

/// Save (or with an empty string, remove) the Anthropic key for the active system.
#[tauri::command]
async fn ai_set_key(app: State<'_, App>, key: String) -> Res<AiStatus> {
    let hh = app.household().await;
    if hh.is_empty() {
        return Err("Find your speakers first: the key is saved per Sonos system.".into());
    }
    let key = key.trim().to_string();
    if !key.is_empty() {
        ai::check_key(&app.http, &key).await?;
    }
    let shared = {
        let mut s = app.settings.lock().await;
        if key.is_empty() {
            s.ai_keys.remove(&hh);
        } else {
            s.ai_keys.insert(hh.clone(), key.clone());
        }
        write_settings(&app, &s)?;
        !s.local_only
    };
    if shared {
        sync::push_ai_key(&app.http, &hh, (!key.is_empty()).then_some(key.as_str())).await?;
        app.ai_checked.lock().await.insert(hh, std::time::Instant::now());
    }
    Ok(AiStatus { ready: !key.is_empty(), shared })
}

const NO_AI_KEY: &str = "NO_AI_KEY";

/// Run `call` with the system's key. If Anthropic rejects it, someone may have
/// replaced the shared key since: fetch it again and try once more.
async fn with_ai_key<T, F, Fut>(app: &App, call: F) -> Res<T>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Res<T>>,
{
    let key = ai_key(app, false).await.ok_or(NO_AI_KEY)?;
    match call(key.clone()).await {
        Err(e) if e == ai::BAD_KEY => match ai_key(app, true).await {
            Some(fresh) if fresh != key => call(fresh).await,
            _ => Err(e),
        },
        other => other,
    }
}

/// Looks songs Claude named up on Spotify, a few at a time. With `run`, each
/// result is also reported to the page waiting for that request.
struct Lookups {
    handle: AppHandle,
    run: Option<u32>,
    gate: std::sync::Arc<tokio::sync::Semaphore>,
    set: tokio::task::JoinSet<(usize, Option<SpItem>)>,
}

impl Lookups {
    fn new(handle: &AppHandle, run: Option<u32>) -> Self {
        Self { handle: handle.clone(), run, gate: std::sync::Arc::new(tokio::sync::Semaphore::new(5)), set: Default::default() }
    }

    fn start(&mut self, index: usize, p: ai::Pick) {
        let (handle, gate, run) = (self.handle.clone(), self.gate.clone(), self.run);
        self.set.spawn(async move {
            let _slot = gate.acquire().await;
            let app = handle.state::<App>();
            let s = app.settings.lock().await.clone();
            let found = app
                .spotify
                .find_track(&app.http, &creds(&s), &p.title, &p.artist, market(&s))
                .await;
            let item = found.ok().flatten();
            if let Some(run) = run {
                let _ = handle.emit("ai-playlist", AiProgress::Found { run, index, item: item.clone() });
            }
            (index, item)
        });
    }

    /// Everything started, in the order of `index`; `len` is how many songs there are.
    async fn finish(self, len: usize) -> Vec<Option<SpItem>> {
        let mut out = vec![None; len];
        for (i, item) in self.set.join_all().await {
            if let Some(slot) = out.get_mut(i) {
                *slot = item;
            }
        }
        out
    }
}

async fn find_tracks(handle: &AppHandle, picks: Vec<ai::Pick>) -> Vec<Option<SpItem>> {
    let (len, mut lookups) = (picks.len(), Lookups::new(handle, None));
    for (i, p) in picks.into_iter().enumerate() {
        lookups.start(i, p);
    }
    lookups.finish(len).await
}

/// What the waiting page shows while a playlist is being made; `run` is the
/// caller's number for the request, so a page ignores someone else's progress.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "step")]
enum AiProgress {
    /// the playlist as far as Claude has written it; `complete` once every song is named
    Plan { run: u32, title: Option<String>, description: Option<String>, picks: Vec<ai::Pick>, complete: bool },
    /// one lookup finished; `item` is None when Spotify doesn't have the song
    Found { run: u32, index: usize, item: Option<SpItem> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiPlaylist {
    title: String,
    description: String,
    tracks: Vec<SpItem>,
    /// what Claude named, for asking it to change the playlist later
    picks: Vec<ai::Pick>,
    /// songs Spotify didn't have
    missing: usize,
}

/// A playlist from a text prompt. With `previous`, `request` says how to change that one.
#[tauri::command]
async fn ai_playlist(
    handle: AppHandle,
    app: State<'_, App>,
    request: String,
    context: String,
    previous: Option<ai::Previous>,
    run: u32,
) -> Res<AiPlaylist> {
    let country = market(&*app.settings.lock().await).to_string();
    let context = format!("{context}\nCountry: {country}");
    // songs are looked up on Spotify as soon as Claude has named them, while it is still writing
    let (drafts, mut heard) = tokio::sync::mpsc::unbounded_channel::<ai::Draft>();
    let asking = async {
        let plan = with_ai_key(&app, |key| {
            let (request, context, previous) = (request.clone(), context.clone(), previous.clone());
            let (http, drafts) = (app.http.clone(), drafts.clone());
            async move {
                ai::playlist(&http, &key, &request, &context, previous.as_ref(), move |d| {
                    let _ = drafts.send(d);
                })
                .await
            }
        })
        .await;
        drop(drafts);
        plan
    };
    let looking = async {
        let (mut lookups, mut started) = (Lookups::new(&handle, Some(run)), Vec::<ai::Pick>::new());
        while let Some(d) = heard.recv().await {
            for (i, p) in d.tracks.iter().enumerate().skip(started.len()) {
                lookups.start(i, p.clone());
            }
            if d.tracks.len() > started.len() {
                started = d.tracks.clone();
            }
            let _ = handle.emit(
                "ai-playlist",
                AiProgress::Plan { run, title: d.title, description: d.description, picks: started.clone(), complete: false },
            );
        }
        (lookups, started)
    };
    let (plan, (mut lookups, started)) = tokio::join!(asking, looking);
    let plan = plan?;
    // normally the finished playlist just continues what was shown; if it doesn't, start the lookups over
    if !plan.tracks.starts_with(&started) {
        lookups = Lookups::new(&handle, Some(run));
    }
    let done = if plan.tracks.starts_with(&started) { started.len() } else { 0 };
    let _ = handle.emit(
        "ai-playlist",
        AiProgress::Plan {
            run,
            title: Some(plan.title.clone()),
            description: Some(plan.description.clone()),
            picks: plan.tracks.clone(),
            complete: true,
        },
    );
    for (i, p) in plan.tracks.iter().enumerate().skip(done) {
        lookups.start(i, p.clone());
    }
    let found = lookups.finish(plan.tracks.len()).await;
    let missing = found.iter().filter(|t| t.is_none()).count();
    let mut seen = std::collections::HashSet::new();
    let (mut tracks, mut picks) = (vec![], vec![]);
    for (pick, item) in plan.tracks.into_iter().zip(found) {
        // the same recording can come back for two differently named picks
        if let Some(item) = item.filter(|t| seen.insert(t.id.clone())) {
            tracks.push(item);
            picks.push(pick);
        }
    }
    if tracks.is_empty() {
        return Err("Spotify didn't have any of the suggested songs. Try again.".into());
    }
    Ok(AiPlaylist {
        title: plan.title,
        description: plan.description,
        tracks,
        picks,
        missing,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiFollowUp {
    item: SpItem,
    why: String,
}

/// Songs that would be good right after the one that is playing.
#[tauri::command]
async fn ai_follow_ups(
    handle: AppHandle,
    app: State<'_, App>,
    title: String,
    artist: String,
    upcoming: Vec<String>,
) -> Res<Vec<AiFollowUp>> {
    let now = ai::Pick { title, artist };
    let picks = with_ai_key(&app, |key| {
        let (now, upcoming, http) = (now.clone(), upcoming.clone(), app.http.clone());
        async move { ai::follow_ups(&http, &key, &now, &upcoming).await }
    })
    .await?;
    let found = find_tracks(
        &handle,
        picks.iter().map(|p| ai::Pick { title: p.title.clone(), artist: p.artist.clone() }).collect(),
    )
    .await;
    let mut seen = std::collections::HashSet::new();
    Ok(picks
        .into_iter()
        .zip(found)
        .filter_map(|(p, item)| {
            let item = item.filter(|t| seen.insert(t.id.clone()))?;
            Some(AiFollowUp { item, why: p.why })
        })
        .collect())
}

// ------------------------------------------------------------------ sharing

/// What this machine knows about a system, as shared with the others.
async fn snapshot(app: &App, household: &str) -> sync::Shared {
    let s = app.settings.lock().await;
    sync::Shared {
        floorplan: std::fs::read_to_string(floorplan_file(app, household)).ok(),
        name: s.household_names.get(household).cloned(),
        spotify: s.spotify_overrides.get(household).copied(),
    }
}

/// Upload in the background after a local edit.
fn share(app: &App, handle: AppHandle, household: String) {
    use std::sync::atomic::Ordering;
    app.local_rev.fetch_add(1, Ordering::SeqCst);
    if household.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let app = handle.state::<App>();
        if app.settings.lock().await.local_only {
            return;
        }
        let _guard = app.sync_lock.lock().await;
        let shared = snapshot(&app, &household).await;
        match sync::push(&app.http, &household, &shared).await {
            Ok(etag) => drop(app.sync_etags.lock().await.insert(household, etag)),
            Err(e) => eprintln!("sync push: {e}"),
        }
    });
}

/// Fetch what others shared for the active system. True when something changed
/// (the frontend then reloads the floorplan and system names).
#[tauri::command]
async fn sync_pull(app: State<'_, App>) -> Res<bool> {
    use std::sync::atomic::Ordering;
    let hh = app.household().await;
    if hh.is_empty() || app.settings.lock().await.local_only {
        return Ok(false);
    }
    let _guard = app.sync_lock.lock().await;
    let rev = app.local_rev.load(Ordering::SeqCst);
    let etag = app.sync_etags.lock().await.get(&hh).cloned();
    let pulled = match sync::pull(&app.http, &hh, etag.as_deref()).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("sync pull: {e}"); // offline is fine, keep the local copy
            return Ok(false);
        }
    };
    match pulled {
        sync::Pulled::Unchanged => Ok(false),
        sync::Pulled::Missing => {
            // first one here: share what we have
            let shared = snapshot(&app, &hh).await;
            if !shared.is_empty() {
                if let Ok(etag) = sync::push(&app.http, &hh, &shared).await {
                    app.sync_etags.lock().await.insert(hh, etag);
                }
            }
            Ok(false)
        }
        sync::Pulled::Doc { etag, shared } => {
            if app.local_rev.load(Ordering::SeqCst) != rev {
                return Ok(false); // edited meanwhile; that edit's push wins
            }
            let changed = shared != snapshot(&app, &hh).await;
            if changed {
                if let Some(fp) = &shared.floorplan {
                    std::fs::write(floorplan_file(&app, &hh), fp).map_err(|e| e.to_string())?;
                }
                let mut s = app.settings.lock().await;
                match shared.name {
                    Some(n) => drop(s.household_names.insert(hh.clone(), n)),
                    None => drop(s.household_names.remove(&hh)),
                }
                match shared.spotify {
                    Some(o) => drop(s.spotify_overrides.insert(hh.clone(), o)),
                    None => drop(s.spotify_overrides.remove(&hh)),
                }
                write_settings(&app, &s)?;
                app.links.lock().await.remove(&hh);
            }
            app.sync_etags.lock().await.insert(hh, etag);
            Ok(changed)
        }
    }
}

#[tauri::command]
async fn spotify_check_app(app: State<'_, App>, id: String, secret: String) -> Res<()> {
    spotify::check_app(&app.http, id.trim(), secret.trim()).await
}

/// Open a pasted Spotify link without logging in.
#[tauri::command]
async fn spotify_resolve(app: State<'_, App>, link: String) -> Res<SpItem> {
    spotify::resolve(&app.http, &link).await
}

// ------------------------------------------------------------------ votes + added by

async fn social_ctx(app: &App) -> Option<(String, String)> {
    let hh = app.household().await;
    let s = app.settings.lock().await;
    (!hh.is_empty() && !s.local_only).then(|| (hh, s.device_id.clone()))
}

async fn store_social(app: &App, hh: String, etag: String, tracks: Vec<sync::TrackSocial>) -> Vec<sync::TrackSocial> {
    app.social.lock().await.insert(hh, (etag, tracks.clone()));
    tracks
}

/// Votes and who added what on the active system, newest first.
#[tauri::command]
async fn get_social(app: State<'_, App>) -> Res<Vec<sync::TrackSocial>> {
    let Some((hh, device)) = social_ctx(&app).await else { return Ok(vec![]) };
    let cached = app.social.lock().await.get(&hh).cloned();
    match sync::social(&app.http, &hh, &device, cached.as_ref().map(|c| c.0.as_str())).await {
        Ok(Some((etag, tracks))) => Ok(store_social(&app, hh, etag, tracks).await),
        Ok(None) => Ok(cached.map(|c| c.1).unwrap_or_default()),
        Err(e) => {
            eprintln!("social: {e}"); // offline: keep showing the last known state
            Ok(cached.map(|c| c.1).unwrap_or_default())
        }
    }
}

/// `value` 1 (up), -1 (down) or 0 (take back).
#[tauri::command]
async fn vote_track(
    app: State<'_, App>,
    track: sync::TrackMeta,
    value: i8,
    person: Option<sync::Person>,
) -> Res<Vec<sync::TrackSocial>> {
    let (hh, device) = social_ctx(&app).await.ok_or("Voting needs sharing turned on in Settings → Sonos")?;
    let change = sync::Change { person: person.as_ref(), vote: Some((&track, value.clamp(-1, 1))), react: None, added: &[] };
    let (etag, tracks) = sync::change(&app.http, &hh, &device, change).await?;
    Ok(store_social(&app, hh, etag, tracks).await)
}

/// A fun emoji reaction ("poo", "unicorn", …), or `None` to take yours back.
#[tauri::command]
async fn react_track(
    app: State<'_, App>,
    track: sync::TrackMeta,
    emoji: Option<String>,
    person: Option<sync::Person>,
) -> Res<Vec<sync::TrackSocial>> {
    let (hh, device) = social_ctx(&app).await.ok_or("Reactions need sharing turned on in Settings → Sonos")?;
    let change = sync::Change { person: person.as_ref(), vote: None, react: Some((&track, emoji.as_deref())), added: &[] };
    let (etag, tracks) = sync::change(&app.http, &hh, &device, change).await?;
    Ok(store_social(&app, hh, etag, tracks).await)
}

/// Remember who added these songs, so others see a face next to them in the queue.
#[tauri::command]
async fn mark_added(app: State<'_, App>, tracks: Vec<sync::TrackMeta>, person: sync::Person) -> Res<Vec<sync::TrackSocial>> {
    let Some((hh, device)) = social_ctx(&app).await else { return Ok(vec![]) };
    if tracks.is_empty() {
        return Ok(vec![]);
    }
    let change = sync::Change { person: Some(&person), vote: None, react: None, added: &tracks };
    let (etag, all) = sync::change(&app.http, &hh, &device, change).await?;
    Ok(store_social(&app, hh, etag, all).await)
}

// ------------------------------------------------------------------ boot

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&dir)?;
            let settings_path = dir.join("settings.json");
            let mut settings: Settings = std::fs::read_to_string(&settings_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            // older versions stored the built-in ID; treat it as "use the default"
            if settings.spotify_client_id == DEFAULT_CLIENT_ID {
                settings.spotify_client_id.clear();
            }
            if settings.spotify_client_secret.is_empty() {
                settings.spotify_client_secret =
                    std::env::var("SPOTIFY_CLIENT_SECRET").unwrap_or_default();
            }
            if settings.device_id.is_empty() {
                use rand::RngExt;
                settings.device_id = (0..16).map(|_| format!("{:02x}", rand::rng().random::<u8>())).collect();
                let _ = std::fs::write(&settings_path, serde_json::to_string_pretty(&settings)?);
            }
            if settings.market.is_empty() {
                settings.market = "DK".into();
            }
            app.manage(App {
                http: reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(3))
                    .build()?,
                spotify: Spotify::new(dir.join("spotify.json")),
                settings: Mutex::new(settings),
                settings_path,
                groups: Default::default(),
                household: Default::default(),
                links: Default::default(),
                sync_etags: Default::default(),
                local_rev: Default::default(),
                sync_lock: Default::default(),
                ai_checked: Default::default(),
                social: Default::default(),
            });
            tray::setup(app.handle())?;
            app.set_menu(tray::app_menu(app.handle())?)?;
            app.on_menu_event(|app, e| {
                if matches!(e.id().as_ref(), "about" | "check-updates" | "settings") {
                    tray::show(app);
                    let _ = app.emit("tray", e.id().as_ref().to_string());
                }
            });
            Ok(())
        })
        // closing the window keeps the app alive in the menu bar; Quit lives in the tray menu
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            discover,
            get_state,
            get_queue,
            get_members,
            set_member_volume,
            control,
            seek,
            set_volume,
            set_mute,
            set_play_mode,
            set_crossfade,
            play_index,
            remove_index,
            move_index,
            clear_queue,
            spotify_link,
            spotify_search,
            spotify_children,
            spotify_login,
            spotify_cancel_login,
            spotify_logout,
            spotify_me,
            spotify_playlists,
            spotify_player_queue,
            queue_tracks,
            spotify_add_to_player_queue,
            queue_spotify,
            tray::tray_update,
            tray::build_info,
            overview,
            join_group,
            leave_group,
            regroup,
            get_floorplan,
            save_floorplan,
            spotify_resolve,
            spotify_check_app,
            sync_pull,
            ai_status,
            ai_set_key,
            ai_playlist,
            ai_follow_ups,
            get_social,
            vote_track,
            react_track,
            mark_added,
            set_household,
            rename_household,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, e| {
            // clicking the Dock icon brings a hidden window back
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = e {
                tray::show(app);
            }
            let _ = (app, e);
        });
}
