//! Shared application actions used by both the process CLI and Ratatui dashboard.

use crate::organizations::{fetch_organizations, organization_rows_from_body, OrgRow};
use crate::projects::{
    environment_rows_from_body, fetch_application_environments, fetch_projects,
    project_rows_from_body, EnvironmentRow, ProjectRow,
};
use crate::session_store::{
    clear_current_context, clear_session, credentials_path, load_session,
    set_current_application_id, set_current_environment_id, set_current_organization_id,
    set_current_tenant_id, StoredSession,
};
use crate::tenants::{fetch_tenants, tenant_listing_rows_from_body, TenantRow};
use crate::whoami::fetch_identity_userinfo;
use anyhow::{Context, Result};
use clap::ValueEnum;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ContextResource {
    Organization,
    Tenant,
    Project,
    Environment,
}

impl ContextResource {
    pub const ALL: [Self; 4] = [
        Self::Organization,
        Self::Tenant,
        Self::Project,
        Self::Environment,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Tenant => "tenant",
            Self::Project => "project",
            Self::Environment => "environment",
        }
    }

    pub fn descendants(self) -> &'static str {
        match self {
            Self::Organization => "tenant, project, and environment",
            Self::Tenant => "project and environment",
            Self::Project => "environment",
            Self::Environment => "no other context",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionStatus {
    pub session: Option<StoredSession>,
    pub credentials_path: PathBuf,
}

impl SessionStatus {
    pub fn logged_in(&self) -> bool {
        self.session.is_some()
    }
}

#[derive(Clone, Debug)]
pub enum ResourceRows {
    Organizations(Vec<OrgRow>),
    Tenants(Vec<TenantRow>),
    Projects(Vec<ProjectRow>),
    Environments(Vec<EnvironmentRow>),
}

#[derive(Clone, Debug, Default)]
pub struct ListScopes {
    pub organization: Option<String>,
    pub tenant: Option<String>,
    pub project: Option<String>,
}

pub fn status() -> Result<SessionStatus> {
    Ok(SessionStatus {
        session: load_session()?,
        credentials_path: credentials_path()?,
    })
}

pub fn require_session() -> Result<StoredSession> {
    load_session()?.context("not logged in; run `authdog login`")
}

pub fn identity() -> Result<Value> {
    let session = require_session()?;
    fetch_identity_userinfo(&session.access_token)
}

pub fn list_organizations() -> Result<Vec<OrgRow>> {
    let session = require_session()?;
    Ok(organization_rows_from_body(&fetch_organizations(
        &session.access_token,
    )?))
}

pub fn list_tenants(organization: Option<&str>) -> Result<Vec<TenantRow>> {
    let session = require_session()?;
    let scope = clean_scope(organization).or(session.current_organization_id.as_deref());
    let value = fetch_tenants(&session.access_token, scope)?;
    let (rows, warning) = tenant_listing_rows_from_body(&value, scope);
    if let Some(warning) = warning {
        anyhow::bail!("{warning}");
    }
    Ok(rows)
}

pub fn list_projects(tenant: Option<&str>) -> Result<Vec<ProjectRow>> {
    let session = require_session()?;
    let tenant = required_scope(
        clean_scope(tenant).or(session.current_tenant_id.as_deref()),
        "tenant",
        "--tenant",
    )?;
    Ok(project_rows_from_body(&fetch_projects(
        &session.access_token,
        tenant,
    )?))
}

pub fn list_environments(
    tenant: Option<&str>,
    project: Option<&str>,
) -> Result<Vec<EnvironmentRow>> {
    let session = require_session()?;
    let tenant = required_scope(
        clean_scope(tenant).or(session.current_tenant_id.as_deref()),
        "tenant",
        "--tenant",
    )?;
    let project = required_scope(
        clean_scope(project).or(session.current_application_id.as_deref()),
        "project",
        "--project",
    )?;
    Ok(environment_rows_from_body(&fetch_application_environments(
        &session.access_token,
        tenant,
        project,
    )?))
}

pub fn list_resources(resource: ContextResource, scopes: &ListScopes) -> Result<ResourceRows> {
    match resource {
        ContextResource::Organization => Ok(ResourceRows::Organizations(list_organizations()?)),
        ContextResource::Tenant => Ok(ResourceRows::Tenants(list_tenants(
            scopes.organization.as_deref(),
        )?)),
        ContextResource::Project => Ok(ResourceRows::Projects(list_projects(
            scopes.tenant.as_deref(),
        )?)),
        ContextResource::Environment => Ok(ResourceRows::Environments(list_environments(
            scopes.tenant.as_deref(),
            scopes.project.as_deref(),
        )?)),
    }
}

pub fn set_context(resource: ContextResource, id: String) -> Result<String> {
    let id = id.trim();
    anyhow::ensure!(!id.is_empty(), "context ID cannot be empty");
    update_context(resource, Some(id.to_string()))?;
    Ok(id.to_string())
}

pub fn clear_context(resource: Option<ContextResource>) -> Result<()> {
    match resource {
        Some(resource) => update_context(resource, None),
        None => clear_current_context(),
    }
}

pub fn logout() -> Result<()> {
    clear_session()
}

fn update_context(resource: ContextResource, id: Option<String>) -> Result<()> {
    match resource {
        ContextResource::Organization => set_current_organization_id(id),
        ContextResource::Tenant => set_current_tenant_id(id),
        ContextResource::Project => set_current_application_id(id),
        ContextResource::Environment => set_current_environment_id(id),
    }
}

fn clean_scope(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

pub fn required_scope<'a>(value: Option<&'a str>, name: &str, flag: &str) -> Result<&'a str> {
    clean_scope(value).with_context(|| {
        format!("no {name} selected; pass `{flag} ID` or run `authdog context set {name} ID`")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_resource_describes_cascade() {
        assert_eq!(
            ContextResource::Organization.descendants(),
            "tenant, project, and environment"
        );
        assert_eq!(
            ContextResource::Environment.descendants(),
            "no other context"
        );
    }

    #[test]
    fn required_scope_trims_and_rejects_empty_values() {
        assert_eq!(
            required_scope(Some(" tenant "), "tenant", "--tenant").unwrap(),
            "tenant"
        );
        assert!(required_scope(Some(" "), "tenant", "--tenant").is_err());
    }
}
