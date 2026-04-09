#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

// --- Types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Playback {
    track_name: String,
    artist_name: String,
    album_name: String,
    album_image: Option<String>,
    duration_ms: u64,
    progress_ms: u64,
    is_playing: bool,
    track_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LyricLine {
    time_ms: u64,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollResult {
    playback: Option<Playback>,
    lyrics: Option<Vec<LyricLine>>,
    timestamp: u64,
    track_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    opacity: f64,
    accent_color: String,
    visible_lines: u32,
    pinned: bool,
    #[serde(default = "default_font_size")]
    font_size: u32,
}

fn default_font_size() -> u32 { 15 }

impl Default for Settings {
    fn default() -> Self {
        Self {
            opacity: 0.88,
            accent_color: "#3fb950".into(),
            visible_lines: 5,
            pinned: true,
            font_size: 15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenStore {
    refresh_token: String,
}

// --- State ---

struct AppState {
    http: Client,
    client_id: Mutex<Option<String>>,
    client_secret: Mutex<Option<String>>,
    access_token: Mutex<Option<String>>,
    refresh_token: Mutex<Option<String>>,
    token_expiry: Mutex<u64>,
    current_track_id: Mutex<Option<String>>,
    current_lyrics: Mutex<Option<Vec<LyricLine>>>,
    settings: Mutex<Settings>,
}

fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path().app_data_dir().unwrap()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// --- Commands: Config ---

#[tauri::command]
async fn check_credentials(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let dir = data_dir(&app);
    let path = dir.join("config.json");
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(config) = serde_json::from_str::<Config>(&data) {
            *state.client_id.lock().unwrap() = Some(config.client_id);
            *state.client_secret.lock().unwrap() = Some(config.client_secret);
            return Ok(true);
        }
    }
    Ok(false)
}

#[tauri::command]
async fn save_credentials(app: AppHandle, state: tauri::State<'_, AppState>, client_id: String, client_secret: String) -> Result<(), String> {
    let dir = data_dir(&app);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let config = Config { client_id: client_id.clone(), client_secret: client_secret.clone() };
    fs::write(dir.join("config.json"), serde_json::to_string_pretty(&config).unwrap())
        .map_err(|e| e.to_string())?;
    *state.client_id.lock().unwrap() = Some(client_id);
    *state.client_secret.lock().unwrap() = Some(client_secret);
    Ok(())
}

#[tauri::command]
async fn reset_credentials(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let dir = data_dir(&app);
    let _ = fs::remove_file(dir.join("config.json"));
    let _ = fs::remove_file(dir.join("token.json"));
    *state.client_id.lock().unwrap() = None;
    *state.client_secret.lock().unwrap() = None;
    *state.access_token.lock().unwrap() = None;
    *state.refresh_token.lock().unwrap() = None;
    *state.token_expiry.lock().unwrap() = 0;
    *state.current_track_id.lock().unwrap() = None;
    *state.current_lyrics.lock().unwrap() = None;
    Ok(())
}

// --- Commands: Settings ---

#[tauri::command]
async fn load_settings(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<Settings, String> {
    let dir = data_dir(&app);
    if let Ok(data) = fs::read_to_string(dir.join("settings.json")) {
        if let Ok(s) = serde_json::from_str::<Settings>(&data) {
            *state.settings.lock().unwrap() = s.clone();
            return Ok(s);
        }
    }
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
async fn save_settings_cmd(app: AppHandle, state: tauri::State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let dir = data_dir(&app);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("settings.json"), serde_json::to_string_pretty(&settings).unwrap())
        .map_err(|e| e.to_string())?;

    // Update window always-on-top
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_always_on_top(settings.pinned);
    }

    *state.settings.lock().unwrap() = settings.clone();
    Ok(settings)
}

// --- Commands: Auth ---

#[tauri::command]
async fn start_auth(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let client_id = state.client_id.lock().unwrap().clone().ok_or("No client_id")?;

    let auth_url = format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}",
        client_id,
        urlencoding::encode("http://127.0.0.1:8888/callback"),
        urlencoding::encode("user-read-playback-state user-read-currently-playing")
    );

    // Start callback server
    let listener = TcpListener::bind("127.0.0.1:8888").await.map_err(|e| e.to_string())?;

    // Open browser
    open::that(&auth_url).map_err(|e| e.to_string())?;

    // Wait for callback
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 4096];
        let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
        let request = String::from_utf8_lossy(&buf[..n]);

        if let Some(first_line) = request.lines().next() {
            if let Some(path) = first_line.split_whitespace().nth(1) {
                if path.contains("/callback") && path.contains("code=") {
                    let code = path.split("code=").nth(1).unwrap_or("")
                        .split('&').next().unwrap_or("").to_string();

                    let html = "<html><body style='background:#0d1117;color:#e6edf3;font-family:Segoe UI,sans-serif;display:flex;align-items:center;justify-content:center;height:100vh'><h2>Login feito! Pode fechar esta aba.</h2></body></html>";
                    let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n{}", html);
                    let _ = stream.write_all(resp.as_bytes()).await;

                    return Ok(code);
                }
            }
        }

        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await;
    }
}

#[tauri::command]
async fn exchange_code(app: AppHandle, state: tauri::State<'_, AppState>, code: String) -> Result<(), String> {
    let client_id = state.client_id.lock().unwrap().clone().ok_or("No client_id")?;
    let client_secret = state.client_secret.lock().unwrap().clone().ok_or("No client_secret")?;

    let resp = state.http.post("https://accounts.spotify.com/api/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .basic_auth(&client_id, Some(&client_secret))
        .body(format!(
            "grant_type=authorization_code&code={}&redirect_uri={}",
            code,
            urlencoding::encode("http://127.0.0.1:8888/callback")
        ))
        .send().await.map_err(|e| e.to_string())?
        .json::<serde_json::Value>().await.map_err(|e| e.to_string())?;

    let at = resp["access_token"].as_str().ok_or("No access_token")?.to_string();
    let rt = resp["refresh_token"].as_str().ok_or("No refresh_token")?.to_string();
    let exp = resp["expires_in"].as_u64().unwrap_or(3600);

    *state.access_token.lock().unwrap() = Some(at);
    *state.refresh_token.lock().unwrap() = Some(rt.clone());
    *state.token_expiry.lock().unwrap() = now_ms() + exp * 1000;

    let dir = data_dir(&app);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(dir.join("token.json"), serde_json::to_string(&TokenStore { refresh_token: rt }).unwrap())
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn ensure_auth(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<bool, String> {
    // Try loading saved token
    let dir = data_dir(&app);
    if let Ok(data) = fs::read_to_string(dir.join("token.json")) {
        if let Ok(store) = serde_json::from_str::<TokenStore>(&data) {
            *state.refresh_token.lock().unwrap() = Some(store.refresh_token);
            if refresh_token_impl(&app, &state).await {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

async fn refresh_token_impl(app: &AppHandle, state: &AppState) -> bool {
    let client_id = state.client_id.lock().unwrap().clone();
    let client_secret = state.client_secret.lock().unwrap().clone();
    let rt = state.refresh_token.lock().unwrap().clone();

    let (Some(cid), Some(cs), Some(rt)) = (client_id, client_secret, rt) else { return false };

    let Ok(resp) = state.http.post("https://accounts.spotify.com/api/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .basic_auth(&cid, Some(&cs))
        .body(format!("grant_type=refresh_token&refresh_token={}", rt))
        .send().await else { return false };

    let Ok(data) = resp.json::<serde_json::Value>().await else { return false };

    if let Some(at) = data["access_token"].as_str() {
        *state.access_token.lock().unwrap() = Some(at.to_string());
        *state.token_expiry.lock().unwrap() = now_ms() + data["expires_in"].as_u64().unwrap_or(3600) * 1000;
        if let Some(new_rt) = data["refresh_token"].as_str() {
            *state.refresh_token.lock().unwrap() = Some(new_rt.to_string());
            let dir = data_dir(app);
            let _ = fs::write(dir.join("token.json"), serde_json::to_string(&TokenStore { refresh_token: new_rt.to_string() }).unwrap());
        }
        return true;
    }
    false
}

async fn ensure_token(app: &AppHandle, state: &AppState) {
    let expiry = *state.token_expiry.lock().unwrap();
    if now_ms() >= expiry.saturating_sub(60000) {
        refresh_token_impl(app, state).await;
    }
}

// --- Commands: Spotify API ---

#[tauri::command]
async fn poll(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<PollResult, String> {
    ensure_token(&app, &state).await;

    let token = state.access_token.lock().unwrap().clone();
    let Some(token) = token else {
        return Ok(PollResult { playback: None, lyrics: None, timestamp: now_ms(), track_changed: false });
    };

    let playback = get_playback(&state.http, &token).await;

    let mut track_changed = false;
    let prev_id = state.current_track_id.lock().unwrap().clone();

    if let Some(ref pb) = playback {
        if prev_id.as_deref() != Some(&pb.track_id) {
            *state.current_track_id.lock().unwrap() = Some(pb.track_id.clone());
            *state.current_lyrics.lock().unwrap() = None;
            track_changed = true;
        }
    } else if prev_id.is_some() {
        *state.current_track_id.lock().unwrap() = None;
        *state.current_lyrics.lock().unwrap() = None;
    }

    let lyrics = state.current_lyrics.lock().unwrap().clone();

    Ok(PollResult { playback, lyrics, timestamp: now_ms(), track_changed })
}

async fn get_playback(http: &Client, token: &str) -> Option<Playback> {
    let resp = http.get("https://api.spotify.com/v1/me/player")
        .bearer_auth(token)
        .send().await.ok()?;

    if resp.status() != 200 { return None; }

    let data: serde_json::Value = resp.json().await.ok()?;
    if data.get("item").is_none() || data["currently_playing_type"].as_str()? != "track" {
        return None;
    }

    let item = &data["item"];
    let artists: Vec<&str> = item["artists"].as_array()?
        .iter().filter_map(|a| a["name"].as_str()).collect();

    let images = item["album"]["images"].as_array()?;
    let img = images.get(1).or(images.first())
        .and_then(|i| i["url"].as_str()).map(String::from);

    Some(Playback {
        track_name: item["name"].as_str()?.into(),
        artist_name: artists.join(", "),
        album_name: item["album"]["name"].as_str()?.into(),
        album_image: img,
        duration_ms: item["duration_ms"].as_u64()?,
        progress_ms: data["progress_ms"].as_u64().unwrap_or(0),
        is_playing: data["is_playing"].as_bool().unwrap_or(false),
        track_id: item["id"].as_str()?.into(),
    })
}

// --- Commands: Lyrics ---

#[tauri::command]
async fn fetch_lyrics_cmd(state: tauri::State<'_, AppState>, track_name: String, artist_name: String, album_name: String, duration: i32) -> Result<Option<Vec<LyricLine>>, String> {
    let lyrics = fetch_lyrics(&state.http, &track_name, &artist_name, &album_name, duration).await;
    *state.current_lyrics.lock().unwrap() = lyrics.clone();
    Ok(lyrics)
}

async fn fetch_lyrics(http: &Client, track_name: &str, artist_name: &str, album_name: &str, duration: i32) -> Option<Vec<LyricLine>> {
    let try_fetch = |track: String, artist: String, album: String, dur: i32| {
        let http = http.clone();
        async move {
            let mut params = vec![
                ("track_name", track.clone()),
                ("artist_name", artist.clone()),
            ];
            if !album.is_empty() { params.push(("album_name", album)); }
            if dur > 0 { params.push(("duration", dur.to_string())); }

            // Try exact match
            if let Ok(resp) = http.get("https://lrclib.net/api/get")
                .header("User-Agent", "spotify-lyrics v1.0.0")
                .query(&params).send().await {
                if resp.status() == 200 {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        if let Some(synced) = data["syncedLyrics"].as_str() {
                            return Some(parse_lrc(synced));
                        }
                    }
                }
            }

            // Try search
            if let Ok(resp) = http.get("https://lrclib.net/api/search")
                .header("User-Agent", "spotify-lyrics v1.0.0")
                .query(&[("track_name", &track), ("artist_name", &artist)])
                .send().await {
                if let Ok(results) = resp.json::<Vec<serde_json::Value>>().await {
                    for r in results {
                        if let Some(synced) = r["syncedLyrics"].as_str() {
                            return Some(parse_lrc(synced));
                        }
                    }
                }
            }

            None::<Vec<LyricLine>>
        }
    };

    // Run all attempts in parallel
    let cleaned = clean_track_name(track_name);
    let first_artist = artist_name.split(',').next().unwrap_or(artist_name).trim().to_string();

    let mut handles = vec![
        tokio::spawn(try_fetch(track_name.into(), artist_name.into(), album_name.into(), duration)),
    ];

    if cleaned != track_name {
        handles.push(tokio::spawn(try_fetch(cleaned.clone(), artist_name.into(), album_name.into(), duration)));
    }
    if first_artist != artist_name {
        handles.push(tokio::spawn(try_fetch(track_name.into(), first_artist.clone(), album_name.into(), duration)));
        if cleaned != track_name {
            handles.push(tokio::spawn(try_fetch(cleaned, first_artist, album_name.into(), duration)));
        }
    }

    // Return first non-None result
    let results = futures::future::join_all(handles).await;
    for r in results {
        if let Ok(Some(lyrics)) = r {
            return Some(lyrics);
        }
    }
    None
}

fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let re = Regex::new(r"\[(\d{2}):(\d{2})\.(\d{2,3})]\s*(.*)").unwrap();
    text.lines().filter_map(|line| {
        let caps = re.captures(line.trim())?;
        let min: u64 = caps[1].parse().ok()?;
        let sec: u64 = caps[2].parse().ok()?;
        let cs = &caps[3];
        let ms: u64 = if cs.len() == 2 { cs.parse::<u64>().ok()? * 10 } else { cs.parse().ok()? };
        Some(LyricLine {
            time_ms: (min * 60 + sec) * 1000 + ms,
            text: caps[4].trim().to_string(),
        })
    }).collect()
}

fn clean_track_name(name: &str) -> String {
    let re1 = Regex::new(r"(?i)\s*[-\x{2013}]\s*(?:Remaster(?:ed)?|Remix|Deluxe|Bonus Track|Live|Acoustic|Radio Edit).*$").unwrap();
    let re2 = Regex::new(r"(?i)\s*[(\[](feat\.?|ft\.?|with)\s+[^)\]]*[)\]]").unwrap();
    let s = re1.replace(name, "").to_string();
    re2.replace(&s, "").trim().to_string()
}

// --- Commands: Misc ---

#[tauri::command]
async fn open_external(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| e.to_string())
}

#[tauri::command]
async fn close_app(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

// --- Main ---

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            http: Client::new(),
            client_id: Mutex::new(None),
            client_secret: Mutex::new(None),
            access_token: Mutex::new(None),
            refresh_token: Mutex::new(None),
            token_expiry: Mutex::new(0),
            current_track_id: Mutex::new(None),
            current_lyrics: Mutex::new(None),
            settings: Mutex::new(Settings::default()),
        })
        .invoke_handler(tauri::generate_handler![
            check_credentials,
            save_credentials,
            reset_credentials,
            load_settings,
            save_settings_cmd,
            start_auth,
            exchange_code,
            ensure_auth,
            poll,
            fetch_lyrics_cmd,
            open_external,
            close_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
