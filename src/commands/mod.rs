//! Slash commands (`/help`, `/login`, …).

pub mod dispatch;
pub mod registry;

pub use dispatch::{apply_submit, SubmitEffect};
pub use registry::{slash_palette_indices, CMDS};
