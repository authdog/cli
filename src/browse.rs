//! Interactive org → tenant → project → environment (`/browse`).

use anyhow::{Context, Result};
use authdog_cli::organizations;
use authdog_cli::projects::{
    self, compose_selected_environment_report, environment_rows_from_body,
    fetch_application_environments, project_rows_from_body, EnvironmentRow, ProjectRow,
};
use authdog_cli::session_store;
use authdog_cli::tenants::{self, TenantRow};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrowsePopOutcome {
    SteppedBack,
    /// Left `/browse` entirely (still signed in).
    ExitedBrowse,
}

#[derive(Clone, Debug)]
#[allow(clippy::enum_variant_names)] // intentional Pick* wizard steps
pub enum BrowseStep {
    PickOrganization,
    PickTenant {
        org_summary: String,
        tenants: Vec<TenantRow>,
    },
    PickProject {
        tenant_summary: String,
        tenant_id: String,
        projects: Vec<ProjectRow>,
    },
    PickEnvironment {
        tenant_summary: String,
        tenant_id: String,
        application_summary: String,
        application_id: String,
        environments: Vec<EnvironmentRow>,
        /// Full **`GET …/environments`** body (used to snapshot the picked row verbatim).
        env_response: Value,
    },
}

/// Server-backed navigation state for **`/browse`**.
#[derive(Clone, Debug)]
pub struct BrowseSession {
    pub access_token: String,
    pub credentials_note: Option<String>,
    pub organizations: Vec<organizations::OrgRow>,
    pub step: BrowseStep,
    /// Restores **[`BrowseStep::PickTenant`]** after **[`BrowseStep::PickProject`]** back-nav.
    tenant_pick_snapshot: Option<(String, Vec<TenantRow>)>,
    /// Restores **[`BrowseStep::PickProject`]** after **[`BrowseStep::PickEnvironment`]** back-nav.
    project_pick_snapshot: Option<(String, String, Vec<ProjectRow>)>,
}

fn org_heading(row: &organizations::OrgRow) -> String {
    let primary = row.display_primary();
    if primary.as_str() == row.id.as_str() {
        return primary;
    }
    format!("{}   {}", primary, row.id)
}

fn tenant_heading(t: &TenantRow) -> String {
    if let Some(ref n) = t.name {
        let nt = n.trim();
        if !nt.is_empty() && nt != t.id {
            return format!("{nt}   {}", t.id);
        }
    }
    t.id.clone()
}

fn project_heading(p: &ProjectRow) -> String {
    let prim = p.display_primary();
    if prim.as_str() == p.id.as_str() {
        return prim;
    }
    format!("{prim}   {}", p.id)
}

fn extract_environment_object(response: &Value, index: usize) -> Result<Value> {
    let arr = response
        .get("environments")
        .and_then(|v| v.as_array())
        .context("environments array missing in API response")?;
    arr.get(index)
        .cloned()
        .context("environment index out of range")
}

impl BrowseSession {
    /// Loads organizations from the API. If empty, skips to tenants for the signed-in principal.
    pub fn begin(access_token: String, credentials_note: Option<String>) -> Result<Self> {
        let org_value = organizations::fetch_organizations(&access_token)?;
        let org_rows = organizations::organization_rows_from_body(&org_value);
        let step = if org_rows.is_empty() {
            session_store::set_current_organization_id(None)?;
            let ten_value = tenants::fetch_tenants(&access_token, None)
                .context("GET /v1/tenants (no organizations)")?;
            let tenant_rows = tenants::tenant_rows_from_body(&ten_value);
            BrowseStep::PickTenant {
                org_summary: "All tenants (no organizations)".into(),
                tenants: tenant_rows,
            }
        } else {
            BrowseStep::PickOrganization
        };
        Ok(Self {
            access_token,
            credentials_note,
            organizations: org_rows,
            step,
            tenant_pick_snapshot: None,
            project_pick_snapshot: None,
        })
    }

    /// After highlighting an organization row (`org_index`). Pulls tenants and applies org linkage heuristics.
    pub fn activate_organization(&mut self, org_index: usize) -> Result<Option<String>> {
        self.tenant_pick_snapshot = None;
        self.project_pick_snapshot = None;
        let row = self
            .organizations
            .get(org_index)
            .context("organization index")?;
        let headline = org_heading(row);
        let org_id = row.id.clone();
        session_store::set_current_organization_id(Some(org_id.clone()))?;

        let ten_value = tenants::fetch_tenants(&self.access_token, Some(org_id.as_str()))
            .context("GET /v1/tenants")?;
        let all = tenants::tenant_rows_from_body(&ten_value);

        let (filtered, advisory) =
            tenants::filter_tenants_for_organization_for_browse(&all, org_id.as_str());
        if filtered.is_empty() {
            anyhow::bail!(
                "{}",
                advisory.unwrap_or_else(|| "No tenants for this organization.".into())
            );
        }

        self.step = BrowseStep::PickTenant {
            org_summary: headline,
            tenants: filtered,
        };
        Ok(None)
    }

    /// Confirm tenant → list projects for that tenant.
    pub fn advance_from_tenant(&mut self, tenant_index: usize) -> Result<()> {
        let BrowseStep::PickTenant {
            org_summary,
            tenants,
        } = &self.step
        else {
            anyhow::bail!("not at tenant picker");
        };
        let tenant = tenants.get(tenant_index).context("tenant index")?;

        session_store::set_current_tenant_id(Some(tenant.id.clone()))?;

        let raw = projects::fetch_projects(&self.access_token, tenant.id.as_str())
            .context("GET /v1/tenants/{id}/projects")?;
        let prows = project_rows_from_body(&raw);
        if prows.is_empty() {
            anyhow::bail!("no projects returned for this tenant");
        }

        self.tenant_pick_snapshot = Some((org_summary.clone(), tenants.clone()));
        self.project_pick_snapshot = None;

        self.step = BrowseStep::PickProject {
            tenant_summary: tenant_heading(tenant),
            tenant_id: tenant.id.clone(),
            projects: prows,
        };
        Ok(())
    }

    /// Confirm project → list environments (**`GET …/applications/{applicationId}/environments`**).
    pub fn advance_from_project(&mut self, project_index: usize) -> Result<()> {
        let BrowseStep::PickProject {
            tenant_summary,
            tenant_id,
            projects,
        } = &self.step
        else {
            anyhow::bail!("not at project picker");
        };
        let proj = projects.get(project_index).context("project index")?;

        let raw = fetch_application_environments(
            &self.access_token,
            tenant_id.as_str(),
            proj.id.as_str(),
        )
        .context("GET application environments")?;

        let envs = environment_rows_from_body(&raw);
        if envs.is_empty() {
            anyhow::bail!(
                "no environments returned for this project (still created a tenant selection on disk)",
            );
        }

        self.project_pick_snapshot =
            Some((tenant_summary.clone(), tenant_id.clone(), projects.clone()));

        self.step = BrowseStep::PickEnvironment {
            tenant_summary: tenant_summary.clone(),
            tenant_id: tenant_id.clone(),
            application_summary: project_heading(proj),
            application_id: proj.id.clone(),
            environments: envs,
            env_response: raw,
        };
        Ok(())
    }

    /// Confirm environment → persist application + env ids and return session text.
    pub fn finalize_environment(&mut self, environment_index: usize) -> Result<String> {
        let BrowseStep::PickEnvironment {
            tenant_id,
            application_id,
            env_response,
            environments,
            tenant_summary,
            application_summary,
            ..
        } = &self.step
        else {
            anyhow::bail!("not at environment picker");
        };
        let row = environments
            .get(environment_index)
            .context("environment index")?;
        let detail = extract_environment_object(env_response, environment_index)?;

        session_store::set_current_application_id(Some(application_id.clone()))?;
        session_store::set_current_environment_id(Some(row.id.clone()))?;

        let banner = format!(
            "Selections stored in credentials.json:\n  Tenant: {tenant_summary}\n  Project: {application_summary}\n  Environment: {}\n\n",
            row.display_primary(),
        );

        let body = compose_selected_environment_report(
            &self.access_token,
            tenant_id.as_str(),
            application_id.as_str(),
            row.id.as_str(),
            detail,
            self.credentials_note.clone(),
        );

        Ok(format!("{banner}{body}"))
    }

    pub fn pop_navigation(&mut self) -> BrowsePopOutcome {
        match &self.step {
            BrowseStep::PickEnvironment { .. } => match self.project_pick_snapshot.take() {
                Some((tenant_summary, tenant_id, projects)) => {
                    self.step = BrowseStep::PickProject {
                        tenant_summary,
                        tenant_id,
                        projects,
                    };
                    BrowsePopOutcome::SteppedBack
                }
                None => BrowsePopOutcome::ExitedBrowse,
            },
            BrowseStep::PickProject { .. } => match self.tenant_pick_snapshot.take() {
                Some((org_summary, tenants)) => {
                    self.project_pick_snapshot = None;
                    self.step = BrowseStep::PickTenant {
                        org_summary,
                        tenants,
                    };
                    BrowsePopOutcome::SteppedBack
                }
                None => BrowsePopOutcome::ExitedBrowse,
            },
            BrowseStep::PickTenant { .. } => {
                if self.organizations.is_empty() {
                    BrowsePopOutcome::ExitedBrowse
                } else {
                    self.tenant_pick_snapshot = None;
                    self.project_pick_snapshot = None;
                    self.step = BrowseStep::PickOrganization;
                    BrowsePopOutcome::SteppedBack
                }
            }
            BrowseStep::PickOrganization => BrowsePopOutcome::ExitedBrowse,
        }
    }
}
