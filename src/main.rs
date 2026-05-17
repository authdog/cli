//! Authdog CLI — full-screen Ratatui (Crossterm) interface.

mod tui_output;

use authdog_cli::cli_login;
use authdog_cli::organizations;
use authdog_cli::session_store;
use authdog_cli::tenants;
use authdog_cli::whoami;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
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

struct SlashCmd {
    name: &'static str,
    desc: &'static str,
}

const CMDS: &[SlashCmd] = &[
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
        desc: "Tenants from api.authdog.com (/v1/tenants)",
    },
    SlashCmd {
        name: "organizations",
        desc: "Organizations (/v1/organizations; alias /orgs)",
    },
    SlashCmd {
        name: "status",
        desc: "Show session status",
    },
    SlashCmd {
        name: "quit",
        desc: "Exit the CLI",
    },
];

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

/// Hide the ✔ Signed in banner after this delay (header layout shrinks back).
const LOGIN_SUCCESS_STATUS_TTL: Duration = Duration::from_secs(4);

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SubmitEffect {
    None,
    BrowserLogin,
}

#[derive(Default)]
struct App {
    quit: bool,
    input: Input,
    list_state: ListState,
    status: Option<String>,
    status_err: bool,
    /// When set, clear `status` once `Instant::now()` passes (transient toasts).
    status_clear_at: Option<Instant>,
    /// Hash of [`App::status`] text; bumps reset vertical scroll position.
    status_scroll_digest: u64,
    /// Vertical scroll rows for [`Paragraph::scroll`] in the session output pane.
    status_scroll_row: u16,
    last_status_viewport_h: u16,
    last_status_scroll_row_max: u16,
}

fn main() -> Result<()> {
    let mut terminal = ratatui::init();
    App::default().run(&mut terminal)?;
    ratatui::restore();
    Ok(())
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

    fn run(mut self, term: &mut DefaultTerminal) -> Result<()> {
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

        self.draw_input_and_cursor(f, input_chunk);

        let mut hint = vec![
            "↑↓".dim(),
            " choose · Tab · ".into(),
            "Enter".bold(),
            " run · Esc".into(),
            " leave · Ctrl+C quit".dim(),
        ];
        if self.status.is_some() {
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
            hint.push(Span::styled(" output", Style::default().fg(TXT_DIM)));
        }
        f.render_widget(
            Paragraph::new(Line::from(hint))
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
        let mut lines = banner_lines;
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            "interactive CLI",
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
                Span::styled(" · PgUp/PgDn · Home/End", Style::default().fg(TXT_DIM)),
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

    fn draw_menu(&mut self, f: &mut ratatui::Frame<'_>, area: Rect, idxs: &[usize]) {
        let cmd_w = 11usize;
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

    fn draw_input_and_cursor(&self, f: &mut ratatui::Frame<'_>, area: Rect) {
        let width = area.width.max(4).saturating_sub(3);
        let scroll = self.input.visual_scroll(width as usize);

        let p = Paragraph::new(self.input.value())
            .style(Style::default().fg(TXT))
            .scroll((0, scroll as u16))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(BORDER)
                    .style(Style::default().bg(SURFACE))
                    .title(Line::from(Span::styled(
                        "→ ",
                        Style::default().fg(ACCENT).bold(),
                    )))
                    .title_alignment(Alignment::Left),
            );
        f.render_widget(p, area);

        let x = self
            .input
            .visual_cursor()
            .saturating_sub(scroll)
            .saturating_add(1);
        let max_x = area.width.saturating_sub(2); // inner text width (approx)
        let cx = cmp::min(x as u16, max_x.max(1));
        f.set_cursor_position((area.x + cx, area.y + 1));
    }

    fn on_event(&mut self, ev: Event, term: &mut DefaultTerminal) -> Result<()> {
        match ev {
            Event::Resize(_, _) => {}
            Event::Key(ke) => {
                if ke.kind == KeyEventKind::Release {
                    return Ok(());
                }
                let palette = slash_palette_indices(self.input.value());
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
                        self.quit = true;
                        return Ok(());
                    }
                    KeyCode::Enter => {
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

                        let effect = self.apply_submit(&line_to_submit);
                        self.input.reset();
                        self.list_state.select(None);
                        self.handle_submit_followup(term, effect)?;
                        return Ok(());
                    }
                    KeyCode::Down if palette.as_ref().is_some_and(|v| !v.is_empty()) => {
                        self.list_state.select_next();
                    }
                    KeyCode::Up if palette.as_ref().is_some_and(|v| !v.is_empty()) => {
                        self.list_state.select_previous();
                    }
                    KeyCode::Tab => {
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
                        if self.status.is_some()
                            && palette.as_ref().is_none_or(|v| v.is_empty()) =>
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
                        if self.status.is_some()
                            && palette.as_ref().is_none_or(|v| v.is_empty()) =>
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
                        if self.status.is_some()
                            && palette.as_ref().is_none_or(|v| v.is_empty()) =>
                    {
                        self.status_scroll_row = 0;
                        return Ok(());
                    }
                    KeyCode::End
                        if self.status.is_some()
                            && palette.as_ref().is_none_or(|v| v.is_empty()) =>
                    {
                        self.status_scroll_row = self.last_status_scroll_row_max;
                        return Ok(());
                    }
                    _ => {}
                }

                let _ = self.input.handle_event(&ev);
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
        if effect != SubmitEffect::BrowserLogin {
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

    fn apply_submit(&mut self, line: &str) -> SubmitEffect {
        let line = line.trim();
        if line.is_empty() {
            self.status_clear_at = None;
            self.status = None;
            self.status_err = false;
            return SubmitEffect::None;
        }

        self.status_clear_at = None;

        let first = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_ascii_lowercase();

        match first.as_str() {
            "quit" | "q" => {
                self.quit = true;
                SubmitEffect::None
            }
            "" | "help" | "h" | "?" => {
                let buf: String = CMDS
                    .iter()
                    .map(|c| format!("/{:<9} — {}", c.name, c.desc))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.status = Some(buf);
                self.status_err = false;
                SubmitEffect::None
            }
            "login" => {
                let cfg = cli_login::CliAuthConfig::from_env();
                self.status = Some(format!(
                    "Opening browser ({}/signin/{} …).\nAUTHDOG_IDENTITY_ORIGIN overrides the host (default: {}).",
                    cfg.identity_origin, cfg.environment_id, cli_login::DEFAULT_IDENTITY_ORIGIN,
                ));
                self.status_err = false;
                SubmitEffect::BrowserLogin
            }
            "logout" => match session_store::clear_session() {
                Ok(()) => {
                    self.status =
                        Some("Signed out locally (credentials file removed).\nRun /login to sign in again.".into());
                    self.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    self.status = Some(format!("{err:#}"));
                    self.status_err = true;
                    SubmitEffect::None
                }
            },
            "whoami" | "me" => match session_store::load_session() {
                Ok(Some(s)) => {
                    self.status = Some(whoami::compose_whoami_report(
                        &s.access_token,
                        session_store::credentials_path()
                            .ok()
                            .map(|path| format!("credentials file: {}", path.display())),
                    ));
                    self.status_err = false;
                    SubmitEffect::None
                }
                Ok(None) => {
                    self.status = Some(
                        "Not logged in (/whoami).\nTry /login, or use /status to confirm files."
                            .into(),
                    );
                    self.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    self.status = Some(format!("{err:#}"));
                    self.status_err = true;
                    SubmitEffect::None
                }
            },
            "tenants" => match session_store::load_session() {
                Ok(Some(s)) => {
                    self.status = Some(tenants::compose_tenants_report(
                        &s.access_token,
                        session_store::credentials_path()
                            .ok()
                            .map(|path| format!("credentials file: {}", path.display())),
                    ));
                    self.status_err = false;
                    SubmitEffect::None
                }
                Ok(None) => {
                    self.status = Some(
                        "Not logged in (/tenants).\nTry /login, or use /status to confirm files."
                            .into(),
                    );
                    self.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    self.status = Some(format!("{err:#}"));
                    self.status_err = true;
                    SubmitEffect::None
                }
            },
            "organizations" | "orgs" => match session_store::load_session() {
                Ok(Some(s)) => {
                    self.status = Some(organizations::compose_organizations_report(
                        &s.access_token,
                        session_store::credentials_path()
                            .ok()
                            .map(|path| format!("credentials file: {}", path.display())),
                    ));
                    self.status_err = false;
                    SubmitEffect::None
                }
                Ok(None) => {
                    self.status = Some(
                        "Not logged in (/organizations).\nTry /login, or use /status to confirm files."
                            .into(),
                    );
                    self.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    self.status = Some(format!("{err:#}"));
                    self.status_err = true;
                    SubmitEffect::None
                }
            },
            "status" => match session_store::load_session() {
                Ok(Some(s)) => {
                    let p = cmp::min(28, s.access_token.len());
                    let preview = if p == 0 {
                        String::new()
                    } else {
                        s.access_token[..p].to_string()
                    };
                    let path_show = session_store::credentials_path()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| "(unknown)".into());
                    self.status = Some(format!(
                        "Session file: {path_show}\nAccess token preview: {preview}… ({} chars)\nRefresh token: {} chars",
                        s.access_token.len(),
                        s.refresh_token.len(),
                    ));
                    self.status_err = false;
                    SubmitEffect::None
                }
                Ok(None) => {
                    self.status = Some("Not logged in (no credentials.json). Try /login.".into());
                    self.status_err = false;
                    SubmitEffect::None
                }
                Err(err) => {
                    self.status = Some(format!("{err:#}"));
                    self.status_err = true;
                    SubmitEffect::None
                }
            },
            _other => {
                if line.starts_with('/') {
                    self.status = Some(format!(
                        "unknown command: {}",
                        line.split_whitespace().next().unwrap_or("")
                    ));
                    self.status_err = true;
                } else {
                    self.status = Some(line.to_string());
                    self.status_err = false;
                }
                SubmitEffect::None
            }
        }
    }
}

fn slash_palette_indices(value: &str) -> Option<Vec<usize>> {
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
