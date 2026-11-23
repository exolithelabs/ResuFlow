use crate::workspace::{atomic_write_file, resolve_abs_path, WorkspaceError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_DIR: &str = ".resuflow";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    pub default_vault: String,
    pub recent_vaults: Vec<String>,
}

pub fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if !profile.is_empty() {
            return PathBuf::from(profile);
        }
    }
    PathBuf::from(".")
}

pub fn user_config_path() -> PathBuf {
    home_dir().join(CONFIG_DIR).join("config.json")
}

pub fn load_user_config() -> UserConfig {
    let file = user_config_path();
    let Ok(text) = fs::read_to_string(&file) else {
        return UserConfig::default();
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
        return UserConfig::default();
    };
    let default_vault = payload
        .get("defaultVault")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let recent_vaults = payload
        .get("recentVaults")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    UserConfig {
        default_vault,
        recent_vaults,
    }
}

pub fn save_user_config(
    default_vault: Option<String>,
    recent_vaults: Option<Vec<String>>,
) -> Result<UserConfig, WorkspaceError> {
    let current = load_user_config();
    let next = UserConfig {
        default_vault: default_vault.unwrap_or(current.default_vault),
        recent_vaults: recent_vaults.unwrap_or(current.recent_vaults),
    };
    let file = user_config_path();
    let serialized = serde_json::to_string_pretty(&json!({
        "defaultVault": next.default_vault,
        "recentVaults": next.recent_vaults,
    }))
    .map_err(|e| WorkspaceError::new(e.to_string(), 500))?;
    atomic_write_file(&file, &format!("{serialized}\n"))?;
    Ok(next)
}

pub fn remember_vault(root: &Path, set_default: bool) -> Result<UserConfig, WorkspaceError> {
    let resolved = resolve_abs_path(root).to_string_lossy().to_string();
    let current = load_user_config();
    let mut recent = vec![resolved.clone()];
    for item in current.recent_vaults {
        if item != resolved && recent.len() < 8 {
            recent.push(item);
        }
    }
    let next_default = if set_default || current.default_vault.is_empty() {
        resolved
    } else {
        current.default_vault
    };
    save_user_config(Some(next_default), Some(recent))
}
