//! Slash-command metadata and autocomplete palette.

pub struct SlashCmd {
    pub name: &'static str,
    pub desc: &'static str,
}

pub const CMDS: &[SlashCmd] = &[
    SlashCmd {
        name: "help",
        desc: "Show available commands",
    },
    SlashCmd {
        name: "login",
        desc: "Sign in to Authdog",
    },
    SlashCmd {
        name: "logout",
        desc: "Delete saved credentials locally",
    },
    SlashCmd {
        name: "whoami",
        desc: "Identity from api.authdog.com (/v1/userinfo)",
    },
    SlashCmd {
        name: "tenants",
        desc: "Interactive tenant list (↑↓ Enter) · api /v1/tenants",
    },
    SlashCmd {
        name: "tenant",
        desc: "Show/set/clear current tenant id (for /projects)",
    },
    SlashCmd {
        name: "projects",
        desc: "Interactive projects list (↑↓ Enter) · /v1/tenants/{tenant}/projects",
    },
    SlashCmd {
        name: "browse",
        desc: "Pick org · tenant · project · env (↑↓ Enter Esc)",
    },
    SlashCmd {
        name: "organizations",
        desc: "Pick org (↑↓ Enter) · persists org (clears stale tenant/project/env) · /orgs",
    },
    SlashCmd {
        name: "status",
        desc: "Session · tokens preview · organization · tenant · project · env",
    },
    SlashCmd {
        name: "quit",
        desc: "Exit the CLI",
    },
];

pub fn slash_palette_indices(value: &str) -> Option<Vec<usize>> {
    let t = value.trim_start();
    if !t.starts_with('/') {
        return None;
    }
    let rest = &t[1..];
    if rest.contains(' ') || rest.contains('\t') || rest.contains('\n') {
        return None;
    }
    let q = rest.to_ascii_lowercase();
    let mut out = Vec::new();
    if q.is_empty() {
        out.extend(0..CMDS.len());
    } else {
        for (i, c) in CMDS.iter().enumerate() {
            if c.name.starts_with(q.as_str()) {
                out.push(i);
            }
        }
    }
    Some(out)
}
