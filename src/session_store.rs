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
    /// Organization id (**`/organizations`** or **`/browse`**); narrower scopes invalidate when changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_organization_id: Option<String>,
    /// Tenant uuid selected for scoped commands (`/projects`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_tenant_id: Option<String>,
    /// Project (application) id; cleared when organization or tenant scope changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_application_id: Option<String>,
    /// Environment id; cleared when organization, tenant, or application scope changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_environment_id: Option<String>,
}

impl StoredSession {
    pub fn clear_context(&mut self) {
        self.current_organization_id = None;
        self.current_tenant_id = None;
        self.current_application_id = None;
        self.current_environment_id = None;
    }

    pub fn set_organization_id(&mut self, organization_id: Option<String>) {
        if !optional_id_scope_matches(&self.current_organization_id, &organization_id) {
            self.current_tenant_id = None;
            self.current_application_id = None;
            self.current_environment_id = None;
        }
        self.current_organization_id = organization_id;
    }

    pub fn set_tenant_id(&mut self, tenant_id: Option<String>) {
        if !optional_id_scope_matches(&self.current_tenant_id, &tenant_id) {
            self.current_application_id = None;
            self.current_environment_id = None;
        }
        self.current_tenant_id = tenant_id;
    }

    pub fn set_application_id(&mut self, application_id: Option<String>) {
        if !optional_id_scope_matches(&self.current_application_id, &application_id) {
            self.current_environment_id = None;
        }
        self.current_application_id = application_id;
    }

    pub fn set_environment_id(&mut self, environment_id: Option<String>) {
        self.current_environment_id = environment_id;
    }
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

/// Update **`credentials.json`** with the current application (project) id.
///
/// Changing project (including clearing it) resets **`current_environment_id`**, since it refers to
/// the previous application’s environment scope.
pub fn set_current_application_id(application_id: Option<String>) -> Result<()> {
    mutate_session(|session| session.set_application_id(application_id))
}

/// Update **`credentials.json`** with the current project environment id.
pub fn set_current_environment_id(environment_id: Option<String>) -> Result<()> {
    mutate_session(|session| session.set_environment_id(environment_id))
}

fn optional_id_scope_matches(existing: &Option<String>, incoming: &Option<String>) -> bool {
    match (existing.as_ref(), incoming.as_ref()) {
        (None, None) => true,
        (Some(x), Some(y)) => x.trim() == y.trim(),
        _ => false,
    }
}

/// Update **`credentials.json`** with a new current organization id (must already be logged in).
///
/// Changing or clearing organization resets **`current_tenant_id`**, **`current_application_id`**, and
/// **`current_environment_id`**, since they belong to prior org-scoped navigation.
pub fn set_current_organization_id(organization_id: Option<String>) -> Result<()> {
    mutate_session(|session| session.set_organization_id(organization_id))
}

/// Update **`credentials.json`** with a new current tenant id (must already be logged in).
///
/// Changing the tenant (including clearing it) resets **`current_application_id`** and
/// **`current_environment_id`**, since they refer to resources under the previous tenant.
pub fn set_current_tenant_id(tenant_id: Option<String>) -> Result<()> {
    mutate_session(|session| session.set_tenant_id(tenant_id))
}

pub fn clear_current_context() -> Result<()> {
    mutate_session(StoredSession::clear_context)
}

fn mutate_session(update: impl FnOnce(&mut StoredSession)) -> Result<()> {
    let mut s = load_session()?.context("not logged in (no credentials.json)")?;
    update(&mut s);
    save_session(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_id_scope_matches_trims_and_handles_none() {
        assert!(optional_id_scope_matches(&None, &None));
        assert!(optional_id_scope_matches(
            &Some("id".into()),
            &Some("id".into())
        ));
        assert!(optional_id_scope_matches(
            &Some(" id ".into()),
            &Some("id".into())
        ));
        assert!(!optional_id_scope_matches(
            &Some("a".into()),
            &Some("b".into())
        ));
        assert!(!optional_id_scope_matches(&None, &Some("x".into())));
        assert!(!optional_id_scope_matches(&Some("x".into()), &None));
    }

    #[test]
    fn stored_session_json_roundtrip() {
        let s = StoredSession {
            access_token: "token-a".into(),
            refresh_token: "token-r".into(),
            current_organization_id: Some("org-uuid".into()),
            current_tenant_id: Some("tenant-uuid".into()),
            current_application_id: Some("app-uuid".into()),
            current_environment_id: Some("env-uuid".into()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StoredSession = serde_json::from_str(&json).unwrap();
        assert_eq!(back.access_token, "token-a");
        assert_eq!(back.current_organization_id.as_deref(), Some("org-uuid"));
        assert_eq!(back.current_tenant_id.as_deref(), Some("tenant-uuid"));
        assert_eq!(back.current_application_id.as_deref(), Some("app-uuid"));
        assert_eq!(back.current_environment_id.as_deref(), Some("env-uuid"));
    }

    #[test]
    fn stored_session_json_without_organization_defaults_to_none() {
        let json = r#"{"access_token":"a","refresh_token":"b","current_tenant_id":"t"}"#;
        let back: StoredSession = serde_json::from_str(json).unwrap();
        assert!(back.current_organization_id.is_none());
        assert_eq!(back.current_tenant_id.as_deref(), Some("t"));
    }

    #[test]
    fn context_changes_clear_only_descendants() {
        let mut session = StoredSession {
            access_token: "a".into(),
            refresh_token: "r".into(),
            current_organization_id: Some("org-a".into()),
            current_tenant_id: Some("tenant-a".into()),
            current_application_id: Some("project-a".into()),
            current_environment_id: Some("environment-a".into()),
        };

        session.set_tenant_id(Some("tenant-b".into()));
        assert_eq!(session.current_organization_id.as_deref(), Some("org-a"));
        assert_eq!(session.current_tenant_id.as_deref(), Some("tenant-b"));
        assert!(session.current_application_id.is_none());
        assert!(session.current_environment_id.is_none());

        session.current_application_id = Some("project-b".into());
        session.current_environment_id = Some("environment-b".into());
        session.set_organization_id(Some("org-b".into()));
        assert_eq!(session.current_organization_id.as_deref(), Some("org-b"));
        assert!(session.current_tenant_id.is_none());
        assert!(session.current_application_id.is_none());
        assert!(session.current_environment_id.is_none());

        session.current_tenant_id = Some("tenant-c".into());
        session.clear_context();
        assert!(session.current_organization_id.is_none());
        assert!(session.current_tenant_id.is_none());
    }

    #[test]
    fn atomic_store_roundtrip_and_clear_use_injected_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("credentials.json");
        let session = StoredSession {
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            current_organization_id: None,
            current_tenant_id: Some("tenant".into()),
            current_application_id: None,
            current_environment_id: None,
        };

        save_session_to(&path, &session).unwrap();
        let loaded = load_session_from(&path).unwrap().unwrap();
        assert_eq!(loaded.access_token, "access");
        assert_eq!(loaded.current_tenant_id.as_deref(), Some("tenant"));

        clear_session_at(&path).unwrap();
        assert!(load_session_from(&path).unwrap().is_none());
    }
}
