//! Shared library pieces for [`authdog-cli`] (Ratatui binary uses the same modules).

pub mod whoami;

#[cfg(feature = "desktop")]
pub mod cli_login;
#[cfg(feature = "desktop")]
pub mod session_store;
#[cfg(feature = "desktop")]
pub mod tenants;
#[cfg(feature = "desktop")]
pub mod organizations;
