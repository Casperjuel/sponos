//! Minimal Sonos UPnP client. Every speaker exposes SOAP services on
//! http://<ip>:1400 — no cloud, no auth. We only make outbound requests and
//! poll for state, so nothing here needs an inbound firewall hole (UPnP event
//! subscriptions would).

use regex::Regex;
use serde::Serialize;
use std::{collections::HashMap, net::IpAddr, sync::LazyLock, time::Duration};
use tokio::net::UdpSocket;

pub type Res<T> = Result<T, String>;

#[derive(Clone, Copy)]
pub enum Svc {
    AVTransport,
    RenderingControl,
    GroupRenderingControl,
    ZoneGroupTopology,
    ContentDirectory,
    DeviceProperties,
}

impl Svc {
    fn path(self) -> &'static str {
        match self {
            Svc::AVTransport => "/MediaRenderer/AVTransport/Control",
            Svc::RenderingControl => "/MediaRenderer/RenderingControl/Control",
            Svc::GroupRenderingControl => "/MediaRenderer/GroupRenderingControl/Control",
            Svc::ZoneGroupTopology => "/ZoneGroupTopology/Control",
            Svc::ContentDirectory => "/MediaServer/ContentDirectory/Control",
            Svc::DeviceProperties => "/DeviceProperties/Control",
        }
    }
    fn urn(self) -> &'static str {
        match self {
            Svc::AVTransport => "urn:schemas-upnp-org:service:AVTransport:1",
            Svc::RenderingControl => "urn:schemas-upnp-org:service:RenderingControl:1",
            Svc::GroupRenderingControl => "urn:schemas-upnp-org:service:GroupRenderingControl:1",
            Svc::ZoneGroupTopology => "urn:schemas-upnp-org:service:ZoneGroupTopology:1",
            Svc::ContentDirectory => "urn:schemas-upnp-org:service:ContentDirectory:1",
            Svc::DeviceProperties => "urn:schemas-upnp-org:service:DeviceProperties:1",
        }
    }
    fn has_instance(self) -> bool {
        !matches!(
            self,
            Svc::ZoneGroupTopology | Svc::ContentDirectory | Svc::DeviceProperties
        )
    }
}

// ------------------------------------------------------------------ xml bits

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn unesc(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Inner text of the first `<name ...>…</name>`, entity-decoded once.
pub fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let start = from + i;
        let after = &xml[start + open.len()..];
        // make sure we matched `<name>` / `<name attr>`, not `<nameFoo>`
        if after.starts_with('>') || after.starts_with(' ') {
            let gt = start + open.len() + after.find('>')?;
            let end = xml[gt..].find(&close)? + gt;
            return Some(unesc(&xml[gt + 1..end]));
        }
        from = start + open.len();
    }
    None
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"([\w:]+)="([^"]*)""#).unwrap());
    RE.captures_iter(attrs)
        .find(|c| &c[1] == name)
        .map(|c| unesc(&c[2]))
}

// ------------------------------------------------------------------ transport

pub async fn soap(
    http: &reqwest::Client,
    ip: &str,
    svc: Svc,
    action: &str,
    args: &str,
) -> Res<String> {
    let inst = if svc.has_instance() {
        "<InstanceID>0</InstanceID>"
    } else {
        ""
    };
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{urn}">{inst}{args}</u:{action}></s:Body></s:Envelope>"#,
        urn = svc.urn()
    );
    let res = http
        .post(format!("http://{ip}:1400{}", svc.path()))
        .header("Content-Type", r#"text/xml; charset="utf-8""#)
        .header("SOAPACTION", format!("\"{}#{action}\"", svc.urn()))
        .body(body)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| format!("{action} → {ip}: {e}"))?;
    let ok = res.status().is_success();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !ok {
        let code = tag(&text, "errorCode").unwrap_or_else(|| "?".into());
        return Err(format!("{action} failed (UPnP error {code})"));
    }
    Ok(text)
}

// ------------------------------------------------------------------ discovery

/// SSDP M-SEARCH. Replies arrive unicast on our ephemeral port; if the macOS
/// firewall eats them we fall back to the seed IPs from settings.
pub async fn ssdp(wait: Duration) -> Vec<String> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else {
        return vec![];
    };
    let msg = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:device:ZonePlayer:1\r\n\r\n";
    for _ in 0..2 {
        let _ = sock.send_to(msg.as_bytes(), "239.255.255.250:1900").await;
    }
    let mut found: Vec<String> = vec![];
    let mut buf = [0u8; 2048];
    let deadline = tokio::time::Instant::now() + wait;
    while let Ok(Ok((_, from))) = tokio::time::timeout_at(deadline, sock.recv_from(&mut buf)).await
    {
        if let IpAddr::V4(v4) = from.ip() {
            let s = v4.to_string();
            if !found.contains(&s) {
                found.push(s);
            }
        }
    }
    found
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub uuid: String,
    pub ip: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub coordinator_uuid: String,
    pub coordinator_ip: String,
    pub name: String,
    pub members: Vec<Member>,
}

pub async fn topology(http: &reqwest::Client, ip: &str) -> Res<Vec<Group>> {
    static GROUP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<ZoneGroup\s([^>]*)>(.*?)</ZoneGroup>").unwrap());
    static MEMBER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<ZoneGroupMember\s([^>]*?)/?>").unwrap());
    static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?://([\d.]+):").unwrap());

    let r = soap(http, ip, Svc::ZoneGroupTopology, "GetZoneGroupState", "").await?;
    let xml = tag(&r, "ZoneGroupState").ok_or("no ZoneGroupState")?;
    let mut groups = vec![];
    for g in GROUP.captures_iter(&xml) {
        let coord = attr(&g[1], "Coordinator").unwrap_or_default();
        let id = attr(&g[1], "ID").unwrap_or_else(|| coord.clone());
        let mut members: Vec<Member> = MEMBER
            .captures_iter(&g[2])
            .filter(|m| attr(&m[1], "Invisible").as_deref() != Some("1")) // bonded subs / surrounds
            .filter_map(|m| {
                let loc = attr(&m[1], "Location")?;
                Some(Member {
                    uuid: attr(&m[1], "UUID")?,
                    ip: HOST.captures(&loc)?[1].to_string(),
                    name: attr(&m[1], "ZoneName").unwrap_or_default(),
                })
            })
            .collect();
        // speakers list a group's rooms in whatever order they like, and it differs from
        // one speaker to the next: lead first, the rest by name (emoji in front don't count)
        members.sort_by_cached_key(|m| {
            let name = m.name.trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            (m.uuid != coord, name, m.uuid.clone())
        });
        let Some(c) = members.iter().find(|m| m.uuid == coord).cloned() else {
            continue;
        };
        let name = match members.len() {
            1 => c.name.clone(),
            n => format!("{} + {}", c.name, n - 1),
        };
        groups.push(Group {
            id,
            coordinator_uuid: c.uuid,
            coordinator_ip: c.ip,
            name,
            members,
        });
    }
    groups.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(groups)
}

/// Which Sonos system ("household") a speaker belongs to — stable across
/// networks, so it keys everything per-system (floorplan, Spotify link).
pub async fn household_id(http: &reqwest::Client, ip: &str) -> Res<String> {
    let r = soap(http, ip, Svc::DeviceProperties, "GetHouseholdID", "").await?;
    tag(&r, "CurrentHouseholdID")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "no household id".into())
}

// ------------------------------------------------------------------ DIDL

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub art: Option<String>,
    pub duration: Option<String>,
    pub uri: Option<String>,
}

fn art_url(ip: &str, raw: Option<String>) -> Option<String> {
    raw.filter(|s| !s.is_empty()).map(|s| {
        if s.starts_with('/') {
            format!("http://{ip}:1400{s}")
        } else {
            s
        }
    })
}

pub fn parse_didl(ip: &str, didl: &str) -> Vec<Item> {
    static ITEM: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)<(item|container)\s([^>]*)>(.*?)</(?:item|container)>").unwrap()
    });
    static RES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<res([^>]*)>([^<]*)</res>").unwrap());
    ITEM.captures_iter(didl)
        .map(|c| {
            let body = &c[3];
            let res = RES.captures(body);
            Item {
                id: attr(&c[2], "id").unwrap_or_default(),
                title: tag(body, "dc:title").unwrap_or_default(),
                artist: tag(body, "dc:creator"),
                album: tag(body, "upnp:album"),
                art: art_url(ip, tag(body, "upnp:albumArtURI")),
                duration: res.as_ref().and_then(|r| attr(&r[1], "duration")),
                uri: res.map(|r| unesc(&r[2])),
            }
        })
        .collect()
}

/// Browse a ContentDirectory container. Returns (items, total, raw result xml).
pub async fn browse(
    http: &reqwest::Client,
    ip: &str,
    id: &str,
    start: u32,
    count: u32,
) -> Res<(Vec<Item>, u32, String)> {
    let r = soap(
        http,
        ip,
        Svc::ContentDirectory,
        "Browse",
        &format!(
            "<ObjectID>{}</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter>\
             <StartingIndex>{start}</StartingIndex><RequestedCount>{count}</RequestedCount><SortCriteria></SortCriteria>",
            esc(id)
        ),
    )
    .await?;
    let didl = tag(&r, "Result").unwrap_or_default();
    let total = tag(&r, "TotalMatches")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok((parse_didl(ip, &didl), total, didl))
}

pub async fn queue(http: &reqwest::Client, ip: &str) -> Res<Vec<Item>> {
    let mut out = vec![];
    loop {
        let (items, total, _) = browse(http, ip, "Q:0", out.len() as u32, 200).await?;
        let n = items.len();
        out.extend(items);
        if n == 0 || out.len() as u32 >= total || out.len() >= 1000 {
            return Ok(out);
        }
    }
}

// ------------------------------------------------------------------ state

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub transport: String,
    pub play_mode: String,
    pub volume: u32,
    pub muted: bool,
    /// 1-based queue position, 0 when not playing from the queue
    pub track_no: u32,
    pub queue_active: bool,
    pub position: u32,
    pub duration: u32,
    pub track: Option<Item>,
    /// e.g. "Spotify Connect" when the queue is bypassed
    pub source: Option<String>,
    /// songs blend into each other (a setting of the room or group)
    pub crossfade: bool,
}

pub fn hms(s: &str) -> u32 {
    s.split(':')
        .filter_map(|p| p.parse::<u32>().ok())
        .fold(0, |a, p| a * 60 + p)
}

pub async fn state(http: &reqwest::Client, ip: &str) -> Res<PlayerState> {
    let (ti, pi, mi, ts, vol, mute, xf) = tokio::join!(
        soap(http, ip, Svc::AVTransport, "GetTransportInfo", ""),
        soap(http, ip, Svc::AVTransport, "GetPositionInfo", ""),
        soap(http, ip, Svc::AVTransport, "GetMediaInfo", ""),
        soap(http, ip, Svc::AVTransport, "GetTransportSettings", ""),
        soap(http, ip, Svc::GroupRenderingControl, "GetGroupVolume", ""),
        soap(http, ip, Svc::GroupRenderingControl, "GetGroupMute", ""),
        soap(http, ip, Svc::AVTransport, "GetCrossfadeMode", ""),
    );
    let ti = ti?;
    let pi = pi.unwrap_or_default();
    let media_uri = mi
        .ok()
        .and_then(|m| tag(&m, "CurrentURI"))
        .unwrap_or_default();
    let meta = tag(&pi, "TrackMetaData").unwrap_or_default();
    let track = parse_didl(ip, &meta).into_iter().next().map(|mut t| {
        // radio streams put "artist - title" in streamContent
        if let Some(sc) = tag(&meta, "r:streamContent").filter(|s| !s.is_empty()) {
            t.artist = Some(t.title.clone());
            t.title = sc;
        }
        t
    });
    let queue_active = media_uri.starts_with("x-rincon-queue:");
    let source = if queue_active {
        None
    } else if media_uri.starts_with("x-sonos-vli:") && media_uri.contains("spotify") {
        Some("Spotify Connect".into())
    } else if media_uri.starts_with("x-sonos-vli:") {
        Some("AirPlay / Connect".into())
    } else if media_uri.starts_with("x-sonos-htastream:") {
        Some("TV".into())
    } else if media_uri.starts_with("x-rincon:") {
        Some("Grouped".into())
    } else if media_uri.is_empty() {
        None
    } else {
        Some("Stream".into())
    };
    Ok(PlayerState {
        crossfade: xf.ok().and_then(|r| tag(&r, "CrossfadeMode")).as_deref() == Some("1"),
        transport: tag(&ti, "CurrentTransportState").unwrap_or_default(),
        play_mode: ts
            .ok()
            .and_then(|t| tag(&t, "PlayMode"))
            .unwrap_or_else(|| "NORMAL".into()),
        volume: vol
            .ok()
            .and_then(|v| tag(&v, "CurrentVolume"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        muted: mute.ok().and_then(|m| tag(&m, "CurrentMute")).as_deref() == Some("1"),
        track_no: tag(&pi, "Track").and_then(|t| t.parse().ok()).unwrap_or(0),
        queue_active,
        position: hms(&tag(&pi, "RelTime").unwrap_or_default()),
        duration: hms(&tag(&pi, "TrackDuration").unwrap_or_default()),
        track,
        source,
    })
}

// ------------------------------------------------------------------ control

pub async fn use_queue(http: &reqwest::Client, ip: &str, uuid: &str) -> Res<()> {
    soap(
        http,
        ip,
        Svc::AVTransport,
        "SetAVTransportURI",
        &format!("<CurrentURI>x-rincon-queue:{uuid}#0</CurrentURI><CurrentURIMetaData></CurrentURIMetaData>"),
    )
    .await
    .map(|_| ())
}

pub async fn play_track(http: &reqwest::Client, ip: &str, uuid: &str, n: u32) -> Res<()> {
    use_queue(http, ip, uuid).await?;
    soap(
        http,
        ip,
        Svc::AVTransport,
        "Seek",
        &format!("<Unit>TRACK_NR</Unit><Target>{n}</Target>"),
    )
    .await?;
    soap(http, ip, Svc::AVTransport, "Play", "<Speed>1</Speed>")
        .await
        .map(|_| ())
}

/// Returns (first track number enqueued, number added).
pub async fn enqueue(
    http: &reqwest::Client,
    ip: &str,
    uri: &str,
    meta: &str,
    at: u32,
    as_next: bool,
) -> Res<(u32, u32)> {
    let r = soap(
        http,
        ip,
        Svc::AVTransport,
        "AddURIToQueue",
        &format!(
            "<EnqueuedURI>{}</EnqueuedURI><EnqueuedURIMetaData>{}</EnqueuedURIMetaData>\
             <DesiredFirstTrackNumberEnqueued>{at}</DesiredFirstTrackNumberEnqueued><EnqueueAsNext>{}</EnqueueAsNext>",
            esc(uri),
            esc(meta),
            as_next as u8
        ),
    )
    .await?;
    let num = |n| tag(&r, n).and_then(|v| v.parse().ok()).unwrap_or(0);
    Ok((num("FirstTrackNumberEnqueued"), num("NumTracksAdded")))
}

pub async fn set_group_volume(http: &reqwest::Client, ip: &str, v: u32) -> Res<()> {
    // SnapshotGroupVolume fixes the member ratios so SetGroupVolume scales proportionally
    soap(
        http,
        ip,
        Svc::GroupRenderingControl,
        "SnapshotGroupVolume",
        "",
    )
    .await?;
    soap(
        http,
        ip,
        Svc::GroupRenderingControl,
        "SetGroupVolume",
        &format!("<DesiredVolume>{}</DesiredVolume>", v.min(100)),
    )
    .await
    .map(|_| ())
}

pub async fn member_volumes(http: &reqwest::Client, members: &[Member]) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    for m in members {
        let v = soap(
            http,
            &m.ip,
            Svc::RenderingControl,
            "GetVolume",
            "<Channel>Master</Channel>",
        )
        .await
        .ok()
        .and_then(|r| tag(&r, "CurrentVolume"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
        out.insert(m.uuid.clone(), v);
    }
    out
}

pub async fn set_member_volume(http: &reqwest::Client, ip: &str, v: u32) -> Res<()> {
    soap(
        http,
        ip,
        Svc::RenderingControl,
        "SetVolume",
        &format!(
            "<Channel>Master</Channel><DesiredVolume>{}</DesiredVolume>",
            v.min(100)
        ),
    )
    .await
    .map(|_| ())
}

// ------------------------------------------------------------------ music service

/// How this household's Sonos reaches Spotify. `sid`/`sn` vary per system
/// (S1 uses sid 12, S2 typically 9; `sn` is the account slot), so we read them
/// off something already in the queue or favourites.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SpotifyLink {
    pub sid: u32,
    pub sn: u32,
    pub desc: String,
    pub detected: bool,
}

impl SpotifyLink {
    pub fn fallback(sid: u32, sn: u32) -> Self {
        let svc = sid * 256 + 7;
        Self {
            sid,
            sn,
            desc: format!("SA_RINCON{svc}_X_#Svc{svc}-0-Token"),
            detected: false,
        }
    }

    /// kind: track | album | playlist | artist
    pub fn uri_and_meta(&self, kind: &str, id: &str, title: &str) -> Res<(String, String)> {
        let (prefix, class, container) = match kind {
            "track" => ("00032020", "object.item.audioItem.musicTrack", false),
            "album" => ("1004206c", "object.container.album.musicAlbum", true),
            "playlist" => ("1006206c", "object.container.playlistContainer", true),
            "artist" => ("100e206c", "object.container.playlistContainer", true),
            _ => return Err(format!("can't queue a {kind}")),
        };
        let sp_kind = if kind == "artist" {
            "artistTopTracks"
        } else {
            kind
        };
        let item_id = format!("{prefix}spotify%3a{sp_kind}%3a{id}");
        let uri = if container {
            format!(
                "x-rincon-cpcontainer:{item_id}?sid={}&flags=8300&sn={}",
                self.sid, self.sn
            )
        } else {
            format!(
                "x-sonos-spotify:spotify%3atrack%3a{id}?sid={}&flags=8232&sn={}",
                self.sid, self.sn
            )
        };
        let meta = format!(
            r#"<DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/" xmlns:r="urn:schemas-rinconnetworks-com:metadata-1-0/" xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/"><item id="{item_id}" parentID="" restricted="true"><dc:title>{}</dc:title><upnp:class>{class}</upnp:class><desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">{}</desc></item></DIDL-Lite>"#,
            esc(title),
            esc(&self.desc)
        );
        Ok((uri, meta))
    }
}

pub async fn detect_spotify(http: &reqwest::Client, ip: &str) -> Option<SpotifyLink> {
    static URI: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"spotify[^<\s]*?\?sid=(\d+)&(?:amp;)?flags=\d+&(?:amp;)?sn=(\d+)").unwrap()
    });
    static DESC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"SA_RINCON(\d+)_X_#Svc\d+-[^<&]*-Token").unwrap());
    let mut text = String::new();
    for id in ["FV:2", "Q:0"] {
        if let Ok((_, _, raw)) = browse(http, ip, id, 0, 100).await {
            text.push_str(&unesc(&raw));
        }
    }
    let c = URI.captures(&text)?;
    let (sid, sn): (u32, u32) = (c[1].parse().ok()?, c[2].parse().ok()?);
    let svc = (sid * 256 + 7).to_string();
    let desc = DESC
        .captures_iter(&text)
        .find(|d| d[1] == svc)
        .map(|d| d[0].to_string())
        .unwrap_or_else(|| SpotifyLink::fallback(sid, sn).desc);
    Some(SpotifyLink {
        sid,
        sn,
        desc,
        detected: true,
    })
}

#[cfg(test)]
mod live {
    //! Read-only checks against a real system: `SONOS_IP=… cargo test -- --ignored --nocapture`
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn reads_live_system() {
        let ip = std::env::var("SONOS_IP").expect("set SONOS_IP");
        let http = reqwest::Client::new();
        println!("ssdp: {:?}", ssdp(Duration::from_millis(1800)).await);
        let groups = topology(&http, &ip).await.unwrap();
        for g in &groups {
            let st = state(&http, &g.coordinator_ip).await.unwrap();
            let q = queue(&http, &g.coordinator_ip).await.unwrap();
            println!(
                "{} [{}] {:?} vol={} q={} track={:?}",
                g.name,
                st.transport,
                st.source,
                st.volume,
                q.len(),
                st.track.map(|t| t.title)
            );
        }
        println!(
            "{:?}",
            detect_spotify(&http, &groups[0].coordinator_ip).await
        );
    }
}
