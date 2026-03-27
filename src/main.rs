use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Credentials ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCreds,
}

#[derive(Deserialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")]
    access_token: String,
}

fn credentials_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home).join(".claude").join(".credentials.json")
}

fn read_access_token() -> Result<String, String> {
    let content = fs::read_to_string(credentials_path())
        .map_err(|e| format!("credentials read error: {e}"))?;
    let creds: Credentials =
        serde_json::from_str(&content).map_err(|e| format!("credentials parse error: {e}"))?;
    Ok(creds.claude_ai_oauth.access_token)
}

// ── Usage API ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, serde::Serialize, Clone)]
struct WindowUsage {
    utilization: f64,
    resets_at: String,
}

#[derive(Deserialize, serde::Serialize, Clone)]
struct UsageResponse {
    five_hour: Option<WindowUsage>,
    seven_day: Option<WindowUsage>,
}

fn fetch_usage(token: &str) -> Result<UsageResponse, String> {
    // Minimal HTTPS GET via TcpStream + rustls (or openssl). Instead, use
    // std::process to call curl, which is always available on Ubuntu.
    let output = std::process::Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "10",
            "-H",
            &format!("Authorization: Bearer {token}"),
            "-H",
            "anthropic-beta: oauth-2025-04-20",
            "https://api.anthropic.com/api/oauth/usage",
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {stderr}"));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| format!("parse error: {e}"))
}

// ── Cache ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize, serde::Serialize)]
struct Cache {
    fetched_at: u64, // unix seconds
    usage: UsageResponse,
}

fn cache_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
        .join(".cache")
        .join("claude-usage-widget")
        .join("usage.json")
}

const CACHE_TTL_SECS: u64 = 300;

fn load_cache() -> Option<UsageResponse> {
    let content = fs::read_to_string(cache_path()).ok()?;
    let cache: Cache = serde_json::from_str(&content).ok()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    if now - cache.fetched_at < CACHE_TTL_SECS {
        Some(cache.usage)
    } else {
        None
    }
}

fn save_cache(usage: &UsageResponse) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cache = Cache {
        fetched_at: now,
        usage: usage.clone(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = fs::write(&path, json);
    }
}

// ── Formatting ─────────────────────────────────────────────────────────────────

fn bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn resets_in(resets_at: &str) -> String {
    let Ok(dt) = DateTime::parse_from_rfc3339(resets_at) else {
        return "?".to_string();
    };
    let diff = dt.with_timezone(&Utc) - Utc::now();
    let secs = diff.num_seconds().max(0);
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h{m}m")
    } else {
        format!("{m}m")
    }
}

fn resets_in_dh(resets_at: &str) -> String {
    let Ok(dt) = DateTime::parse_from_rfc3339(resets_at) else {
        return "?".to_string();
    };
    let diff = dt.with_timezone(&Utc) - Utc::now();
    let secs = diff.num_seconds().max(0);
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    format!("{d}d{h:02}h")
}

// ── Main ───────────────────────────────────────────────────────────────────────

fn get_usage() -> Result<UsageResponse, String> {
    if let Some(cached) = load_cache() {
        return Ok(cached);
    }
    let token = read_access_token()?;
    let usage = fetch_usage(&token)?;
    save_cache(&usage);
    Ok(usage)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let panel_mode = args.iter().any(|a| a == "--panel");

    match get_usage() {
        Ok(usage) => {
            let fh_pct = usage.five_hour.as_ref().map(|w| w.utilization).unwrap_or(0.0);
            let w7_pct = usage.seven_day.as_ref().map(|w| w.utilization).unwrap_or(0.0);
            let fh_reset = usage
                .five_hour
                .as_ref()
                .map(|w| resets_in(&w.resets_at))
                .unwrap_or_else(|| "?".to_string());
            let w7_reset = usage
                .seven_day
                .as_ref()
                .map(|w| resets_in(&w.resets_at))
                .unwrap_or_else(|| "?".to_string());
            let w7_reset_dh = usage
                .seven_day
                .as_ref()
                .map(|w| resets_in_dh(&w.resets_at))
                .unwrap_or_else(|| "?".to_string());

            if panel_mode {
                // Compact single line for status bar
                println!("{fh_pct:.0}%/{w7_pct:.0}% {w7_reset_dh}");
            } else {
                // Human-readable with bars
                println!("Claude Code Usage");
                println!("  5-hour  {:>4.0}%  {}  resets in {}", fh_pct, bar(fh_pct, 20), fh_reset);
                println!("  7-day   {:>4.0}%  {}  resets in {}", w7_pct, bar(w7_pct, 20), w7_reset);
            }
        }
        Err(e) => {
            if panel_mode {
                println!("CC: err");
            } else {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}
