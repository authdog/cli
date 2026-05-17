//! Interactive org → tenant browser (used by `/browse`).

use anyhow::{Context, Result};
use authdog_cli::organizations;
use authdog_cli::projects;
use authdog_cli::session_store;
use authdog_cli::tenants::{self, TenantRow};

#[derive(Clone, Debug)]
pub enum BrowseStep {
    PickOrganization,
    PickTenant {
        /// Row label derived from `/v1/organizations`.
        org_summary: String,
        tenants: Vec<TenantRow>,
    },
}

/// Server-backed navigation state until `/browse` settles on projects for a tenant.
#[derive(Clone, Debug)]
pub struct BrowseSession {
    pub access_token: String,
    pub credentials_note: Option<String>,
    pub organizations: Vec<organizations::OrgRow>,
    pub step: BrowseStep,
}

fn org_heading(row: &organizations::OrgRow) -> String {
    let primary = row.display_primary();
    if primary.as_str() == row.id.as_str() {
        return primary;
    }
    format!("{}   {}", primary, row.id)
}

impl BrowseSession {
    /// Loads organizations from the API. If empty, skips to tenants for the signed-in principal.
    pub fn begin(access_token: String, credentials_note: Option<String>) -> Result<Self> {
        let org_value = organizations::fetch_organizations(&access_token)?;
        let org_rows = organizations::organization_rows_from_body(&org_value);
        let step = if org_rows.is_empty() {
            let ten_value = tenants::fetch_tenants(&access_token)
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
        })
    }

    /// After highlighting an organization row (`org_index`). Pulls tenants and applies org linkage heuristics.
    pub fn activate_organization(&mut self, org_index: usize) -> Result<Option<String>> {
        let row = self
            .organizations
            .get(org_index)
            .context("organization index")?;
        let headline = org_heading(row);
        let org_id = row.id.clone();

        let ten_value = tenants::fetch_tenants(&self.access_token).context("GET /v1/tenants")?;
        let all = tenants::tenant_rows_from_body(&ten_value);

        let (filtered, advisory) = tenants::filter_tenants_for_organization(&all, org_id.as_str());

        self.step = BrowseStep::PickTenant {
            org_summary: headline,
            tenants: filtered,
        };
        Ok(advisory)
    }

    pub fn activate_tenant_choice(&mut self, tenant_index: usize) -> Result<String> {
        let BrowseStep::PickTenant { tenants, .. } = &self.step else {
            anyhow::bail!("not at tenant picker");
        };
        let tenant = tenants.get(tenant_index).context("tenant index")?;

        session_store::set_current_tenant_id(Some(tenant.id.clone()))?;

        Ok(projects::compose_projects_report(
            &self.access_token,
            tenant.id.as_str(),
            self.credentials_note.clone(),
        ))
    }

    /// Esc returns to organizations whenever that step existed.
    pub fn pop_to_organizations(&mut self) -> bool {
        match self.step {
            BrowseStep::PickTenant { .. } if !self.organizations.is_empty() => {
                self.step = BrowseStep::PickOrganization;
                true
            }
            _ => false,
        }
    }
}
