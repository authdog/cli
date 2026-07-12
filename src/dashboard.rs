//! Screen-based Ratatui dashboard for Authdog CLI operations.

use anyhow::{Context, Result};
use authdog_cli::actions::{
    self, ContextResource, ListScopes, ResourceRows, SessionStatus,
};
use authdog_cli::cli_login;
use authdog_cli::organizations::OrgRow;
use authdog_cli::projects::{EnvironmentRow, ProjectRow};
use authdog_cli::tenants::TenantRow;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::DefaultTerminal;
use serde_json::Value;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

const BG: Color = Color::Rgb(31, 20, 35);
const SURFACE: Color = Color::Rgb(48, 34, 52);
const SURFACE_HI: Color = Color::Rgb(67, 48, 72);
const BORDER: Color = Color::Rgb(104, 78, 112);
const TEXT: Color = Color::Rgb(239, 228, 244);
const DIM: Color = Color::Rgb(165, 151, 173);
const ACCENT: Color = Color::Rgb(218, 184, 234);
const GOOD: Color = Color::Rgb(146, 220, 174);
const BAD: Color = Color::Rgb(240, 160, 184);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Screen {
    #[default]
    Overview,
    Identity,
    Organizations,
    Tenants,
    Projects,
    Environments,
    Context,
}

impl Screen {
    const ALL: [Self; 7] = [
        Self::Overview,
        Self::Identity,
        Self::Organizations,
        Self::Tenants,
        Self::Projects,
        Self::Environments,
        Self::Context,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Identity => "Identity",
            Self::Organizations => "Organizations",
            Self::Tenants => "Tenants",
            Self::Projects => "Projects",
            Self::Environments => "Environments",
            Self::Context => "Context",
        }
    }

    fn resource(self) -> Option<ContextResource> {
        match self {
            Self::Organizations => Some(ContextResource::Organization),
            Self::Tenants => Some(ContextResource::Tenant),
            Self::Projects => Some(ContextResource::Project),
            Self::Environments => Some(ContextResource::Environment),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
enum LoadState {
    Idle,
    Loading,
    Ready,
    Error(String),
}

#[derive(Clone, Debug)]
enum DashboardData {
    None,
    Identity(Value),
    Organizations(Vec<OrgRow>),
    Tenants(Vec<TenantRow>),
    Projects(Vec<ProjectRow>),
    Environments(Vec<EnvironmentRow>),
}

impl DashboardData {
    fn len(&self) -> usize {
        match self {
            Self::Organizations(rows) => rows.len(),
            Self::Tenants(rows) => rows.len(),
            Self::Projects(rows) => rows.len(),
            Self::Environments(rows) => rows.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ConfirmAction {
    Set(ContextResource),
    Clear(ContextResource),
    ClearAll,
    Logout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScopeField {
    Organization,
    Tenant,
    Project,
}

impl ScopeField {
    fn label(self) -> &'static str {
        match self {
            Self::Organization => "Organization override",
            Self::Tenant => "Tenant override",
            Self::Project => "Project override",
        }
    }
}

#[derive(Clone, Debug)]
enum Modal {
    Help,
    Search(Input),
    Scope {
        field: ScopeField,
        input: Input,
    },
    EditContext {
        resource: ContextResource,
        input: Input,
    },
    Confirm {
        action: ConfirmAction,
        id: Option<String>,
    },
    Palette {
        selected: usize,
    },
}

#[derive(Clone, Debug)]
enum Request {
    Status,
    Identity,
    List(ContextResource, ListScopes),
}

#[derive(Debug)]
enum Response {
    Status(SessionStatus),
    Identity(Value),
    Rows(ResourceRows),
}

#[derive(Debug)]
struct WorkerMessage {
    token: u64,
    result: Result<Response, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Effect {
    None,
    Login,
}

pub(crate) struct App {
    quit: bool,
    screen: Screen,
    nav_state: ListState,
    list_state: ListState,
    context_state: ListState,
    load: LoadState,
    data: DashboardData,
    session: Option<SessionStatus>,
    filter: String,
    scopes: ListScopes,
    modal: Option<Modal>,
    notice: Option<(String, bool)>,
    identity_raw: bool,
    request_token: u64,
    tx: Sender<WorkerMessage>,
    rx: Receiver<WorkerMessage>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut nav_state = ListState::default();
        nav_state.select(Some(0));
        let mut context_state = ListState::default();
        context_state.select(Some(0));
        let mut app = Self {
            quit: false,
            screen: Screen::Overview,
            nav_state,
            list_state: ListState::default(),
            context_state,
            load: LoadState::Idle,
            data: DashboardData::None,
            session: None,
            filter: String::new(),
            scopes: ListScopes::default(),
            modal: None,
            notice: None,
            identity_raw: false,
            request_token: 0,
            tx,
            rx,
        };
        app.request(Request::Status);
        app
    }

    pub(crate) fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.drain_worker();
            terminal.draw(|frame| self.draw(frame))?;
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release {
                        let effect = self.handle_key(key);
                        if effect == Effect::Login {
                            self.login(terminal)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn request(&mut self, request: Request) {
        self.request_token = self.request_token.wrapping_add(1);
        let token = self.request_token;
        let tx = self.tx.clone();
        self.load = LoadState::Loading;
        self.notice = None;
        thread::spawn(move || {
            let result = run_request(request).map_err(|error| format!("{error:#}"));
            let _ = tx.send(WorkerMessage { token, result });
        });
    }

    fn drain_worker(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            if message.token != self.request_token {
                continue;
            }
            match message.result {
                Ok(Response::Status(status)) => {
                    self.session = Some(status);
                    self.load = LoadState::Ready;
                }
                Ok(Response::Identity(value)) => {
                    self.data = DashboardData::Identity(value);
                    self.load = LoadState::Ready;
                }
                Ok(Response::Rows(rows)) => {
                    self.data = match rows {
                        ResourceRows::Organizations(rows) => DashboardData::Organizations(rows),
                        ResourceRows::Tenants(rows) => DashboardData::Tenants(rows),
                        ResourceRows::Projects(rows) => DashboardData::Projects(rows),
                        ResourceRows::Environments(rows) => DashboardData::Environments(rows),
                    };
                    self.load = LoadState::Ready;
                    self.select_first_visible();
                }
                Err(error) => {
                    self.load = LoadState::Error(error);
                    self.data = DashboardData::None;
                    self.list_state.select(None);
                }
            }
        }
    }

    fn navigate(&mut self, screen: Screen) {
        self.screen = screen;
        self.nav_state
            .select(Screen::ALL.iter().position(|candidate| *candidate == screen));
        self.modal = None;
        self.notice = None;
        self.filter.clear();
        self.data = DashboardData::None;
        self.list_state.select(None);
        self.refresh();
    }

    fn refresh(&mut self) {
        match self.screen {
            Screen::Overview | Screen::Context => self.request(Request::Status),
            Screen::Identity => self.request(Request::Identity),
            screen => {
                if let Some(resource) = screen.resource() {
                    self.request(Request::List(resource, self.scopes.clone()));
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Effect {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return Effect::None;
        }
        if self.modal.is_some() {
            return self.handle_modal_key(key);
        }
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.modal = Some(Modal::Palette { selected: 0 });
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Left | KeyCode::Char('h') => self.move_nav(-1),
            KeyCode::Right | KeyCode::Char('l') if self.is_logged_in() => self.move_nav(1),
            KeyCode::Char('l') if !self.is_logged_in() => return Effect::Login,
            KeyCode::Char('L') if self.is_logged_in() => {
                self.modal = Some(Modal::Confirm {
                    action: ConfirmAction::Logout,
                    id: None,
                });
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Char('/') if self.screen.resource().is_some() => {
                self.modal = Some(Modal::Search(Input::new(self.filter.clone())));
            }
            KeyCode::Char('s') => self.open_scope_editor(),
            KeyCode::Enter => self.confirm_selected_context(),
            KeyCode::Char('x') => self.confirm_clear_selected(),
            KeyCode::Char('X') if self.screen == Screen::Context => {
                self.modal = Some(Modal::Confirm {
                    action: ConfirmAction::ClearAll,
                    id: None,
                });
            }
            KeyCode::Char('e') if self.screen == Screen::Context => self.edit_context(),
            KeyCode::Tab if self.screen == Screen::Identity => {
                self.identity_raw = !self.identity_raw;
            }
            _ => {}
        }
        Effect::None
    }

    fn handle_modal_key(&mut self, key: KeyEvent) -> Effect {
        let Some(mut modal) = self.modal.take() else {
            return Effect::None;
        };
        match &mut modal {
            Modal::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?')) {
                    return Effect::None;
                }
            }
            Modal::Search(input) => match key.code {
                KeyCode::Esc => return Effect::None,
                KeyCode::Enter => {
                    self.filter = input.value().trim().to_string();
                    self.select_first_visible();
                    return Effect::None;
                }
                _ => {
                    let _ = input.handle_event(&Event::Key(key));
                }
            },
            Modal::Scope { field, input } => match key.code {
                KeyCode::Esc => return Effect::None,
                KeyCode::Tab if self.screen == Screen::Environments => {
                    let value = input.value().trim().to_string();
                    self.set_scope(*field, value);
                    *field = match field {
                        ScopeField::Tenant => ScopeField::Project,
                        _ => ScopeField::Tenant,
                    };
                    *input = Input::new(self.scope_value(*field).to_string());
                }
                KeyCode::Enter => {
                    let value = input.value().trim().to_string();
                    self.set_scope(*field, value);
                    self.refresh();
                    return Effect::None;
                }
                _ => {
                    let _ = input.handle_event(&Event::Key(key));
                }
            },
            Modal::EditContext { resource, input } => match key.code {
                KeyCode::Esc => return Effect::None,
                KeyCode::Enter => {
                    let id = input.value().trim().to_string();
                    if id.is_empty() {
                        self.notice = Some(("Context ID cannot be empty.".into(), true));
                        return Effect::None;
                    }
                    self.modal = Some(Modal::Confirm {
                        action: ConfirmAction::Set(*resource),
                        id: Some(id),
                    });
                    return Effect::None;
                }
                _ => {
                    let _ = input.handle_event(&Event::Key(key));
                }
            },
            Modal::Confirm { action, id } => match key.code {
                KeyCode::Esc | KeyCode::Char('n') => return Effect::None,
                KeyCode::Enter | KeyCode::Char('y') => {
                    self.apply_confirm(*action, id.clone());
                    return Effect::None;
                }
                _ => {}
            },
            Modal::Palette { selected } => match key.code {
                KeyCode::Esc => return Effect::None,
                KeyCode::Down | KeyCode::Char('j') => {
                    *selected = (*selected + 1).min(Screen::ALL.len() - 1);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Enter => {
                    self.navigate(Screen::ALL[*selected]);
                    return Effect::None;
                }
                _ => {}
            },
        }
        self.modal = Some(modal);
        Effect::None
    }

    fn apply_confirm(&mut self, action: ConfirmAction, id: Option<String>) {
        let result = match action {
            ConfirmAction::Set(resource) => {
                actions::set_context(resource, id.unwrap_or_default()).map(|id| {
                    format!("Current {} set to {id}.", resource.name())
                })
            }
            ConfirmAction::Clear(resource) => actions::clear_context(Some(resource))
                .map(|()| format!("Current {} cleared.", resource.name())),
            ConfirmAction::ClearAll => {
                actions::clear_context(None).map(|()| "All resource context cleared.".into())
            }
            ConfirmAction::Logout => {
                actions::logout().map(|()| "Signed out. Local credentials removed.".into())
            }
        };
        match result {
            Ok(message) => self.notice = Some((message, false)),
            Err(error) => self.notice = Some((format!("{error:#}"), true)),
        }
        self.request(Request::Status);
    }

    fn login(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        cli_login::suspend_tui_for_shell_io()?;
        let result = cli_login::run_browser_login_blocking(&cli_login::CliAuthConfig::from_env());
        if let Err(error) = cli_login::resume_tui_io() {
            eprintln!("warning: failed to resume TUI: {error:#}");
        } else {
            terminal.clear().context("clear terminal after OAuth resume")?;
        }
        match result {
            Ok(()) => {
                self.notice = Some(("Signed in to Authdog.".into(), false));
                self.request(Request::Status);
            }
            Err(error) => self.notice = Some((format!("Login failed: {error:#}"), true)),
        }
        Ok(())
    }

    fn is_logged_in(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(SessionStatus::logged_in)
    }

    fn move_nav(&mut self, delta: isize) {
        let current = Screen::ALL
            .iter()
            .position(|screen| *screen == self.screen)
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, Screen::ALL.len() as isize - 1) as usize;
        self.navigate(Screen::ALL[next]);
    }

    fn move_selection(&mut self, delta: isize) {
        if self.screen == Screen::Context {
            let current = self.context_state.selected().unwrap_or(0) as isize;
            self.context_state
                .select(Some((current + delta).clamp(0, 3) as usize));
            return;
        }
        let visible = self.visible_indices();
        if visible.is_empty() {
            self.list_state.select(None);
            return;
        }
        let current = self
            .list_state
            .selected()
            .and_then(|selected| visible.iter().position(|index| *index == selected))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, visible.len() as isize - 1) as usize;
        self.list_state.select(Some(visible[next]));
    }

    fn select_first_visible(&mut self) {
        self.list_state.select(self.visible_indices().first().copied());
    }

    fn visible_indices(&self) -> Vec<usize> {
        let needle = self.filter.trim().to_ascii_lowercase();
        (0..self.data.len())
            .filter(|index| {
                needle.is_empty()
                    || self
                        .row_search_text(*index)
                        .to_ascii_lowercase()
                        .contains(&needle)
            })
            .collect()
    }

    fn row_search_text(&self, index: usize) -> String {
        let (name, id) = self.row_name_id(index).unwrap_or_default();
        format!("{name} {id}")
    }

    fn row_name_id(&self, index: usize) -> Option<(String, String)> {
        match &self.data {
            DashboardData::Organizations(rows) => rows.get(index).map(|row| {
                (
                    row.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    row.id.clone(),
                )
            }),
            DashboardData::Tenants(rows) => rows.get(index).map(|row| {
                (
                    row.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    row.id.clone(),
                )
            }),
            DashboardData::Projects(rows) => rows.get(index).map(|row| {
                (
                    row.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    row.id.clone(),
                )
            }),
            DashboardData::Environments(rows) => rows.get(index).map(|row| {
                (
                    row.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    row.id.clone(),
                )
            }),
            _ => None,
        }
    }

    fn selected_id(&self) -> Option<String> {
        self.list_state
            .selected()
            .and_then(|index| self.row_name_id(index))
            .map(|(_, id)| id)
    }

    fn confirm_selected_context(&mut self) {
        let Some(resource) = self.screen.resource() else {
            return;
        };
        let Some(id) = self.selected_id() else {
            return;
        };
        self.modal = Some(Modal::Confirm {
            action: ConfirmAction::Set(resource),
            id: Some(id),
        });
    }

    fn confirm_clear_selected(&mut self) {
        let resource = if self.screen == Screen::Context {
            self.context_state
                .selected()
                .and_then(|index| ContextResource::ALL.get(index).copied())
        } else {
            self.screen.resource()
        };
        if let Some(resource) = resource {
            self.modal = Some(Modal::Confirm {
                action: ConfirmAction::Clear(resource),
                id: None,
            });
        }
    }

    fn edit_context(&mut self) {
        let Some(resource) = self
            .context_state
            .selected()
            .and_then(|index| ContextResource::ALL.get(index).copied())
        else {
            return;
        };
        let value = self.context_value(resource).unwrap_or_default().to_string();
        self.modal = Some(Modal::EditContext {
            resource,
            input: Input::new(value),
        });
    }

    fn open_scope_editor(&mut self) {
        let field = match self.screen {
            Screen::Tenants => ScopeField::Organization,
            Screen::Projects | Screen::Environments => ScopeField::Tenant,
            _ => return,
        };
        self.modal = Some(Modal::Scope {
            field,
            input: Input::new(self.scope_value(field).to_string()),
        });
    }

    fn scope_value(&self, field: ScopeField) -> &str {
        let value = match field {
            ScopeField::Organization => self.scopes.organization.as_deref(),
            ScopeField::Tenant => self.scopes.tenant.as_deref(),
            ScopeField::Project => self.scopes.project.as_deref(),
        };
        value.unwrap_or("")
    }

    fn set_scope(&mut self, field: ScopeField, value: String) {
        let value = (!value.is_empty()).then_some(value);
        match field {
            ScopeField::Organization => self.scopes.organization = value,
            ScopeField::Tenant => self.scopes.tenant = value,
            ScopeField::Project => self.scopes.project = value,
        }
    }

    fn context_value(&self, resource: ContextResource) -> Option<&str> {
        let session = self.session.as_ref()?.session.as_ref()?;
        match resource {
            ContextResource::Organization => session.current_organization_id.as_deref(),
            ContextResource::Tenant => session.current_tenant_id.as_deref(),
            ContextResource::Project => session.current_application_id.as_deref(),
            ContextResource::Environment => session.current_environment_id.as_deref(),
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame<'_>) {
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
        if area.width < 64 || area.height < 18 {
            frame.render_widget(
                Paragraph::new("Terminal too small (need at least 64×18)")
                    .centered()
                    .style(DIM),
                area,
            );
            return;
        }
        let outer = area.inner(Margin::new(1, 1));
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(2),
        ])
        .areas(outer);
        self.draw_header(frame, header);
        let [nav, content] =
            Layout::horizontal([Constraint::Length(21), Constraint::Fill(1)]).areas(body);
        self.draw_navigation(frame, nav);
        self.draw_content(frame, content);
        self.draw_footer(frame, footer);
        if let Some(modal) = self.modal.clone() {
            self.draw_modal(frame, area, modal);
        }
    }

    fn draw_header(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let auth = if self.is_logged_in() {
            Span::styled("● signed in", GOOD)
        } else {
            Span::styled("○ signed out", BAD)
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" authdog ", Style::default().fg(BG).bg(ACCENT).bold()),
                Span::raw("  "),
                Span::styled("TUI Dashboard", Style::default().fg(TEXT).bold()),
                Span::raw("  "),
                auth,
            ]))
            .block(Block::default().borders(Borders::BOTTOM).border_style(BORDER))
            .style(Style::default().bg(BG)),
            area,
        );
    }

    fn draw_navigation(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let items = Screen::ALL
            .iter()
            .map(|screen| ListItem::new(format!("  {}", screen.title())))
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Navigate ")
                    .borders(Borders::ALL)
                    .border_style(BORDER),
            )
            .style(TEXT)
            .highlight_style(Style::default().fg(BG).bg(ACCENT).bold())
            .highlight_symbol("›");
        frame.render_stateful_widget(list, area, &mut self.nav_state);
    }

    fn draw_content(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        match self.screen {
            Screen::Overview => self.draw_overview(frame, area),
            Screen::Identity => self.draw_identity(frame, area),
            Screen::Context => self.draw_context(frame, area),
            _ => self.draw_resources(frame, area),
        }
    }

    fn draw_overview(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let text = if let LoadState::Error(error) = &self.load {
            format!("Could not load session\n\n{error}")
        } else if let Some(status) = &self.session {
            let mut lines = vec![
                format!(
                    "Authentication   {}",
                    if status.logged_in() {
                        "Signed in"
                    } else {
                        "Signed out"
                    }
                ),
                format!("Credentials      {}", status.credentials_path.display()),
                String::new(),
                "Current context".into(),
            ];
            for resource in ContextResource::ALL {
                lines.push(format!(
                    "  {:<13} {}",
                    resource.name(),
                    self.context_value(resource).unwrap_or("(none)")
                ));
            }
            lines.push(String::new());
            lines.push(if status.logged_in() {
                "Use the navigation panel to inspect resources. Tokens are never displayed.".into()
            } else {
                "Press l to sign in through your browser.".into()
            });
            lines.join("\n")
        } else {
            "Loading session…".into()
        };
        frame.render_widget(panel(" Overview ", text), area);
    }

    fn draw_identity(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let text = match (&self.load, &self.data) {
            (LoadState::Loading, _) => "Loading server-checked identity…".into(),
            (LoadState::Error(error), _) => format!("Could not load identity\n\n{error}"),
            (_, DashboardData::Identity(value)) if self.identity_raw => {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
            (_, DashboardData::Identity(value)) => identity_summary(value),
            _ => "No identity loaded.".into(),
        };
        let title = if self.identity_raw {
            " Identity · Raw "
        } else {
            " Identity · Pretty "
        };
        frame.render_widget(panel(title, text), area);
    }

    fn draw_resources(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
                .areas(area);
        let visible = self.visible_indices();
        let items = visible
            .iter()
            .filter_map(|index| {
                self.row_name_id(*index)
                    .map(|(name, id)| ListItem::new(vec![Line::from(name), Line::from(id).dim()]))
            })
            .collect::<Vec<_>>();
        let scope = self.scope_summary();
        let title = format!(
            " {} · {}{} ",
            self.screen.title(),
            visible.len(),
            if scope.is_empty() {
                String::new()
            } else {
                format!(" · {scope}")
            }
        );
        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(BORDER),
            )
            .highlight_style(Style::default().fg(BG).bg(ACCENT))
            .highlight_symbol("› ");
        if matches!(self.load, LoadState::Ready) {
            let mut visible_state = ListState::default();
            visible_state.select(self.list_state.selected().and_then(|selected| {
                visible.iter().position(|index| *index == selected)
            }));
            frame.render_stateful_widget(list, list_area, &mut visible_state);
        } else {
            frame.render_widget(list, list_area);
        }
        let detail = match &self.load {
            LoadState::Loading => "Loading…".into(),
            LoadState::Error(error) => format!("Request failed\n\n{error}\n\nPress r to retry."),
            LoadState::Ready if visible.is_empty() => {
                if self.filter.is_empty() {
                    "No resources found.".into()
                } else {
                    format!("No matches for “{}”.\n\nPress / to change the filter.", self.filter)
                }
            }
            LoadState::Ready => self.selected_detail(),
            LoadState::Idle => "Choose a resource screen.".into(),
        };
        frame.render_widget(panel(" Details ", detail), detail_area);
    }

    fn draw_context(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let items = ContextResource::ALL
            .iter()
            .map(|resource| {
                ListItem::new(vec![
                    Line::from(resource.name().to_string().to_uppercase()).bold(),
                    Line::from(
                        self.context_value(*resource)
                            .unwrap_or("(none)")
                            .to_string(),
                    )
                    .dim(),
                ])
            })
            .collect::<Vec<_>>();
        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Saved context ")
                    .borders(Borders::ALL)
                    .border_style(BORDER),
            )
            .highlight_style(Style::default().fg(BG).bg(ACCENT))
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, area, &mut self.context_state);
    }

    fn draw_footer(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let notice = self.notice.as_ref().map(|(message, error)| {
            Span::styled(
                format!(" {message} "),
                if *error { BAD } else { GOOD },
            )
        });
        let hints = match self.screen {
            Screen::Context => "↑↓ select · e edit · x clear · X clear all",
            Screen::Identity => "Tab pretty/raw · r refresh",
            Screen::Organizations => "↑↓ select · Enter set current · / search · r refresh",
            Screen::Tenants | Screen::Projects | Screen::Environments => {
                "↑↓ select · Enter set current · / search · s scope · r refresh"
            }
            Screen::Overview => {
                if self.is_logged_in() {
                    "←→ navigate · L sign out"
                } else {
                    "l sign in · ←→ navigate"
                }
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                notice.unwrap_or_else(|| Span::styled(format!(" {hints} "), DIM)),
                Span::styled("  ? help · Ctrl+P pages · q quit", DIM),
            ]))
            .block(Block::default().borders(Borders::TOP).border_style(BORDER)),
            area,
        );
    }

    fn draw_modal(&self, frame: &mut ratatui::Frame<'_>, area: Rect, modal: Modal) {
        let popup = centered_rect(64, 50, area);
        frame.render_widget(Clear, popup);
        match modal {
            Modal::Help => {
                let text = "Navigation\n  ←/→ or h/l   switch page\n  Ctrl+P       page palette\n\nResources\n  ↑/↓ or j/k   select\n  Enter        set selected as current\n  /            search locally\n  s            temporary API scope override\n  r            refresh\n\nContext\n  e edit · x clear · X clear all\n\nGlobal\n  l login · L logout · ? help · q quit";
                frame.render_widget(panel(" Help ", text.into()), popup);
            }
            Modal::Search(input) => {
                draw_input_modal(frame, popup, " Search ", &input, "Enter apply · Esc cancel");
            }
            Modal::Scope { field, input } => {
                let hint = if self.screen == Screen::Environments {
                    "Tab switch tenant/project · Enter apply · empty uses saved context"
                } else {
                    "Enter apply · empty uses saved context"
                };
                draw_input_modal(
                    frame,
                    popup,
                    &format!(" {} ", field.label()),
                    &input,
                    hint,
                );
            }
            Modal::EditContext { resource, input } => draw_input_modal(
                frame,
                popup,
                &format!(" Set {} context ", resource.name()),
                &input,
                "Enter review · Esc cancel",
            ),
            Modal::Confirm { action, id } => {
                let message = confirm_message(action, id.as_deref());
                frame.render_widget(
                    panel(
                        " Confirm ",
                        format!("{message}\n\nEnter/y confirm · Esc/n cancel"),
                    ),
                    popup,
                );
            }
            Modal::Palette { selected } => {
                let items = Screen::ALL
                    .iter()
                    .enumerate()
                    .map(|(index, screen)| {
                        let style = if index == selected {
                            Style::default().fg(BG).bg(ACCENT).bold()
                        } else {
                            Style::default().fg(TEXT)
                        };
                        ListItem::new(format!("  {}", screen.title())).style(style)
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    List::new(items).block(
                        Block::default()
                            .title(" Go to page ")
                            .borders(Borders::ALL)
                            .border_style(BORDER),
                    ),
                    popup,
                );
            }
        }
    }

    fn selected_detail(&self) -> String {
        let Some(index) = self.list_state.selected() else {
            return "Select a resource.".into();
        };
        match &self.data {
            DashboardData::Organizations(rows) => rows
                .get(index)
                .map(|row| {
                    format!(
                        "Name\n{}\n\nID\n{}",
                        row.name.as_deref().unwrap_or("(unnamed)"),
                        row.id
                    )
                })
                .unwrap_or_default(),
            DashboardData::Tenants(rows) => rows
                .get(index)
                .map(|row| {
                    format!(
                        "Name\n{}\n\nID\n{}\n\nOrganization\n{}",
                        row.name.as_deref().unwrap_or("(unnamed)"),
                        row.id,
                        row.organization_id.as_deref().unwrap_or("(not provided)")
                    )
                })
                .unwrap_or_default(),
            DashboardData::Projects(rows) => rows
                .get(index)
                .map(|row| {
                    format!(
                        "Name\n{}\n\nID\n{}\n\nType\n{}",
                        row.name.as_deref().unwrap_or("(unnamed)"),
                        row.id,
                        row.project_type.as_deref().unwrap_or("(not provided)")
                    )
                })
                .unwrap_or_default(),
            DashboardData::Environments(rows) => rows
                .get(index)
                .map(|row| {
                    format!(
                        "Name\n{}\n\nID\n{}",
                        row.name.as_deref().unwrap_or("(unnamed)"),
                        row.id
                    )
                })
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn scope_summary(&self) -> String {
        match self.screen {
            Screen::Tenants => self
                .scopes
                .organization
                .as_ref()
                .map(|id| format!("org {id}"))
                .unwrap_or_default(),
            Screen::Projects => self
                .scopes
                .tenant
                .as_ref()
                .map(|id| format!("tenant {id}"))
                .unwrap_or_default(),
            Screen::Environments => {
                let mut parts = Vec::new();
                if let Some(id) = &self.scopes.tenant {
                    parts.push(format!("tenant {id}"));
                }
                if let Some(id) = &self.scopes.project {
                    parts.push(format!("project {id}"));
                }
                parts.join(" · ")
            }
            _ => String::new(),
        }
    }
}

fn run_request(request: Request) -> Result<Response> {
    match request {
        Request::Status => Ok(Response::Status(actions::status()?)),
        Request::Identity => Ok(Response::Identity(actions::identity()?)),
        Request::List(resource, scopes) => {
            Ok(Response::Rows(actions::list_resources(resource, &scopes)?))
        }
    }
}

fn panel(title: &str, text: String) -> Paragraph<'static> {
    Paragraph::new(text)
        .block(
            Block::default()
                .title(title.to_string())
                .borders(Borders::ALL)
                .border_style(BORDER),
        )
        .style(Style::default().fg(TEXT).bg(SURFACE))
        .wrap(Wrap { trim: false })
}

fn draw_input_modal(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    input: &Input,
    hint: &str,
) {
    let [field, footer] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(2)]).areas(area);
    frame.render_widget(
        Paragraph::new(input.value().to_string())
            .block(
                Block::default()
                    .title(title.to_string())
                    .borders(Borders::ALL)
                    .border_style(ACCENT),
            )
            .style(Style::default().fg(TEXT).bg(SURFACE_HI)),
        field,
    );
    frame.render_widget(Paragraph::new(hint).style(DIM).centered(), footer);
    let x = field.x + 1 + input.visual_cursor() as u16;
    let y = field.y + 1;
    frame.set_cursor_position((x.min(field.right().saturating_sub(2)), y));
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [vertical] = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    let [horizontal] = Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);
    horizontal
}

fn identity_summary(value: &Value) -> String {
    let object = value
        .get("user")
        .and_then(Value::as_object)
        .or_else(|| value.as_object());
    let Some(object) = object else {
        return value.to_string();
    };
    object
        .iter()
        .map(|(key, value)| {
            let value = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            format!("{key:<20} {value}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn confirm_message(action: ConfirmAction, id: Option<&str>) -> String {
    match action {
        ConfirmAction::Set(resource) => format!(
            "Set current {} to {}?\n\nThis clears {} when the value changes.",
            resource.name(),
            id.unwrap_or("(missing)"),
            resource.descendants()
        ),
        ConfirmAction::Clear(resource) => format!(
            "Clear current {}?\n\nThis also clears {}.",
            resource.name(),
            resource.descendants()
        ),
        ConfirmAction::ClearAll => "Clear all saved resource context?".into(),
        ConfirmAction::Logout => "Sign out and remove local credentials?".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn test_app() -> App {
        let mut app = App::new();
        app.request_token = app.request_token.wrapping_add(1);
        app.load = LoadState::Ready;
        app.session = None;
        app
    }

    #[test]
    fn navigation_uses_explicit_screen_state() {
        let mut app = test_app();
        app.navigate(Screen::Projects);
        assert_eq!(app.screen, Screen::Projects);
        assert_eq!(app.nav_state.selected(), Some(4));
    }

    #[test]
    fn filtering_selects_first_visible_row() {
        let mut app = test_app();
        app.data = DashboardData::Organizations(vec![
            OrgRow {
                id: "o1".into(),
                name: Some("Alpha".into()),
            },
            OrgRow {
                id: "o2".into(),
                name: Some("Beta".into()),
            },
        ]);
        app.filter = "beta".into();
        app.select_first_visible();
        assert_eq!(app.list_state.selected(), Some(1));
    }

    #[test]
    fn scope_overrides_are_independent_from_saved_context() {
        let mut app = test_app();
        app.set_scope(ScopeField::Tenant, "temporary".into());
        assert_eq!(app.scopes.tenant.as_deref(), Some("temporary"));
        assert!(app.session.is_none());
    }

    #[test]
    fn confirmation_describes_context_cascade() {
        let text = confirm_message(ConfirmAction::Set(ContextResource::Tenant), Some("t1"));
        assert!(text.contains("project and environment"));
    }

    #[test]
    fn renders_normal_and_small_terminal_states() {
        for (width, height) in [(100, 30), (40, 10)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut app = test_app();
            terminal.draw(|frame| app.draw(frame)).unwrap();
        }
    }
}
