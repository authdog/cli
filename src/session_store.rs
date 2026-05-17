//! Persist CLI credentials under the OS config dir (`~/.config/authdog-cli` on Linux).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
}

fn config_dir() -> Result<PathBuf> {
    let d = directories::ProjectDirs::from("com", "Authdog", "authdog-cli")
        .context("could not resolve config directory")?;
    let p = d.config_dir().to_path_buf();
    fs::create_dir_all(&p).with_context(|| format!("mkdir {}", p.display()))?;
    Ok(p)
}

pub fn credentials_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("credentials.json"))
}

pub fn load_session() -> Result<Option<StoredSession>> {
    let path = credentials_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let s: StoredSession = serde_json::from_str(&raw).context("invalid credentials.json")?;
    Ok(Some(s))
}

pub fn save_session(sess: &StoredSession) -> Result<()> {
    let path = credentials_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = fs::File::create(&path).with_context(|| format!("create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    let json = serde_json::to_string_pretty(sess).context("serialize StoredSession")?;
    f.write_all(json.as_bytes())?;
    Ok(())
}

pub fn clear_session() -> Result<()> {
    let path = credentials_path()?;
    if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_session_json_roundtrip() {
        let s = StoredSession {
            access_token: "token-a".into(),
            refresh_token: "token-r".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StoredSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.access_token, "token-a");
        assert_eq!(back.refresh_token, "token-r");
    }
}
