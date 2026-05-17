//! Full-screen Ratatui shell: layout, input, and session output pane.

use crate::browse::{BrowsePopOutcome, BrowseSession, BrowseStep};
use crate::commands::{apply_submit, slash_palette_indices, SubmitEffect, CMDS};
use crate::tui_output;

use authdog_cli::cli_login;
use authdog_cli::organizations::OrgRow;
use authdog_cli::projects::{EnvironmentRow, ProjectRow};
use authdog_cli::tenants::TenantRow;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use figlet_rs::FIGlet;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::Stylize;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::DefaultTerminal;
use std::cmp;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;
use unicode_width::UnicodeWidthStr;

const BG: Color = Color::Rgb(43, 21, 40);
const SURFACE: Color = Color::Rgb(54, 36, 50);
const SURFACE_HI: Color = Color::Rgb(69, 52, 64);
const BORDER: Color = Color::Rgb(92, 69, 88);
const TXT: Color = Color::Rgb(232, 219, 244);
const TXT_DIM: Color = Color::Rgb(156, 147, 168);
const ACCENT: Color = Color::Rgb(218, 184, 234);
const SEL_BG: Color = Color::Rgb(232, 220, 232);
const SEL_FG: Color = Color::Rgb(35, 20, 40);
const STATUS_OK: Color = Color::Rgb(212, 189, 230);
const STATUS_ERR: Color = Color::Rgb(240, 188, 212);
const STATUS_SUCCESS: Color = Color::Rgb(146, 220, 174);

/// Bundled ANSI Shadow FIGlet font (`patorjk/figlet.js` via jsDelivr).
const ANSI_SHADOW_FLF: &str = include_str!("../assets/fonts/ANSI Shadow.flf");
/// Lowercase renders correctly for this alphabet; avoids tall small-caps glyphs.
const BANNER_FIGLET: &str = "authdog";

const HEADER_TAIL_LINES: u16 = 6;

/// Prefix before the input buffer (fixed; cursor sits in the value area after this).
const INPUT_PREFIX: &str = "→ ";

/// Hide the ✔ Signed in banner after this delay (header layout shrinks back).
const LOGIN_SUCCESS_STATUS_TTL: Duration = Duration::from_secs(4);

/// Keyboard-driven list for **`/tenants`**, **`/organizations`**, or **`/projects`** (instead of raw JSON dumps).
#[derive(Clone)]
pub(crate) enum ListingPicker {
    Tenants {
        rows: Vec<TenantRow>,
        endpoint: String,
        credentials_note: Option<String>,
    },
    Organizations {
        rows: Vec<OrgRow>,
        endpoint: String,
        credentials_note: Option<String>,
    },
    Projects {
        rows: Vec<ProjectRow>,
        endpoint: String,
        credentials_note: Option<String>,
    },
}

impl ListingPicker {
    fn trimmed_credentials_note(note: Option<&String>) -> Option<&str> {
        note.and_then(|n| {
            let t = n.trim();
            (!t.is_empty()).then_some(t)
        })
    }

    fn pick_one_title_tail(endpoint: &str, row_count: usize, note: Option<&String>) -> String {
        let mut s = format!(" ({endpoint}) · {row_count} rows · pick one");
        if let Some(t) = Self::trimmed_credentials_note(note) {
            s.push_str(" · ");
            s.push_str(&truncate_vis(t, 44));
        }
        s
    }

    fn row_count(&self) -> usize {
        match self {
            ListingPicker::Tenants { rows, .. } => rows.len(),
            ListingPicker::Organizations { rows, .. } => rows.len(),
            ListingPicker::Projects { rows, .. } => rows.len(),
        }
    }

    fn block_title(&self) -> Line<'_> {
        match self {
            ListingPicker::Tenants {
                endpoint,
                rows,
                credentials_note,
            } => Line::from(vec![
                Span::styled("Tenants", Style::default().fg(TXT).bold()),
                Span::styled(
                    Self::pick_one_title_tail(endpoint, rows.len(), credentials_note.as_ref()),
                    TXT_DIM,
                ),
            ]),
            ListingPicker::Organizations {
                endpoint,
                rows,
                credentials_note,
            } => Line::from(vec![
                Span::styled("Organizations", Style::default().fg(TXT).bold()),
                Span::styled(
                    Self::pick_one_title_tail(endpoint, rows.len(), credentials_note.as_ref()),
                    TXT_DIM,
                ),
            ]),
            ListingPicker::Projects {
                endpoint,
                rows,
                credentials_note,
            } => Line::from(vec![
                Span::styled("Projects", Style::default().fg(TXT).bold()),
                Span::styled(
                    Self::pick_one_title_tail(endpoint, rows.len(), credentials_note.as_ref()),
                    TXT_DIM,
                ),
            ]),
        }
    }

    fn append_credentials_footer(text: &mut String, note: Option<&String>) {
        if let Some(t) = Self::trimmed_credentials_note(note) {
            text.push_str("\n\n");
            text.push_str(t);
        }
    }
}

/// Pretty (**tabular**) vs indented JSON (**Raw** tab) for **`/whoami`**.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum WhoamiJsonTab {
    #[default]
    Pretty,
    Raw,
}

#[derive(Clone)]
pub(crate) struct WhoamiOutputPane {
    pub(crate) endpoint_note: String,
    /// Tabular (**Pretty**) REST envelope text (**`/whoami`** only).
    pub(crate) pretty_json: String,
    /// Indented JSON object matching the server payload (**Raw** tab).
    pub(crate) raw_json: String,
    pub(crate) credentials_note: Option<String>,
    pub(crate) tab: WhoamiJsonTab,
}

impl WhoamiOutputPane {
    pub(crate) fn composed_for_styling(&self) -> String {
        let json = match self.tab {
            WhoamiJsonTab::Pretty => self.pretty_json.as_str(),
            WhoamiJsonTab::Raw => self.raw_json.as_str(),
        };
        let mut s = format!("── Identity ({}) ──\n{json}", self.endpoint_note);
        if let Some(ref note) = self.credentials_note {
            let t = note.trim();
            if !t.is_empty() {
                s.push_str("\n\n");
                s.push_str(note);
            }
        }
        s
    }

    pub(crate) fn tab_line(&self) -> Line<'static> {
        let pretty_hot = self.tab == WhoamiJsonTab::Pretty;
        Line::from(vec![
            Span::styled(
                " Pretty ",
                if pretty_hot {
                    Style::default().fg(SEL_FG).bg(ACCENT).bold()
                } else {
                    Style::default().fg(TXT_DIM).italic()
                },
            ),
            Span::styled(" · ", BORDER),
            Span::styled(
                " Raw ",
                if !pretty_hot {
                    Style::default().fg(SEL_FG).bg(ACCENT).bold()
                } else {
                    Style::default().fg(TXT_DIM).italic()
                },
            ),
        ])
    }
}

fn status_fingerprint(opt: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    match opt {
        None => 0u8.hash(&mut h),
        Some(t) => {
            1u8.hash(&mut h);
            t.hash(&mut h);
        }
    }
    h.finish()
}

#[derive(Default)]
pub struct App {
    pub(crate) quit: bool,
    pub(crate) input: Input,
    pub(crate) list_state: ListState,
    /// `/browse` pickers reuse this list (orgs → tenants → projects → environments).
    pub(crate) browse_list_state: ListState,
    pub(crate) browse: Option<BrowseSession>,
    pub(crate) status: Option<String>,
    pub(crate) status_err: bool,
    /// When set, clear `status` once `Instant::now()` passes (transient toasts).
    pub(crate) status_clear_at: Option<Instant>,
    /// Hash used to detect session-output text changes; bumps reset vertical scroll position.
    pub(crate) status_scroll_digest: u64,
    /// Vertical scroll rows for [`Paragraph::scroll`] in the session output pane.
    pub(crate) status_scroll_row: u16,
    pub(crate) last_status_viewport_h: u16,
    pub(crate) last_status_scroll_row_max: u16,
    /// Last-drawn rectangle for `/projects`-style session output (`None` until drawn).
    pub(crate) session_output_rect: Option<Rect>,
    /// `/tenants`, `/organizations`, or **`/projects`** interactive table (exclusive with [`Self::browse`]).
    pub(crate) listing_picker: Option<ListingPicker>,
    pub(crate) listing_list_state: ListState,
    /// **`/whoami`** JSON pane (exclusive with meaningful [`Self::status`] content for layout).
    pub(crate) whoami_output: Option<WhoamiOutputPane>,
}

impl App {
    fn tick_status_autoclose(&mut self) {
        if let Some(deadline) = self.status_clear_at {
            if Instant::now() >= deadline {
                self.status = None;
                self.status_err = false;
                self.status_clear_at = None;
            }
        }
    }

    pub(crate) fn clear_listing_picker(&mut self) {
        self.listing_picker = None;
        self.listing_list_state.select(None);
    }

    fn sync_listing_picker_selection(&mut self) {
        let len = self
            .listing_picker
            .as_ref()
            .map(ListingPicker::row_count)
            .unwrap_or(0);
        match self.listing_list_state.selected() {
            Some(s) if s < len => {}
            _ if len > 0 => self.listing_list_state.select(Some(0)),
            _ => self.listing_list_state.select(None),
        }
    }
    fn browse_exclusive(&self, palette: &Option<Vec<usize>>) -> bool {
        self.browse.is_some() && palette.as_ref().is_none_or(|cmds| cmds.is_empty())
    }

    fn browse_list_len(&self) -> usize {
        self.browse
            .as_ref()
            .map(|b| match &b.step {
                BrowseStep::PickOrganization => b.organizations.len(),
                BrowseStep::PickTenant { tenants, .. } => tenants.len(),
                BrowseStep::PickProject { projects, .. } => projects.len(),
                BrowseStep::PickEnvironment { environments, .. } => environments.len(),
            })
            .unwrap_or(0)
    }

    fn sync_browse_list_selection(&mut self) {
        let len = self.browse_list_len();
        match self.browse_list_state.selected() {
            Some(s) if s < len => {}
            _ if len > 0 => self.browse_list_state.select(Some(0)),
            _ => self.browse_list_state.select(None),
        }
    }

    fn handle_browse_enter(&mut self) -> Result<()> {
        let sel = self.browse_list_state.selected().unwrap_or(0);
        let Some(mut session) = self.browse.take() else {
            return Ok(());
        };

        if matches!(session.step, BrowseStep::PickOrganization) {
            match session.activate_organization(sel) {
                Ok(advisory) => {
                    self.browse = Some(session);
                    self.browse_list_state.select(Some(0));
                    self.sync_browse_list_selection();
                    if let Some(msg) = advisory {
                        self.status = Some(msg);
                        self.status_err = false;
                        self.status_clear_at = None;
                    }
                }
                Err(e) => {
                    self.browse = Some(session);
                    self.status = Some(format!("{e:#}"));
                    self.status_err = true;
                    self.status_clear_at = None;
                }
            }
        } else if matches!(session.step, BrowseStep::PickTenant { .. }) {
            match session.advance_from_tenant(sel) {
                Ok(()) => {
                    self.browse = Some(session);
                    self.status = None;
                    self.status_err = false;
                    self.browse_list_state.select(Some(0));
                    self.sync_browse_list_selection();
                }
                Err(e) => {
                    self.browse = Some(session);
                    self.status = Some(format!("{e:#}"));
                    self.status_err = true;
                }
            }
        } else if matches!(session.step, BrowseStep::PickProject { .. }) {
            match session.advance_from_project(sel) {
                Ok(()) => {
                    self.browse = Some(session);
                    self.status = None;
                    self.status_err = false;
                    self.browse_list_state.select(Some(0));
                    self.sync_browse_list_selection();
                }
                Err(e) => {
                    self.browse = Some(session);
                    self.status = Some(format!("{e:#}"));
                    self.status_err = true;
                }
            }
        } else if matches!(session.step, BrowseStep::PickEnvironment { .. }) {
            match session.finalize_environment(sel) {
                Ok(text) => {
                    self.status = Some(text);
                    self.status_err = false;
                    self.status_clear_at = None;
                    self.browse_list_state.select(None);
                }
                Err(e) => {
                    self.browse = Some(session);
                    self.status = Some(format!("{e:#}"));
                    self.status_err = true;
                }
            }
        }
        Ok(())
    }

    fn handle_listing_enter(&mut self) -> Result<()> {
        use authdog_cli::session_store;

        let picker = match self.listing_picker.take() {
            Some(p) => p,
            None => return Ok(()),
        };
        let sel = self.listing_list_state.selected().unwrap_or(0);
        self.listing_list_state.select(None);

        match picker {
            ListingPicker::Tenants {
                rows,
                endpoint,
                credentials_note,
            } => {
                if rows.is_empty() {
                    let mut msg = format!("Tenants ({endpoint})\n(No rows.)");
                    ListingPicker::append_credentials_footer(&mut msg, credentials_note.as_ref());
                    self.status = Some(msg);
                    self.status_err = false;
                    return Ok(());
                }
                let Some(row) = rows.get(sel) else {
                    self.status = Some(format!(
                        "Tenants ({endpoint})\n(Invalid selection; run /tenants.)"
                    ));
                    self.status_err = true;
                    return Ok(());
                };
                let primary = tenant_row_primary(row);
                match session_store::set_current_tenant_id(Some(row.id.clone())) {
                    Ok(()) => {
                        let mut ok = format!("Current tenant set to `{}`\n{}", row.id, primary);
                        ListingPicker::append_credentials_footer(
                            &mut ok,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(ok);
                        self.status_err = false;
                    }
                    Err(e) => {
                        let mut err = format!("Could not save tenant:\n{e:#}");
                        ListingPicker::append_credentials_footer(
                            &mut err,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(err);
                        self.status_err = true;
                    }
                }
            }
            ListingPicker::Organizations {
                rows,
                endpoint,
                credentials_note,
            } => {
                if rows.is_empty() {
                    let mut msg = format!("Organizations ({endpoint})\n(No rows.)");
                    ListingPicker::append_credentials_footer(&mut msg, credentials_note.as_ref());
                    self.status = Some(msg);
                    self.status_err = false;
                    return Ok(());
                }
                let Some(row) = rows.get(sel) else {
                    self.status = Some(format!(
                        "Organizations ({endpoint})\n(Invalid selection; run /organizations.)"
                    ));
                    self.status_err = true;
                    return Ok(());
                };
                let prim = row.display_primary();
                match session_store::set_current_organization_id(Some(row.id.clone())) {
                    Ok(()) => {
                        let mut ok = format!("Current organization set to `{}`\n{}", row.id, prim);
                        ListingPicker::append_credentials_footer(
                            &mut ok,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(ok);
                        self.status_err = false;
                    }
                    Err(e) => {
                        let mut err = format!("Could not save organization:\n{e:#}");
                        ListingPicker::append_credentials_footer(
                            &mut err,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(err);
                        self.status_err = true;
                    }
                }
            }
            ListingPicker::Projects {
                rows,
                endpoint,
                credentials_note,
            } => {
                if rows.is_empty() {
                    let mut msg = format!("Projects ({endpoint})\n(No rows.)");
                    ListingPicker::append_credentials_footer(&mut msg, credentials_note.as_ref());
                    self.status = Some(msg);
                    self.status_err = false;
                    return Ok(());
                }
                let Some(row) = rows.get(sel) else {
                    self.status = Some(format!(
                        "Projects ({endpoint})\n(Invalid selection; run /projects.)"
                    ));
                    self.status_err = true;
                    return Ok(());
                };
                let primary = row.display_primary();
                match session_store::set_current_application_id(Some(row.id.clone())) {
                    Ok(()) => {
                        let mut ok = format!("Current project set to `{}`\n{}", row.id, primary);
                        ListingPicker::append_credentials_footer(
                            &mut ok,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(ok);
                        self.status_err = false;
                    }
                    Err(e) => {
                        let mut err = format!("Could not save project:\n{e:#}");
                        ListingPicker::append_credentials_footer(
                            &mut err,
                            credentials_note.as_ref(),
                        );
                        self.status = Some(err);
                        self.status_err = true;
                    }
                }
            }
        }
        Ok(())
    }

    fn draw_listing_picker_panel(
        &mut self,
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        picker: &ListingPicker,
    ) {
        self.sync_listing_picker_selection();
        let vw = usize::from(area.width.max(3).saturating_sub(4));
        let items: Vec<ListItem> = match picker {
            ListingPicker::Tenants { rows, .. } => rows
                .iter()
                .map(|t| ListItem::new(browse_tenant_line(t, vw)))
                .collect(),
            ListingPicker::Organizations { rows, .. } => rows
                .iter()
                .map(|o| ListItem::new(browse_org_line(o, vw)))
                .collect(),
            ListingPicker::Projects { rows, .. } => rows
                .iter()
                .map(|p| ListItem::new(browse_project_line(p, vw)))
                .collect(),
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(BORDER)
                    .title(picker.block_title())
                    .style(Style::default().bg(SURFACE_HI)),
            )
            .highlight_style(Style::default().bg(SEL_BG).fg(SEL_FG).bold())
            .highlight_symbol("› ");

        f.render_stateful_widget(list, area, &mut self.listing_list_state);
        self.last_status_viewport_h = area.height.saturating_sub(2).max(1);
        self.last_status_scroll_row_max = 0;
    }

    pub fn run(mut self, term: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.tick_status_autoclose();
            term.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(250))? {
                self.on_event(event::read()?, term)?;
            }
        }
        Ok(())
    }

    fn draw(&mut self, f: &mut ratatui::Frame<'_>) {
        let area = f.area();
        self.session_output_rect = None;
        f.render_widget(Clear, area);

        let base = Block::default().style(Style::default().bg(BG));
        f.render_widget(base, area);

        let outer = area.inner(Margin::new(2, 1));
        if outer.height < 8 || outer.width < 28 {
            f.render_widget(
                Paragraph::new("Terminal too small (need ≥28×8)")
                    .centered()
                    .style(TXT_DIM),
                outer,
            );
            return;
        }

        let palette = slash_palette_indices(self.input.value());
        let browse_open = self.browse.is_some();
        let listing_open = self.listing_picker.is_some();

        let (banner_lines, banner_h) = figlet_banner_lines(outer.width);

        let header_need = banner_h + HEADER_TAIL_LINES;

        let list_h = palette
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(|v| ((v.len() + 3) as u16).min(outer.height.saturating_sub(header_need + 4)))
            .unwrap_or(0);

        match &palette {
            Some(idxs) if !idxs.is_empty() => self.sync_list_selection(idxs.len()),
            _ => self.list_state.select(None),
        }

        let [header_chunk, list_chunk, spacer, input_chunk, foot_chunk] = Layout::vertical([
            Constraint::Length(header_need),
            Constraint::Length(list_h),
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(outer);

        self.draw_header(f, header_chunk, banner_lines);
        if let Some(ref idxs) = palette {
            if !idxs.is_empty() {
                self.draw_menu(f, list_chunk, idxs);
            }
        }

        self.draw_session_output(f, spacer);

        self.draw_input_and_cursor(f, input_chunk, browse_open, listing_open);

        let hint_line = if browse_open {
            Line::from(vec![
                "Browse ".into(),
                "↑↓".dim(),
                " pick · ".into(),
                "Enter".bold(),
                " open · ".into(),
                "Esc".bold(),
                " back / exit browse · Ctrl+C quit".dim(),
            ])
        } else if listing_open {
            Line::from(vec![
                "Listing ".into(),
                "↑↓".dim(),
                " pick · ".into(),
                "Enter".bold(),
                " choose · ".into(),
                "Esc".bold(),
                " close · Ctrl+C quit".dim(),
            ])
        } else {
            let mut hint = vec![
                "↑↓".dim(),
                " choose · Tab · ".into(),
                "Enter".bold(),
                " run · Esc".into(),
                " leave · Ctrl+C quit".dim(),
            ];
            if self.status.is_some() || self.whoami_output.is_some() {
                hint.push(Span::raw(" · "));
                hint.push(Span::styled(
                    "PgUp/PgDn",
                    Style::default().fg(TXT).add_modifier(Modifier::BOLD),
                ));
                hint.push(Span::raw("/"));
                hint.push(Span::styled(
                    "Home/End",
                    Style::default().fg(TXT).add_modifier(Modifier::BOLD),
                ));
                hint.push(Span::styled(" · ", Style::default().fg(TXT_DIM)));
                hint.push(Span::styled("Wheel", Style::default().fg(TXT).bold()));
                hint.push(Span::styled(" output", Style::default().fg(TXT_DIM)));
                if self.whoami_output.is_some() {
                    hint.push(Span::styled(
                        " · Pretty/Raw: ",
                        Style::default().fg(TXT_DIM),
                    ));
                    hint.push(Span::styled(
                        "Tab",
                        Style::default().fg(TXT).add_modifier(Modifier::BOLD),
                    ));
                }
            }
            Line::from(hint)
        };

        f.render_widget(
            Paragraph::new(hint_line)
                .style(Style::default().fg(TXT_DIM))
                .bg(BG),
            foot_chunk,
        );
    }

    fn sync_list_selection(&mut self, len: usize) {
        match self.list_state.selected() {
            Some(s) if s < len => {}
            _ if len > 0 => self.list_state.select(Some(0)),
            _ => self.list_state.select(None),
        }
    }

    fn draw_header(
        &self,
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        banner_lines: Vec<Line<'static>>,
    ) {
        fn cli_tagline() -> String {
            format!(
                "CLI . Version ({}) . ({}) . {}",
                env!("CARGO_PKG_VERSION"),
                option_env!("AUTHDOG_CLI_MONTH_YEAR").unwrap_or("unknown-month-year"),
                option_env!("AUTHDOG_CLI_GIT_SHA").unwrap_or("unknown-git-sha"),
            )
        }

        let mut lines = banner_lines;
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            cli_tagline(),
            Style::default().fg(TXT_DIM).italic(),
        )]));
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width.max(16) as usize),
            BORDER,
        )));
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            "Type /help for slash commands · Enter runs · Esc clears or exits",
            TXT_DIM,
        )]));

        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .block(Block::default().style(Style::default().bg(BG))),
            area,
        );
    }

    fn draw_session_output(&mut self, f: &mut ratatui::Frame<'_>, area: Rect) {
        if area.height < 3 || area.width < 14 {
            f.render_widget(Paragraph::new("").style(Style::default().bg(BG)), area);
            return;
        }

        if let Some(picker) = self.listing_picker.clone() {
            self.draw_listing_picker_panel(f, area, &picker);
            return;
        }

        if self.browse.is_some() {
            self.sync_browse_list_selection();
            let session = self
                .browse
                .as_ref()
                .cloned()
                .expect("browse state checked immediately above");
            self.draw_browse_panel(f, area, &session);
            self.last_status_viewport_h = area.height.saturating_sub(2).max(1);
            self.last_status_scroll_row_max = 0;
            return;
        }

        if let Some(pane) = self.whoami_output.clone() {
            self.draw_whoami_panel(f, area, &pane);
            return;
        }

        let Some(raw) = self.status.as_deref() else {
            f.render_widget(Paragraph::new("").style(Style::default().bg(BG)), area);
            self.last_status_viewport_h = 0;
            self.last_status_scroll_row_max = 0;
            return;
        };

        let fp = status_fingerprint(Some(raw));
        if fp != self.status_scroll_digest {
            self.status_scroll_digest = fp;
            self.status_scroll_row = 0;
        }

        let palette_out = tui_output::OutputPalette {
            fg: TXT,
            muted: TXT_DIM,
            sep: BORDER,
            accent: ACCENT,
            success: STATUS_SUCCESS,
            ok: STATUS_OK,
            err: STATUS_ERR,
        };

        let lines: Vec<Line<'static>> = if self.status_err {
            raw.lines()
                .map(|l| Line::from(Span::styled(l.to_owned(), Style::default().fg(STATUS_ERR))))
                .collect()
        } else {
            tui_output::styled_status_lines(raw, palette_out, false)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(BORDER)
            .title(Line::from(vec![
                Span::styled("Session output", Style::default().fg(TXT).bold()),
                Span::styled(
                    " · PgUp/PgDn · Wheel · Home/End",
                    Style::default().fg(TXT_DIM),
                ),
            ]))
            .style(Style::default().bg(SURFACE));

        let inner_area = block.inner(area);
        let inner_w = inner_area.width.max(1);
        let vh_usize = usize::from(inner_area.height.max(1));

        let total_rows = tui_output::wrapped_row_count(&lines, inner_w);
        let vmax = total_rows.saturating_sub(vh_usize);
        let vmax_u16 = u16::try_from(vmax).unwrap_or(u16::MAX);

        self.status_scroll_row = self.status_scroll_row.min(vmax_u16);
        self.last_status_viewport_h = inner_area.height.max(1);
        self.last_status_scroll_row_max = vmax_u16;

        self.session_output_rect = Some(area);

        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(SURFACE))
            .alignment(Alignment::Left)
            .wrap(Wrap {
                trim: false, // preserve leading spaces (JSON indentation)
            })
            .scroll((self.status_scroll_row, 0))
            .block(block);

        f.render_widget(paragraph, area);
    }

    fn draw_whoami_panel(
        &mut self,
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        pane: &WhoamiOutputPane,
    ) {
        let composed = pane.composed_for_styling();
        let fp = status_fingerprint(Some(composed.as_str()));
        if fp != self.status_scroll_digest {
            self.status_scroll_digest = fp;
            self.status_scroll_row = 0;
        }

        let palette_out = tui_output::OutputPalette {
            fg: TXT,
            muted: TXT_DIM,
            sep: BORDER,
            accent: ACCENT,
            success: STATUS_SUCCESS,
            ok: STATUS_OK,
            err: STATUS_ERR,
        };

        let mut lines = tui_output::styled_status_lines(&composed, palette_out, false);
        lines.insert(0, pane.tab_line());
        lines.insert(1, Line::default());

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(BORDER)
            .title(Line::from(vec![
                Span::styled("Session output", Style::default().fg(TXT).bold()),
                Span::styled(
                    " · Pretty/Raw Tab · PgUp/PgDn · Wheel · Home/End",
                    Style::default().fg(TXT_DIM),
                ),
            ]))
            .style(Style::default().bg(SURFACE));

        let inner_area = block.inner(area);
        let inner_w = inner_area.width.max(1);
        let vh_usize = usize::from(inner_area.height.max(1));

        let total_rows = tui_output::wrapped_row_count(&lines, inner_w);
        let vmax = total_rows.saturating_sub(vh_usize);
        let vmax_u16 = u16::try_from(vmax).unwrap_or(u16::MAX);

        self.status_scroll_row = self.status_scroll_row.min(vmax_u16);
        self.last_status_viewport_h = inner_area.height.max(1);
        self.last_status_scroll_row_max = vmax_u16;

        self.session_output_rect = Some(area);

        let paragraph = Paragraph::new(lines)
            .style(Style::default().bg(SURFACE))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .scroll((self.status_scroll_row, 0))
            .block(block);

        f.render_widget(paragraph, area);
    }

    fn draw_browse_panel(
        &mut self,
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        session: &BrowseSession,
    ) {
        let inner = area;

        let show_note = matches!(&session.step, BrowseStep::PickTenant { .. })
            && self
                .status
                .as_ref()
                .is_some_and(|s| !self.status_err && !s.trim().is_empty());

        let list_area = if show_note && inner.height > 7 {
            let note_h = (inner.height / 5).clamp(3, inner.height / 3);
            let [note_chunk, rest] =
                Layout::vertical([Constraint::Length(note_h), Constraint::Fill(1)]).areas(inner);

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(BORDER)
                .title(Line::from(Span::styled(
                    "Note",
                    Style::default().fg(TXT_DIM).italic(),
                )))
                .style(Style::default().bg(SURFACE_HI));
            let inset = block.inner(note_chunk);

            let note_text = match &self.status {
                Some(s) if !self.status_err => s.trim(),
                _ => "",
            };

            let p = Paragraph::new(note_text)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(TXT_DIM).bg(SURFACE_HI));

            f.render_widget(block, note_chunk);
            f.render_widget(p, inset);

            rest
        } else {
            inner
        };

        let vw = usize::from(list_area.width.max(3).saturating_sub(4));

        let items: Vec<ListItem<'_>> = match &session.step {
            BrowseStep::PickOrganization => session
                .organizations
                .iter()
                .map(|o| ListItem::new(browse_org_line(o, vw)))
                .collect(),
            BrowseStep::PickTenant { tenants, .. } => tenants
                .iter()
                .map(|t| ListItem::new(browse_tenant_line(t, vw)))
                .collect(),
            BrowseStep::PickProject { projects, .. } => projects
                .iter()
                .map(|p| ListItem::new(browse_project_line(p, vw)))
                .collect(),
            BrowseStep::PickEnvironment { environments, .. } => environments
                .iter()
                .map(|e| ListItem::new(browse_env_line(e, vw)))
                .collect(),
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(BORDER)
            .title(browse_block_title(session))
            .style(Style::default().bg(SURFACE_HI));

        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().bg(SEL_BG).fg(SEL_FG).bold())
            .highlight_symbol("› ");

        f.render_stateful_widget(list, list_area, &mut self.browse_list_state);
    }

    fn draw_menu(&mut self, f: &mut ratatui::Frame<'_>, area: Rect, idxs: &[usize]) {
        let cmd_w = 14usize;
        let items: Vec<ListItem<'_>> = idxs
            .iter()
            .map(|&ci| {
                let c = &CMDS[ci];
                let label = format!("/{}", c.name);
                let padded = truncate_pad(&label, cmd_w);
                let space = cmp::max(2, area.width.saturating_sub((cmd_w as u16) + 8) as usize);
                let rest_w = area.width.saturating_sub(cmd_w as u16 + space as u16 + 6) as usize;
                let tail = truncate_vis(c.desc, rest_w.max(12));
                Line::from(vec![
                    Span::styled(padded, Style::default().fg(TXT).bold()),
                    Span::styled(" ".repeat(space), Style::default().bg(SURFACE_HI)),
                    Span::styled(tail, Style::default().fg(TXT_DIM)),
                ])
            })
            .map(ListItem::new)
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(BORDER)
                    .title(Line::from("Commands".bold().fg(TXT)))
                    .style(Style::default().bg(SURFACE_HI)),
            )
            .highlight_style(Style::default().bg(SEL_BG).fg(SEL_FG).bold())
            .highlight_symbol("› ");

        f.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn draw_input_and_cursor(
        &self,
        f: &mut ratatui::Frame<'_>,
        area: Rect,
        browse_open: bool,
        listing_open: bool,
    ) {
        const PLACEHOLDER: &str = "Add a follow-up";
        const PLACEHOLDER_BROWSE: &str =
            "/browse · ↑↓ Enter · Esc to go back or exit browse · /slash still works";
        const PLACEHOLDER_LISTING: &str =
            "↑↓ pick row · Enter choose · Esc close list · /slash still works";

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(BORDER)
            .style(Style::default().bg(SURFACE));

        let inner = block.inner(area);
        let prefix_cols = INPUT_PREFIX.width() as u16;

        let [prefix_area, value_area] =
            Layout::horizontal([Constraint::Length(prefix_cols), Constraint::Min(0)]).areas(inner);

        f.render_widget(
            Paragraph::new("")
                .style(Style::default().bg(SURFACE))
                .block(block),
            area,
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                INPUT_PREFIX,
                Style::default().fg(ACCENT).bold(),
            )]))
            .style(Style::default().bg(SURFACE)),
            prefix_area,
        );

        let value_w = value_area.width.max(1) as usize;
        let scroll = self.input.visual_scroll(value_w);

        let placeholder = if browse_open {
            PLACEHOLDER_BROWSE
        } else if listing_open {
            PLACEHOLDER_LISTING
        } else {
            PLACEHOLDER
        };

        let value_par = if self.input.value().is_empty() {
            Paragraph::new(Line::from(vec![Span::styled(
                placeholder,
                Style::default().fg(TXT_DIM),
            )]))
            .style(Style::default().bg(SURFACE))
        } else {
            Paragraph::new(self.input.value())
                .style(Style::default().fg(TXT).bg(SURFACE))
                .scroll((0, scroll as u16))
        };
        f.render_widget(value_par, value_area);

        let x = self.input.visual_cursor().max(scroll) - scroll;
        let max_x = value_area.width.saturating_sub(1);
        let cx = value_area.x + (x as u16).min(max_x);
        f.set_cursor_position((cx, value_area.y));
    }

    fn on_event(&mut self, ev: Event, term: &mut DefaultTerminal) -> Result<()> {
        match ev {
            Event::Resize(_, _) => {}
            Event::Key(ke) => {
                if ke.kind == KeyEventKind::Release {
                    return Ok(());
                }
                let palette = slash_palette_indices(self.input.value());
                let browse_exclusive = self.browse_exclusive(&palette);
                let listing_exclusive = self.listing_picker.is_some()
                    && palette.as_ref().is_none_or(|cmds| cmds.is_empty());
                match ke.code {
                    KeyCode::Char('c') if ke.modifiers.contains(KeyModifiers::CONTROL) => {
                        self.quit = true;
                        return Ok(());
                    }
                    KeyCode::Esc => {
                        if palette.is_some() && self.input.value().trim_start().starts_with('/') {
                            self.input.reset();
                            self.list_state.select(None);
                            return Ok(());
                        }
                        if listing_exclusive {
                            self.clear_listing_picker();
                            return Ok(());
                        }
                        if let Some(ref mut sess) = self.browse {
                            match sess.pop_navigation() {
                                BrowsePopOutcome::SteppedBack => {
                                    self.status = None;
                                    self.status_err = false;
                                    self.browse_list_state.select(Some(0));
                                    self.sync_browse_list_selection();
                                }
                                BrowsePopOutcome::ExitedBrowse => {
                                    self.browse = None;
                                    self.browse_list_state.select(None);
                                }
                            }
                            return Ok(());
                        }
                        self.quit = true;
                        return Ok(());
                    }
                    KeyCode::Enter => {
                        if listing_exclusive {
                            self.handle_listing_enter()?;
                            return Ok(());
                        }
                        if browse_exclusive {
                            self.handle_browse_enter()?;
                            return Ok(());
                        }
                        let line_to_submit = if let Some(idxs) = palette.as_ref() {
                            if let Some(si) = self.list_state.selected() {
                                if let Some(&ci) = idxs.get(si) {
                                    format!("/{}", CMDS.get(ci).map(|c| c.name).unwrap_or(""))
                                } else {
                                    self.input.value().trim().to_string()
                                }
                            } else if idxs.len() == 1 {
                                format!("/{}", CMDS.get(idxs[0]).map(|c| c.name).unwrap_or(""))
                            } else {
                                self.input.value().trim().to_string()
                            }
                        } else {
                            self.input.value().trim().to_string()
                        };

                        let effect = apply_submit(self, &line_to_submit);
                        self.input.reset();
                        self.list_state.select(None);

                        if let SubmitEffect::Browse { access_token } = &effect {
                            let credentials_note = authdog_cli::session_store::credentials_path()
                                .ok()
                                .map(|path| format!("credentials file: {}", path.display()));
                            match crate::browse::BrowseSession::begin(
                                access_token.clone(),
                                credentials_note,
                            ) {
                                Ok(sess) => {
                                    self.browse = Some(sess);
                                    self.browse_list_state.select(Some(0));
                                    self.sync_browse_list_selection();
                                }
                                Err(err) => {
                                    self.status =
                                        Some(format!("Browse could not load listings:\n{err:#}"));
                                    self.status_err = true;
                                }
                            }
                        }

                        self.handle_submit_followup(term, effect)?;
                        return Ok(());
                    }
                    KeyCode::Down if browse_exclusive => {
                        self.browse_list_state.select_next();
                    }
                    KeyCode::Up if browse_exclusive => {
                        self.browse_list_state.select_previous();
                    }
                    KeyCode::Down if listing_exclusive => {
                        self.listing_list_state.select_next();
                    }
                    KeyCode::Up if listing_exclusive => {
                        self.listing_list_state.select_previous();
                    }
                    KeyCode::Down if palette.as_ref().is_some_and(|v| !v.is_empty()) => {
                        self.list_state.select_next();
                    }
                    KeyCode::Up if palette.as_ref().is_some_and(|v| !v.is_empty()) => {
                        self.list_state.select_previous();
                    }
                    KeyCode::Tab => {
                        if self.browse.is_none()
                            && self.listing_picker.is_none()
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.whoami_output.is_some()
                        {
                            if let Some(ref mut pane) = self.whoami_output {
                                pane.tab = match pane.tab {
                                    WhoamiJsonTab::Pretty => WhoamiJsonTab::Raw,
                                    WhoamiJsonTab::Raw => WhoamiJsonTab::Pretty,
                                };
                                return Ok(());
                            }
                        }
                        if let Some(ref idxs) = palette {
                            if idxs.len() > 1 {
                                let sel = self.list_state.selected().unwrap_or(0);
                                let next = (sel + 1) % idxs.len();
                                self.list_state.select(Some(next));
                                return Ok(());
                            }
                        }
                    }
                    KeyCode::BackTab => {
                        if self.browse.is_none()
                            && self.listing_picker.is_none()
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.whoami_output.is_some()
                        {
                            if let Some(ref mut pane) = self.whoami_output {
                                pane.tab = match pane.tab {
                                    WhoamiJsonTab::Pretty => WhoamiJsonTab::Raw,
                                    WhoamiJsonTab::Raw => WhoamiJsonTab::Pretty,
                                };
                                return Ok(());
                            }
                        }
                        if let Some(ref idxs) = palette {
                            if !idxs.is_empty() {
                                let sel = self.list_state.selected().unwrap_or(0);
                                let prev = sel.checked_sub(1).unwrap_or(idxs.len() - 1);
                                self.list_state.select(Some(prev));
                                return Ok(());
                            }
                        }
                    }
                    KeyCode::PageUp
                        if (self.status.is_some() || self.whoami_output.is_some())
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.browse.is_none()
                            && self.listing_picker.is_none() =>
                    {
                        let step = if self.last_status_viewport_h > 0 {
                            self.last_status_viewport_h
                        } else {
                            12
                        };
                        self.status_scroll_row = self.status_scroll_row.saturating_sub(step);
                        return Ok(());
                    }
                    KeyCode::PageDown
                        if (self.status.is_some() || self.whoami_output.is_some())
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.browse.is_none()
                            && self.listing_picker.is_none() =>
                    {
                        let step = if self.last_status_viewport_h > 0 {
                            self.last_status_viewport_h
                        } else {
                            12
                        };
                        self.status_scroll_row = cmp::min(
                            self.status_scroll_row.saturating_add(step),
                            self.last_status_scroll_row_max,
                        );
                        return Ok(());
                    }
                    KeyCode::Home
                        if (self.status.is_some() || self.whoami_output.is_some())
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.browse.is_none()
                            && self.listing_picker.is_none() =>
                    {
                        self.status_scroll_row = 0;
                        return Ok(());
                    }
                    KeyCode::End
                        if (self.status.is_some() || self.whoami_output.is_some())
                            && palette.as_ref().is_none_or(|v| v.is_empty())
                            && self.browse.is_none()
                            && self.listing_picker.is_none() =>
                    {
                        self.status_scroll_row = self.last_status_scroll_row_max;
                        return Ok(());
                    }
                    _ => {}
                }

                if !browse_exclusive && !listing_exclusive {
                    let _ = self.input.handle_event(&ev);
                }
            }
            Event::Mouse(me) => {
                if self.browse.is_some() || self.listing_picker.is_some() {
                    return Ok(());
                }
                let palette = slash_palette_indices(self.input.value());
                if (self.status.is_none() && self.whoami_output.is_none())
                    || palette.as_ref().is_some_and(|v| !v.is_empty())
                {
                    return Ok(());
                }
                let Some(rect) = self.session_output_rect else {
                    return Ok(());
                };
                if !rect_contains(rect, me.column, me.row) {
                    return Ok(());
                }
                let dir: i32 = match me.kind {
                    MouseEventKind::ScrollUp => -1,
                    MouseEventKind::ScrollDown => 1,
                    _ => return Ok(()),
                };
                let lines = i32::from(wheel_scroll_lines(self.last_status_viewport_h));
                let delta = dir.saturating_mul(lines);
                if delta < 0 {
                    self.status_scroll_row = self.status_scroll_row.saturating_sub((-delta) as u16);
                } else {
                    self.status_scroll_row = cmp::min(
                        self.status_scroll_row.saturating_add(delta as u16),
                        self.last_status_scroll_row_max,
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_submit_followup(
        &mut self,
        term: &mut DefaultTerminal,
        effect: SubmitEffect,
    ) -> Result<()> {
        if !matches!(effect, SubmitEffect::BrowserLogin) {
            return Ok(());
        }
        cli_login::suspend_tui_for_shell_io()?;
        let cfg = cli_login::CliAuthConfig::from_env();
        let result = cli_login::run_browser_login_blocking(&cfg);
        if let Err(err) = cli_login::resume_tui_io() {
            eprintln!("warning: failed to resume TUI (terminal mode): {err:#}");
        } else {
            // Leaving alternate screen desyncs Ratatui's diff buffer from the terminal; force a
            // full redraw so incremental updates do not leave stray rows (e.g. footer hints).
            term.clear().context("clear terminal after OAuth resume")?;
        }
        match result {
            Ok(()) => {
                // Heavy check ✔ rendered green in [`tui_output::styled_status_lines`].
                self.status = Some("\u{2714} Signed in".into());
                self.status_err = false;
                self.status_clear_at = Some(Instant::now() + LOGIN_SUCCESS_STATUS_TTL);
            }
            Err(err) => {
                self.status_clear_at = None;
                self.status = Some(format!("Login failed:\n{err:#}",));
                self.status_err = true;
            }
        }
        Ok(())
    }
}

/// Terminal cells per wheel notch (fraction of the session viewport, at least 1 row).
fn wheel_scroll_lines(viewport_h: u16) -> u16 {
    let h = viewport_h.max(1);
    cmp::max(1, h / 5)
}

fn rect_contains(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x
        && col < r.x.saturating_add(r.width)
        && row >= r.y
        && row < r.y.saturating_add(r.height)
}

fn tenant_row_primary(t: &TenantRow) -> String {
    if let Some(ref n) = t.name {
        let nt = n.trim();
        if !nt.is_empty() && nt != t.id {
            return format!("{nt} ({})", t.id);
        }
    }
    t.id.clone()
}

fn browse_block_title(session: &BrowseSession) -> Line<'static> {
    match &session.step {
        BrowseStep::PickOrganization => Line::from(vec![
            Span::styled("Organizations", Style::default().fg(TXT).bold()),
            Span::styled(
                format!(" · pick one ({})", session.organizations.len()),
                TXT_DIM,
            ),
        ]),
        BrowseStep::PickTenant {
            org_summary,
            tenants,
        } => Line::from(vec![
            Span::styled("Tenants · ", Style::default().fg(TXT).bold()),
            Span::styled(
                truncate_vis(org_summary.as_str(), 44),
                Style::default().fg(TXT),
            ),
            Span::styled(format!(" · {} rows", tenants.len()), TXT_DIM),
        ]),
        BrowseStep::PickProject {
            tenant_summary,
            projects,
            ..
        } => Line::from(vec![
            Span::styled("Projects · ", Style::default().fg(TXT).bold()),
            Span::styled(
                truncate_vis(tenant_summary.as_str(), 36),
                Style::default().fg(TXT),
            ),
            Span::styled(format!(" · {} rows", projects.len()), TXT_DIM),
        ]),
        BrowseStep::PickEnvironment {
            application_summary,
            environments,
            tenant_summary,
            ..
        } => Line::from(vec![
            Span::styled("Environments · ", Style::default().fg(TXT).bold()),
            Span::styled(
                truncate_vis(application_summary.as_str(), 28),
                Style::default().fg(TXT),
            ),
            Span::styled(" · ", TXT_DIM),
            Span::styled(truncate_vis(tenant_summary.as_str(), 20), TXT_DIM),
            Span::styled(format!(" · {} rows", environments.len()), TXT_DIM),
        ]),
    }
}

fn browse_org_line(o: &OrgRow, vw: usize) -> Line<'static> {
    let prim = o.display_primary();
    let label = if prim.as_str() == o.id.as_str() {
        o.id.clone()
    } else {
        format!(
            "{prim}  ·  {}",
            truncate_vis(
                o.id.as_str(),
                vw.saturating_sub(prim.width().saturating_add(5)),
            )
        )
    };
    Line::from(Span::styled(
        truncate_vis(&label, vw),
        Style::default().fg(TXT).bold(),
    ))
}

fn browse_tenant_line(t: &TenantRow, vw: usize) -> Line<'static> {
    Line::from(Span::styled(
        truncate_vis(&tenant_row_primary(t), vw),
        Style::default().fg(TXT),
    ))
}

fn browse_project_line(p: &ProjectRow, vw: usize) -> Line<'static> {
    let prim = p.display_primary();
    let type_bit = p
        .project_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|ty| format!("  ({ty})"))
        .unwrap_or_default();
    let label = if prim.as_str() == p.id.as_str() {
        format!("{prim}{type_bit}")
    } else {
        format!(
            "{prim}  ·  {}{type_bit}",
            truncate_vis(
                p.id.as_str(),
                vw.saturating_sub(
                    prim.width()
                        .saturating_add(type_bit.width())
                        .saturating_add(8),
                ),
            )
        )
    };
    Line::from(Span::styled(
        truncate_vis(&label, vw),
        Style::default().fg(TXT),
    ))
}

fn browse_env_line(e: &EnvironmentRow, vw: usize) -> Line<'static> {
    let prim = e.display_primary();
    let label = if prim.as_str() == e.id.as_str() {
        e.id.clone()
    } else {
        format!(
            "{prim}  ·  {}",
            truncate_vis(
                e.id.as_str(),
                vw.saturating_sub(prim.width().saturating_add(5)),
            )
        )
    };
    Line::from(Span::styled(
        truncate_vis(&label, vw),
        Style::default().fg(TXT),
    ))
}

fn ansi_shadow_figlet() -> Option<&'static FIGlet> {
    static FONT: OnceLock<Option<FIGlet>> = OnceLock::new();
    FONT.get_or_init(|| FIGlet::from_content(ANSI_SHADOW_FLF).ok())
        .as_ref()
}

/// FIGlet banner sized to the viewport, or the previous single-line fallback.
fn figlet_banner_lines(term_width_cols: u16) -> (Vec<Line<'static>>, u16) {
    let w = term_width_cols as usize;

    let fallback_single = vec![Line::from(Span::styled(
        BANNER_FIGLET,
        Style::default()
            .fg(TXT)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))];

    let Some(font) = ansi_shadow_figlet() else {
        return (fallback_single.clone(), 1);
    };

    let Some(fig) = font.convert(BANNER_FIGLET) else {
        return (fallback_single.clone(), 1);
    };

    let raw_lines: Vec<String> = fig
        .as_str()
        .lines()
        .map(str::trim_end)
        .filter(|ln| !ln.is_empty())
        .map(str::to_owned)
        .collect();

    if raw_lines.is_empty() {
        return (fallback_single.clone(), 1);
    }

    let max_art_w = raw_lines.iter().map(|ln| ln.width()).max().unwrap_or(0);

    if max_art_w > w {
        return (fallback_single.clone(), 1);
    }

    let styled_lines: Vec<Line<'static>> = raw_lines
        .into_iter()
        .map(|t| Line::from(Span::styled(t, Style::default().fg(ACCENT).bold())))
        .collect();

    let h = cmp::max(1, styled_lines.len().min(usize::from(u16::MAX))) as u16;
    (styled_lines, h)
}

fn truncate_vis(s: &str, max_cols: usize) -> String {
    if s.width() <= max_cols {
        return s.to_string();
    }
    let mut acc = String::new();
    for ch in s.chars() {
        let next = format!("{acc}{ch}");
        if next.width() > max_cols.saturating_sub(1) {
            break;
        }
        acc.push(ch);
    }
    format!("{acc}…")
}

fn truncate_pad(s: &str, target: usize) -> String {
    let mut t = truncate_vis(s, target);
    while t.width() < target {
        t.push(' ');
    }
    t
}
