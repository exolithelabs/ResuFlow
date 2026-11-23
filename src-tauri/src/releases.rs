use serde_json::{json, Value};
use std::cmp::Ordering;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPOSITORY: &str = "exolithelabs/resuflow";
const RELEASES_API: &str = "https://api.github.com/repos/exolithelabs/resuflow/releases/latest";
const RELEASE_URL_PREFIX: &str = "https://github.com/exolithelabs/resuflow/releases/tag/";
const CACHE_DURATION: Duration = Duration::from_secs(60 * 60);

struct CachedRelease {
    value: Value,
    expires_at: Instant,
}

static CACHE: OnceLock<Mutex<Option<CachedRelease>>> = OnceLock::new();

pub fn check_latest_release(use_cache: bool) -> Result<Value, String> {
    check_latest_release_with(CURRENT_VERSION, use_cache, fetch_github_release)
}

pub fn check_latest_release_with<F>(
    current_version: &str,
    use_cache: bool,
    fetcher: F,
) -> Result<Value, String>
where
    F: FnOnce(&str) -> Result<(u16, String), String>,
{
    let cache_mutex = CACHE.get_or_init(|| Mutex::new(None));
    if use_cache {
        if let Ok(guard) = cache_mutex.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.expires_at > Instant::now() {
                    return Ok(cached.value.clone());
                }
            }
        }
    }

    let (status, body) = fetcher(RELEASES_API)?;
    if status == 404 {
        let val = json!({
            "checked": true,
            "available": false,
            "currentVersion": current_version,
            "latestVersion": null,
        });
        store_cache(cache_mutex, &val, use_cache);
        return Ok(val);
    }
    if !(200..300).contains(&status) {
        return Err(format!("GitHub release check failed ({status})."));
    }

    let payload: Value = serde_json::from_str(&body)
        .map_err(|_| "GitHub returned an invalid release payload.".to_string())?;
    let tag_name = payload
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let html_url = payload
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let Some(latest_version) = normalize_version(tag_name) else {
        return Err("GitHub returned an invalid release payload.".to_string());
    };
    let Some(release_url) = validate_release_url(html_url) else {
        return Err("GitHub returned an invalid release payload.".to_string());
    };

    let available = compare_versions(&latest_version, current_version)? == Ordering::Greater;
    let name = payload
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(tag_name);
    let published_at = payload.get("published_at").and_then(|v| v.as_str());

    let val = json!({
        "checked": true,
        "available": available,
        "currentVersion": current_version,
        "latestVersion": latest_version,
        "tagName": tag_name,
        "name": name,
        "publishedAt": published_at,
        "url": release_url,
    });
    store_cache(cache_mutex, &val, use_cache);
    Ok(val)
}

fn store_cache(cache_mutex: &Mutex<Option<CachedRelease>>, value: &Value, enabled: bool) {
    if enabled {
        if let Ok(mut guard) = cache_mutex.lock() {
            *guard = Some(CachedRelease {
                value: value.clone(),
                expires_at: Instant::now() + CACHE_DURATION,
            });
        }
    }
}

fn fetch_github_release(url: &str) -> Result<(u16, String), String> {
    let output = Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "5",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: resuflow",
            "-H",
            "X-GitHub-Api-Version: 2026-03-10",
            "-w",
            "\n%{http_code}",
            url,
        ])
        .output()
        .map_err(|e| format!("Failed to invoke curl: {e}"))?;

    if !output.status.success() {
        return Err("Network request failed.".to_string());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some((body, code_str)) = text.rsplit_once('\n') else {
        return Err("Unexpected response format.".to_string());
    };
    let status = code_str
        .trim()
        .parse::<u16>()
        .map_err(|_| "Invalid HTTP status code.".to_string())?;
    Ok((status, body.to_string()))
}

#[derive(Debug, Clone)]
struct ParsedVersion {
    numbers: [u64; 3],
    prerelease: Vec<String>,
}

pub fn compare_versions(left: &str, right: &str) -> Result<Ordering, String> {
    let (Some(a), Some(b)) = (parse_version(left), parse_version(right)) else {
        return Err(format!("Cannot compare invalid versions: {left}, {right}"));
    };
    for i in 0..3 {
        match a.numbers[i].cmp(&b.numbers[i]) {
            Ordering::Equal => continue,
            ord => return Ok(ord),
        }
    }
    Ok(compare_prerelease(&a.prerelease, &b.prerelease))
}

fn normalize_version(value: &str) -> Option<String> {
    let parsed = parse_version(value)?;
    let base = format!(
        "{}.{}.{}",
        parsed.numbers[0], parsed.numbers[1], parsed.numbers[2]
    );
    if parsed.prerelease.is_empty() {
        Some(base)
    } else {
        Some(format!("{base}-{}", parsed.prerelease.join(".")))
    }
}

fn parse_version(value: &str) -> Option<ParsedVersion> {
    let trimmed = value.trim();
    let without_v = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let without_build = without_v.split('+').next().unwrap_or(without_v);
    let (core, pre_opt) = match without_build.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (without_build, None),
    };

    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let n0 = parts[0].parse::<u64>().ok()?;
    let n1 = parts[1].parse::<u64>().ok()?;
    let n2 = parts[2].parse::<u64>().ok()?;

    let mut prerelease = Vec::new();
    if let Some(pre) = pre_opt {
        if pre.is_empty()
            || !pre
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return None;
        }
        prerelease = pre.split('.').map(|s| s.to_string()).collect();
    }

    Some(ParsedVersion {
        numbers: [n0, n1, n2],
        prerelease,
    })
}

fn compare_prerelease(left: &[String], right: &[String]) -> Ordering {
    if left.is_empty() && right.is_empty() {
        return Ordering::Equal;
    }
    if left.is_empty() {
        return Ordering::Greater;
    }
    if right.is_empty() {
        return Ordering::Less;
    }

    let max_len = left.len().max(right.len());
    for i in 0..max_len {
        let Some(l) = left.get(i) else {
            return Ordering::Less;
        };
        let Some(r) = right.get(i) else {
            return Ordering::Greater;
        };
        if l == r {
            continue;
        }
        let l_num = l.parse::<u64>().ok();
        let r_num = r.parse::<u64>().ok();
        match (l_num, r_num) {
            (Some(ln), Some(rn)) => return ln.cmp(&rn),
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (None, None) => return l.cmp(r),
        }
    }
    Ordering::Equal
}

fn validate_release_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.starts_with(RELEASE_URL_PREFIX) && !trimmed.contains(' ') && trimmed.starts_with("https://") {
        let _ = REPOSITORY;
        Some(trimmed.to_string())
    } else {
        None
    }
}
