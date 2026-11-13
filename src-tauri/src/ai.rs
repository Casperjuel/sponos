//! The music curator: a playlist from a text prompt, and follow-up songs for
//! whatever is playing. Claude names real songs; the caller looks each one up
//! on Spotify, so only songs that exist end up in the app.

use crate::sonos::Res;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const API: &str = "https://api.anthropic.com/v1";
const VERSION: &str = "2026-10-21";
const MODEL: &str = "claude-opus-5-5";

pub const BAD_KEY: &str = "Anthropic didn't accept the API key. Check it under Settings → AI playlists.";

/// A song as Claude names it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pick {
    pub title: String,
    pub artist: String,
}

#[derive(Deserialize, Debug)]
pub struct Plan {
    pub title: String,
    pub description: String,
    pub tracks: Vec<Pick>,
}

/// A playlist that is still being written: what has arrived so far.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Draft {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tracks: Vec<Pick>,
}

/// The playlist someone wants changed ("more upbeat", "no 80s").
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Previous {
    pub prompt: String,
    pub title: String,
    pub description: String,
    pub tracks: Vec<Pick>,
}

#[derive(Deserialize, Debug)]
pub struct FollowUp {
    pub title: String,
    pub artist: String,
    /// a few words on why it fits after the current song
    pub why: String,
}

#[derive(Deserialize)]
struct FollowUps {
    tracks: Vec<FollowUp>,
}

const CURATOR: &str = "\
You are the music curator inside Sponos, a desktop app that plays Spotify on the Sonos speakers of a shared space, \
most often an office. Someone describes the music they want in their own words, and you answer with a playlist. \
The app looks every song up on Spotify by title and artist, so a song only plays if it really exists there.

What makes a good answer:
- Real, released songs, named the way Spotify lists them: the song title without remix or remaster notes unless \
that version is the point, and the main artist only.
- Every wish in the request is honoured, including what they don't want. \"No heavy bass\" or \"nothing sad\" rules \
songs out even when they are otherwise a perfect fit.
- The room matters. Many people hear this at once while they work or talk, so prefer songs most of them can enjoy \
or ignore comfortably, unless the request asks for something else.
- The order is part of the playlist: it should flow from one song to the next, with no jarring jumps in energy.
- Variety: at most two songs by the same artist, and a mix of songs people know and songs they will be glad to discover.
- If the request gives a length or a number of songs, follow it. Otherwise pick 25 songs.

Also give the playlist a short title (five words at most, no quotes) and one sentence that says what it sounds like. \
Write the title and the sentence in the language of the request.";

const SELECTOR: &str = "\
You are the music curator inside Sponos, a desktop app that plays Spotify on the Sonos speakers of a shared space, \
most often an office. You are told which song is playing and what is already queued, and you suggest songs that \
would be good to hear right after it. The app looks every song up on Spotify by title and artist, so a song only \
plays if it really exists there.

Suggest six songs. Each should keep the mood and energy of the current song without being more of the same: \
different artists from the current one and from each other, nothing that is already queued, and a mix of obvious \
companions and less expected ones. Name songs the way Spotify lists them (plain title, main artist only). For each, \
say in a few words why it follows well, written for someone glancing at a list, for example \"same lazy groove\".";

fn pick_schema(extra: Value) -> Value {
    let mut props = json!({ "title": { "type": "string" }, "artist": { "type": "string" } });
    let mut required = vec!["title", "artist"];
    if let Some(more) = extra.as_object() {
        for (k, v) in more {
            props[k] = v.clone();
        }
        required.extend(more.keys().map(String::as_str));
    }
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn user(text: String) -> Value {
    json!({ "role": "user", "content": text })
}

/// A playlist for `request`. With `previous`, `request` says how that playlist should change.
/// `progress` hears about the playlist while it is being written: the title, then song by song.
pub async fn playlist(
    http: &reqwest::Client,
    key: &str,
    request: &str,
    context: &str,
    previous: Option<&Previous>,
    progress: impl Fn(Draft) + Send,
) -> Res<Plan> {
    let schema = json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "description": { "type": "string" },
            "tracks": { "type": "array", "items": pick_schema(Value::Null) },
        },
        "required": ["title", "description", "tracks"],
        "additionalProperties": false,
    });
    let messages = match previous {
        None => vec![user(format!("{request}\n\n{context}"))],
        Some(p) => vec![
            user(format!("{}\n\n{context}", p.prompt)),
            json!({ "role": "assistant", "content": json!({ "title": p.title, "description": p.description, "tracks": p.tracks }).to_string() }),
            user(format!(
                "Change the playlist: {request}\n\nKeep the songs that still fit, and answer with the whole playlist again."
            )),
        ],
    };
    let mut last = Draft::default();
    let mut on_text = move |text: &str| {
        let now = draft(text);
        if now != last {
            last = now.clone();
            progress(now);
        }
    };
    let plan: Plan = ask(http, key, CURATOR, messages, schema, &mut on_text).await?;
    if plan.tracks.is_empty() {
        return Err("No songs came back. Try describing the music a little more.".into());
    }
    Ok(plan)
}

/// Songs that would follow `now` well. `upcoming` is what the queue already holds.
pub async fn follow_ups(http: &reqwest::Client, key: &str, now: &Pick, upcoming: &[String]) -> Res<Vec<FollowUp>> {
    let schema = json!({
        "type": "object",
        "properties": { "tracks": { "type": "array", "items": pick_schema(json!({ "why": { "type": "string" } })) } },
        "required": ["tracks"],
        "additionalProperties": false,
    });
    let queued = if upcoming.is_empty() { "nothing".to_string() } else { upcoming.join("; ") };
    let text = format!("Playing now: {} by {}\nAlready queued: {queued}", now.title, now.artist);
    let out: FollowUps = ask(http, key, SELECTOR, vec![user(text)], schema, &mut |_| {}).await?;
    Ok(out.tracks)
}

/// Is this key accepted? Listing models costs nothing.
pub async fn check_key(http: &reqwest::Client, key: &str) -> Res<()> {
    // the look-alikes people paste most often
    if key.starts_with("sk-ant-admin") {
        return Err("That is an Admin key, which can't run Claude. Create a normal API key instead (it starts with sk-ant-api).".into());
    }
    if key.starts_with("sk-ant-oat") || key.starts_with("sk-ant-sid") {
        return Err("That is a login token for the Claude apps, not an API key. Create an API key in the Anthropic console (it starts with sk-ant-api).".into());
    }
    if !key.starts_with("sk-ant-") {
        return Err("That doesn't look like an Anthropic API key. They start with sk-ant-api.".into());
    }
    let r = http
        .get(format!("{API}/models?limit=1"))
        .header("x-api-key", key)
        .header("anthropic-version", VERSION)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach Anthropic: {e}"))?;
    let status = r.status().as_u16();
    if status == 200 {
        return Ok(());
    }
    let v: Value = r.json().await.unwrap_or(Value::Null);
    let said = v["error"]["message"].as_str().unwrap_or("no reason given");
    Err(match status {
        401 => format!(
            "Anthropic says this key isn't valid (\"{said}\"). It may be cut off, deleted or disabled: copy the whole key again right after creating it."
        ),
        403 => format!("Anthropic knows this key but won't let it be used here (\"{said}\"). Check the key's workspace and permissions."),
        _ => format!("Anthropic answered HTTP {status} (\"{said}\"). Try again in a moment."),
    })
}

/// Development only: with `SPONOS_AI_CLI=1`, a debug build answers through the
/// Claude Code CLI on this Mac (`claude -p`) instead of the API, so the feature
/// can be tried with a Claude login and no API key. Release builds never do this.
pub fn via_cli() -> bool {
    cfg!(debug_assertions) && std::env::var("SPONOS_AI_CLI").is_ok_and(|v| !v.is_empty() && v != "0")
}

async fn ask_cli<T: for<'de> Deserialize<'de>>(
    system: &str,
    messages: &[Value],
    schema: &Value,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Res<T> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::{Command, Stdio};
    // the CLI takes one prompt: earlier turns go in as a labelled transcript
    let prompt = match messages {
        [only] => only["content"].as_str().unwrap_or_default().to_string(),
        many => many
            .iter()
            .map(|m| format!("[{}]\n{}", m["role"].as_str().unwrap_or("user"), m["content"].as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    let (system, schema) = (system.to_string(), schema.to_string());
    let (lines, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let child = tokio::task::spawn_blocking(move || -> Res<()> {
        let mut child = Command::new("claude")
            .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
            .args(["--no-session-persistence", "--strict-mcp-config"])
            .args(["--tools", "", "--setting-sources", "", "--model", "opus"])
            .args(["--system-prompt", &system, "--json-schema", &schema])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Couldn't start the claude CLI: {e}"))?;
        child.stdin.take().ok_or("no stdin")?.write_all(prompt.as_bytes()).map_err(|e| e.to_string())?;
        for line in BufReader::new(child.stdout.take().ok_or("no stdout")?).lines().map_while(Result::ok) {
            let _ = lines.send(line);
        }
        let mut said = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut said);
        }
        if !child.wait().map_err(|e| e.to_string())?.success() {
            return Err(format!("claude CLI failed: {}", said.trim()));
        }
        Ok(())
    });
    // one JSON object per line: the answer as it is written, then the result
    let (mut text, mut result) = (String::new(), Value::Null);
    while let Some(line) = rx.recv().await {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v["type"].as_str() {
            Some("stream_event") => match v["event"]["type"].as_str() {
                Some("message_start") => text.clear(),
                Some("content_block_delta") => {
                    if let Some(more) = v["event"]["delta"]["partial_json"].as_str() {
                        text.push_str(more);
                        on_text(&text);
                    }
                }
                _ => {}
            },
            Some("result") => result = v,
            _ => {}
        }
    }
    child.await.map_err(|e| e.to_string())??;
    if result["is_error"].as_bool() == Some(true) {
        return Err(format!("claude CLI: {}", result["result"].as_str().unwrap_or("failed")));
    }
    serde_json::from_value(result["structured_output"].clone()).map_err(|e| format!("Couldn't read the answer: {e}"))
}

/// One Messages request whose answer is JSON in the shape of `schema`.
async fn ask<T: for<'de> Deserialize<'de>>(
    http: &reqwest::Client,
    key: &str,
    system: &str,
    messages: Vec<Value>,
    schema: Value,
    // hears the answer's text so far, each time more of it arrives
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Res<T> {
    if via_cli() {
        return ask_cli(system, &messages, &schema, on_text).await;
    }
    let body = json!({
        "model": MODEL,
        "max_tokens": 16000,
        "system": system,
        "messages": messages,
        // picking songs is taste, not a hard problem: low effort keeps the wait short
        "output_config": { "effort": "low", "format": { "type": "json_schema", "schema": schema } },
        // if a safety classifier declines the request, Anthropic re-runs it on another model
        "fallbacks": "default",
        "stream": true,
    });
    let mut last = String::new();
    for attempt in 0..3u64 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(800 * attempt)).await;
        }
        let sent = http
            .post(format!("{API}/messages"))
            .header("x-api-key", key)
            .header("anthropic-version", VERSION)
            .header("anthropic-beta", "server-side-fallback-2026-07-01")
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await;
        let r = match sent {
            Ok(r) => r,
            Err(e) => {
                last = format!("Couldn't reach Anthropic: {e}");
                continue;
            }
        };
        let status = r.status().as_u16();
        if status == 200 {
            // once songs have been shown, a broken stream is an error, not a quiet second try
            return receive(r, on_text).await;
        }
        let v: Value = r.json().await.unwrap_or(Value::Null);
        match status {
            401 | 403 => return Err(BAD_KEY.into()),
            // rate limited, overloaded or a server error: worth another try
            408 | 409 | 429 | 500..=599 => {
                last = match status {
                    429 => "The AI key is being used a lot right now. Try again in a minute.".into(),
                    _ => "Anthropic is busy right now. Try again in a moment.".into(),
                }
            }
            _ => {
                let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                return Err(format!("Anthropic answered HTTP {status}: {msg}"));
            }
        }
    }
    Err(last)
}

/// The streamed answer (server-sent events) of one Messages request.
#[derive(Default)]
struct Stream {
    text: String,
    stop: Option<String>,
}

impl Stream {
    /// Take one event in; true when the text grew.
    fn event(&mut self, v: &Value) -> Res<bool> {
        match v["type"].as_str() {
            // a fallback model starts the answer over
            Some("message_start") => self.text.clear(),
            Some("content_block_delta") => {
                if let Some(more) = v["delta"]["text"].as_str().filter(|_| v["delta"]["type"] == "text_delta") {
                    self.text.push_str(more);
                    return Ok(true);
                }
            }
            Some("message_delta") => {
                if let Some(why) = v["delta"]["stop_reason"].as_str() {
                    self.stop = Some(why.to_string());
                }
            }
            Some("error") => {
                return Err(match v["error"]["type"].as_str() {
                    Some("overloaded_error" | "api_error" | "rate_limit_error") => "Anthropic is busy right now. Try again in a moment.".into(),
                    _ => format!("Anthropic stopped the answer: {}", v["error"]["message"].as_str().unwrap_or("unknown error")),
                })
            }
            _ => {}
        }
        Ok(false)
    }

    fn finish<T: for<'de> Deserialize<'de>>(&self) -> Res<T> {
        match self.stop.as_deref() {
            Some("refusal") => return Err("That request was declined. Try wording it differently.".into()),
            Some("max_tokens") => return Err("The answer got too long. Ask for fewer songs.".into()),
            _ => {}
        }
        if self.text.is_empty() {
            return Err("The answer came back empty. Try again.".into());
        }
        serde_json::from_str(&self.text).map_err(|e| format!("Couldn't read the answer: {e}"))
    }
}

async fn receive<T: for<'de> Deserialize<'de>>(mut r: reqwest::Response, on_text: &mut (dyn FnMut(&str) + Send)) -> Res<T> {
    let (mut stream, mut buf) = (Stream::default(), Vec::<u8>::new());
    while let Some(chunk) = r.chunk().await.map_err(|e| format!("The answer from Anthropic was cut off: {e}"))? {
        buf.extend_from_slice(&chunk);
        while let Some(end) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=end).collect();
            let Some(data) = std::str::from_utf8(&line).ok().and_then(|l| l.trim().strip_prefix("data:")) else { continue };
            let Ok(v) = serde_json::from_str::<Value>(data.trim()) else { continue };
            if stream.event(&v)? {
                on_text(&stream.text);
            }
        }
    }
    stream.finish()
}

/// JSON that is still being written, closed at its last finished value: the
/// title once its closing quote is there, each song once its `}` is.
fn partial(text: &str) -> Option<Value> {
    let (mut open, mut in_str, mut esc) = (Vec::<char>::new(), false, false);
    // was the string being read a value (after a colon) rather than a key?
    let (mut last, mut value) = (' ', false);
    let mut best: Option<(usize, Vec<char>)> = None;
    for (i, c) in text.char_indices() {
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => {
                    in_str = false;
                    if value {
                        best = Some((i + 1, open.clone()));
                    }
                }
                _ => {}
            }
            continue;
        }
        match c {
            '"' => (in_str, value) = (true, last == ':'),
            '{' | '[' => open.push(c),
            '}' | ']' => {
                open.pop();
                best = Some((i + 1, open.clone()));
            }
            _ => {}
        }
        if !c.is_whitespace() {
            last = c;
        }
    }
    let (end, open) = best?;
    let closed: String = open.iter().rev().map(|c| if *c == '{' { '}' } else { ']' }).collect();
    serde_json::from_str(&format!("{}{closed}", &text[..end])).ok()
}

fn draft(text: &str) -> Draft {
    let Some(v) = partial(text) else { return Draft::default() };
    Draft {
        title: v["title"].as_str().map(String::from),
        description: v["description"].as_str().map(String::from),
        tracks: v["tracks"]
            .as_array()
            .map(|t| t.iter().filter_map(|p| serde_json::from_value(p.clone()).ok()).collect())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_streamed_answer_after_thinking() {
        let mut s = Stream::default();
        let events = [
            json!({ "type": "message_start", "message": {} }),
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "thinking_delta", "thinking": "hm" } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "{\"title\":\"Friday\",\"description\":\"Easy.\"," } }),
            json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "text_delta", "text": "\"tracks\":[{\"title\":\"Tessellate\",\"artist\":\"alt-J\"}]}" } }),
            json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" } }),
        ];
        let grew: Vec<bool> = events.iter().map(|e| s.event(e).unwrap()).collect();
        assert_eq!(grew, [false, false, true, true, false]);
        let plan: Plan = s.finish().unwrap();
        assert_eq!((plan.title.as_str(), plan.tracks.len()), ("Friday", 1));
        assert_eq!(plan.tracks[0].artist, "alt-J");
    }

    #[test]
    fn a_refusal_is_an_error_not_a_parse_failure() {
        let mut s = Stream::default();
        s.event(&json!({ "type": "message_delta", "delta": { "stop_reason": "refusal" } })).unwrap();
        assert!(s.finish::<Plan>().unwrap_err().contains("declined"));
        let busy = Stream::default().event(&json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }));
        assert!(busy.unwrap_err().contains("busy"));
    }

    #[test]
    fn a_playlist_shows_up_piece_by_piece() {
        let whole = r#"{"title": "Fri \"day\" {1}", "description": "Easy, warm.", "tracks": [{"title": "Tessellate", "artist": "alt-J"}, {"title": "Kids [live]", "artist": "MGMT"}]}"#;
        let at = |upto: &str| draft(&whole[..whole.find(upto).unwrap() + upto.len()]);
        assert_eq!(draft(""), Draft::default());
        assert_eq!(at(r#""title": "Fri \"da"#), Draft::default());
        assert_eq!(at("{1}\"").title.as_deref(), Some("Fri \"day\" {1}"));
        assert_eq!(at("\"description\": \"Ea").description, None);
        let one = at(r#""alt-J"}, {"title": "Kids [li"#);
        assert_eq!((one.description.as_deref(), one.tracks.len()), (Some("Easy, warm."), 1));
        // a song counts when it is finished, not when its title is
        assert_eq!(at(r#""Kids [live]", "artist": "MG"#).tracks.len(), 1);
        let all = draft(whole);
        assert_eq!((all.tracks.len(), all.tracks[1].title.as_str()), (2, "Kids [live]"));
    }

    #[test]
    fn follow_up_schema_requires_the_reason() {
        let s = pick_schema(json!({ "why": { "type": "string" } }));
        assert_eq!(s["required"], json!(["title", "artist", "why"]));
        assert_eq!(s["additionalProperties"], json!(false));
    }

    /// SPONOS_AI_CLI=1 cargo test --lib ai::tests::cli -- --ignored --nocapture  (uses the local Claude Code login)
    #[tokio::test]
    #[ignore]
    async fn cli_playlist_change_and_follow_ups() {
        assert!(via_cli(), "set SPONOS_AI_CLI=1");
        let http = reqwest::Client::new();
        let ask = "I want a friday playlist for the office, it should be chill no heavy bass, and fit for people in their start 30's. 8 songs.";
        let plan = playlist(&http, "", ask, "Local time: Friday 14:05\nCountry: DK", None, |d| println!("… {:?} / {} songs", d.title, d.tracks.len())).await.unwrap();
        println!("{plan:#?}");
        assert_eq!(plan.tracks.len(), 8);
        let prev = Previous { prompt: ask.into(), title: plan.title, description: plan.description, tracks: plan.tracks };
        let changed = playlist(&http, "", "a bit more upbeat, and make it 10 songs", "Local time: Friday 14:05\nCountry: DK", Some(&prev), |_| {}).await.unwrap();
        println!("{changed:#?}");
        assert_eq!(changed.tracks.len(), 10);
        let next = follow_ups(&http, "", &Pick { title: "Tessellate".into(), artist: "alt-J".into() }, &["Breezeblocks by alt-J".into()]).await.unwrap();
        println!("{next:#?}");
        assert!(next.len() >= 4 && next.iter().all(|t| t.artist != "alt-J"));
    }

    /// ANTHROPIC_API_KEY=… cargo test --lib ai -- --ignored --nocapture  (spends a few cents)
    #[tokio::test]
    #[ignore]
    async fn live_playlist_and_follow_ups() {
        let key = std::env::var("ANTHROPIC_API_KEY").unwrap();
        let http = reqwest::Client::new();
        check_key(&http, &key).await.unwrap();
        let plan = playlist(&http, &key, "Friday afternoon in the office, chill, no heavy bass, people in their early 30s. 8 songs.", "Country: DK", None, |d| println!("… {:?} / {} songs", d.title, d.tracks.len()))
            .await
            .unwrap();
        println!("{plan:#?}");
        assert_eq!(plan.tracks.len(), 8);
        let next = follow_ups(&http, &key, &Pick { title: "Tessellate".into(), artist: "alt-J".into() }, &[]).await.unwrap();
        println!("{next:#?}");
        assert!(!next.is_empty());
    }
}
