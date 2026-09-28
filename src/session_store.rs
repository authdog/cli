//! Persist CLI credentials under the OS config dir (`~/.config/authdog-cli` on Linux).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
}

fn config_dir() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("AUTHDOG_CONFIG_DIR").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        fs::create_dir_all(&path).with_context(|| format!("mkdir {}", path.display()))?;
        return Ok(path);
    }
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
    load_session_from(&path)
}

pub fn load_session_from(path: &Path) -> Result<Option<StoredSession>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let s: StoredSession = serde_json::from_str(&raw).context("invalid credentials.json")?;
    Ok(Some(s))
}

pub fn save_session(sess: &StoredSession) -> Result<()> {
    let path = credentials_path()?;
    save_session_to(&path, sess)
}

pub fn save_session_to(path: &Path, sess: &StoredSession) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(sess).context("serialize StoredSession")?;
    let mut last_error = None;
    for attempt in 0..100 {
        let temp_path = path.with_extension(format!("tmp-{}-{attempt}", std::process::id()));
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("create {}", temp_path.display()));
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        if let Err(error) = file
            .write_all(json.as_bytes())
            .and_then(|()| file.sync_all())
            .and_then(|()| replace_file(&temp_path, path))
        {
            let _ = fs::remove_file(&temp_path);
            return Err(error).with_context(|| format!("replace {}", path.display()));
        }
        return Ok(());
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("could not allocate temporary file")))
        .with_context(|| format!("replace {}", path.display()))
}

fn replace_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp_path, path)
}

pub fn clear_session() -> Result<()> {
    let path = credentials_path()?;
    clear_session_at(&path)
}

pub fn clear_session_at(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
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

    #[test]
    fn atomic_store_roundtrip_and_clear_use_injected_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        let session = StoredSession {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
        };

        save_session_to(&path, &session).unwrap();
        let loaded = load_session_from(&path).unwrap().unwrap();
        assert_eq!(loaded.access_token, "access");
        assert_eq!(loaded.refresh_token, "refresh");

        clear_session_at(&path).unwrap();
        assert!(load_session_from(&path).unwrap().is_none());
    }
}
