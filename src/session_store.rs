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

/// Update **`credentials.json`** with the current application (project) id.
///
/// Changing project (including clearing it) resets **`current_environment_id`**, since it refers to
/// the previous application’s environment scope.
pub fn set_current_application_id(application_id: Option<String>) -> Result<()> {
    let mut s = load_session()?.context("not logged in (no credentials.json)")?;
    if !optional_id_scope_matches(&s.current_application_id, &application_id) {
        s.current_environment_id = None;
    }
    s.current_application_id = application_id;
    save_session(&s)
}

/// Update **`credentials.json`** with the current project environment id.
pub fn set_current_environment_id(environment_id: Option<String>) -> Result<()> {
    let mut s = load_session()?.context("not logged in (no credentials.json)")?;
    s.current_environment_id = environment_id;
    save_session(&s)
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
    let mut s = load_session()?.context("not logged in (no credentials.json)")?;
    if !optional_id_scope_matches(&s.current_organization_id, &organization_id) {
        s.current_tenant_id = None;
        s.current_application_id = None;
        s.current_environment_id = None;
    }
    s.current_organization_id = organization_id;
    save_session(&s)
}

/// Update **`credentials.json`** with a new current tenant id (must already be logged in).
///
/// Changing the tenant (including clearing it) resets **`current_application_id`** and
/// **`current_environment_id`**, since they refer to resources under the previous tenant.
pub fn set_current_tenant_id(tenant_id: Option<String>) -> Result<()> {
    let mut s = load_session()?.context("not logged in (no credentials.json)")?;
    if !optional_id_scope_matches(&s.current_tenant_id, &tenant_id) {
        s.current_application_id = None;
        s.current_environment_id = None;
    }
    s.current_tenant_id = tenant_id;
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
}
