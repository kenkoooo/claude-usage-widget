use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Credentials ────────────────────────────────────────────────────────────────

#[derive(Deserialize, serde::Serialize)]
struct Credentials {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: OAuthCreds,
}

#[derive(Deserialize, serde::Serialize)]
struct OAuthCreds {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: u64, // milliseconds since epoch
    #[serde(flatten)]
    rest: serde_json::Value, // preserve all other fields
}

fn home_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    PathBuf::from(home)
}

fn credentials_path() -> PathBuf {
    home_dir().join(".claude").join(".credentials.json")
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_credentials() -> Result<Credentials, String> {
    let content = fs::read_to_string(credentials_path())
        .map_err(|e| format!("credentials read error: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("credentials parse error: {e}"))
}

fn save_credentials(creds: &Credentials) -> Result<(), String> {
    let json = serde_json::to_string_pretty(creds)
        .map_err(|e| format!("credentials serialize error: {e}"))?;
    fs::write(credentials_path(), json).map_err(|e| format!("credentials write error: {e}"))
}

struct CurlResponse {
    status: u16,
    body: String,
}

fn run_curl(args: &[&str]) -> Result<CurlResponse, String> {
    const STATUS_MARKER: &str = "__CLAUDE_USAGE_HTTP_STATUS__:";

    let output = Command::new("curl")
        .args(["-sS", "--max-time", "10"])
        .args(args)
        .args(["-w", &format!("\n{STATUS_MARKER}%{{http_code}}")])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let body = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !stderr.is_empty() {
            return Err(format!("curl failed: {stderr}"));
        }
        if !body.is_empty() {
            return Err(format!("curl failed: {body}"));
        }
        return Err("curl failed with no output".to_string());
    }

    let stdout =
        String::from_utf8(output.stdout).map_err(|e| format!("curl output decode error: {e}"))?;
    let (body, status) = stdout
        .rsplit_once(&format!("\n{STATUS_MARKER}"))
        .ok_or_else(|| "curl output missing HTTP status".to_string())?;
    let status = status
        .trim()
        .parse::<u16>()
        .map_err(|e| format!("invalid HTTP status: {e}"))?;

    Ok(CurlResponse {
        status,
        body: body.to_string(),
    })
}

fn format_http_error(prefix: &str, status: u16, body: &str) -> String {
    let fallback_body = body.trim().to_string();
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error")
                .and_then(|error| error.get("message"))
                .and_then(|message| message.as_str())
                .map(ToOwned::to_owned)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or(fallback_body);

    if message.is_empty() {
        format!("{prefix} (HTTP {status})")
    } else {
        format!("{prefix} (HTTP {status}): {message}")
    }
}

/// Get a valid access token, refreshing if expired.
fn get_access_token() -> Result<String, String> {
    let mut creds = read_credentials()?;

    // Add 60s buffer to avoid using a token that's about to expire
    if now_millis() + 60_000 < creds.claude_ai_oauth.expires_at {
        return Ok(creds.claude_ai_oauth.access_token.clone());
    }

    // Token expired — refresh it
    let new_oauth = refresh_token(&creds.claude_ai_oauth.refresh_token)?;
    creds.claude_ai_oauth.access_token = new_oauth.access_token;
    creds.claude_ai_oauth.refresh_token = new_oauth.refresh_token;
    creds.claude_ai_oauth.expires_at = new_oauth.expires_at;
    save_credentials(&creds)?;

    Ok(creds.claude_ai_oauth.access_token.clone())
}

// ── Token Refresh ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64, // seconds
}

fn refresh_token(refresh_token: &str) -> Result<OAuthTokens, String> {
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"
    })
    .to_string();

    let response = run_curl(&[
        "-X",
        "POST",
        "-H",
        "Content-Type: application/json",
        "-d",
        &body,
        "https://console.anthropic.com/v1/oauth/token",
    ])?;

    if response.status >= 400 {
        return Err(format_http_error(
            "token refresh failed",
            response.status,
            &response.body,
        ));
    }

    let resp: RefreshResponse =
        serde_json::from_str(&response.body).map_err(|e| format!("refresh parse error: {e}"))?;

    Ok(OAuthTokens {
        access_token: resp.access_token,
        refresh_token: resp.refresh_token,
        expires_at: now_millis() + resp.expires_in * 1000,
    })
}

struct OAuthTokens {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
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
    let auth_header = format!("Authorization: Bearer {token}");
    let response = run_curl(&[
        "-H",
        &auth_header,
        "-H",
        "anthropic-beta: oauth-2025-04-20",
        "https://api.anthropic.com/api/oauth/usage",
    ])?;

    if response.status >= 400 {
        return Err(format_http_error(
            "usage fetch failed",
            response.status,
            &response.body,
        ));
    }

    serde_json::from_str(&response.body).map_err(|e| format!("parse error: {e}"))
}

// ── Cache ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize, serde::Serialize)]
struct Cache {
    fetched_at: u64, // unix seconds
    usage: UsageResponse,
}

fn cache_path() -> PathBuf {
    home_dir()
        .join(".cache")
        .join("claude-usage-widget")
        .join("usage.json")
}

const CACHE_TTL_SECS: u64 = 300;

fn load_cache() -> Option<Cache> {
    let content = fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn is_cache_fresh(cache: &Cache) -> bool {
    now_secs().saturating_sub(cache.fetched_at) < CACHE_TTL_SECS
}

fn save_cache(usage: &UsageResponse) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let cache = Cache {
        fetched_at: now_secs(),
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
    let cached = load_cache();
    if let Some(cache) = cached.as_ref().filter(|cache| is_cache_fresh(cache)) {
        return Ok(cache.usage.clone());
    }

    let usage_result = get_access_token().and_then(|token| fetch_usage(&token));
    let usage = match usage_result {
        Ok(usage) => usage,
        Err(err) => {
            if let Some(cache) = cached {
                return Ok(cache.usage);
            }
            return Err(err);
        }
    };

    save_cache(&usage);
    Ok(usage)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let panel_mode = args.iter().any(|a| a == "--panel");

    match get_usage() {
        Ok(usage) => {
            let fh_pct = usage
                .five_hour
                .as_ref()
                .map(|w| w.utilization)
                .unwrap_or(0.0);
            let w7_pct = usage
                .seven_day
                .as_ref()
                .map(|w| w.utilization)
                .unwrap_or(0.0);
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
            let w7_elapsed_pct = usage
                .seven_day
                .as_ref()
                .map(|w| {
                    let dt = DateTime::parse_from_rfc3339(&w.resets_at)
                        .map(|d| d.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now());
                    let remaining_secs = (dt - Utc::now()).num_seconds().max(0) as f64;
                    let week_secs = 7.0 * 24.0 * 3600.0;
                    (1.0 - remaining_secs / week_secs) * 100.0
                })
                .unwrap_or(0.0);

            if panel_mode {
                println!("{fh_pct:.0}%/{w7_pct:.0}%/{w7_elapsed_pct:.0}% {w7_reset_dh}");
            } else {
                println!("Claude Code Usage");
                println!(
                    "  5-hour  {:>4.0}%  {}  resets in {}",
                    fh_pct,
                    bar(fh_pct, 20),
                    fh_reset
                );
                println!(
                    "  7-day   {:>4.0}%  {}  resets in {}",
                    w7_pct,
                    bar(w7_pct, 20),
                    w7_reset
                );
            }
        }
        Err(e) => {
            if panel_mode {
                println!("err");
            } else {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}
