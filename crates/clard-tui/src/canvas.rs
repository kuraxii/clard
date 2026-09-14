use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
};

use clard_core::mihomo::models::{Connection, DelayHistory, Proxy as ProxyModel, ProxyType, RuleBehavior};

use crate::app::{
    APP, i18n,
    connections::{ConnUnit, ConnectionsSort, ConnectionsState},
    logs::{AuditRow, LogsState, LogsTab},
    modal::{ConfirmState, InputState},
    page::Page,
    profiles::{HistoryView, ProfileBusy, ProfilesState},
    proxy::{ProxyFocus, ProxyState},
    rules::{RulesState, RulesTab},
    settings::{GeneralRow, LogsRow, SettingsState, SettingsTab, TunRow},
};

#[derive(Debug, Default)]
pub struct Painter;

impl Painter {
    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>, app: &APP) {
        let _ = terminal.draw(|f| {
            let area = f.area();
            let theme = Theme::current();
            render_background(f, area, theme);

            if area.width < 80 || area.height < 24 {
                draw_minimum_size(f, area, theme);
                return;
            }

            let shell = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Min(0),
                        Constraint::Length(3),
                    ]
                    .as_ref(),
                )
                .split(area);

            draw_header(f, shell[0], app, theme);
            draw_navigation(f, shell[1], app, theme);

            match app.current_page {
                Page::Home => HomeLayout::draw(f, shell[2], app, theme),
                Page::Profiles => ProfilesLayout::draw(f, shell[2], &app.profiles, theme),
                Page::Proxies => ProxyLayout::draw(f, shell[2], &app.proxies, theme),
                Page::Connections => ConnectionsLayout::draw(f, shell[2], &app.connections, theme),
                Page::Logs => LogsLayout::draw(f, shell[2], &app.logs, theme),
                Page::Settings => SettingsLayout::draw(f, shell[2], &app.settings, theme),
                Page::Rules => RulesLayout::draw(f, shell[2], &app.rules, theme),
            }

            draw_footer(f, shell[3], app, theme);

            if let Some(input) = &app.input {
                draw_input_modal(f, area, input, theme);
            }
            if let Some(confirm) = &app.confirm {
                draw_confirm_modal(f, area, confirm, theme);
            }
            if let Some(history) = &app.history {
                draw_history_modal(f, area, history, theme);
            }
            if app.show_help {
                draw_help(f, area, app, theme);
            }
        });
    }
}

#[derive(Clone, Copy)]
struct Theme {
    base: Color,
    surface: Color,
    overlay: Color,
    selection: Color,
    fg: Color,
    muted: Color,
    emphasis: Color,
    primary: Color,
    secondary: Color,
    success: Color,
    warning: Color,
    error: Color,
    info: Color,
    border: Color,
}

impl Theme {
    fn current() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self {
                base: Color::Reset,
                surface: Color::Reset,
                overlay: Color::Reset,
                selection: Color::Reset,
                fg: Color::Reset,
                muted: Color::Reset,
                emphasis: Color::Reset,
                primary: Color::Reset,
                secondary: Color::Reset,
                success: Color::Reset,
                warning: Color::Reset,
                error: Color::Reset,
                info: Color::Reset,
                border: Color::Reset,
            };
        }

        Self {
            base: Color::Black,
            surface: Color::Black,
            overlay: Color::DarkGray,
            selection: Color::Blue,
            fg: Color::Gray,
            muted: Color::DarkGray,
            emphasis: Color::White,
            primary: Color::Cyan,
            secondary: Color::Magenta,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::Blue,
            border: Color::DarkGray,
        }
    }

    fn base_style(self) -> Style {
        Style::default().fg(self.fg).bg(self.base)
    }

    fn surface_style(self) -> Style {
        Style::default().fg(self.fg).bg(self.surface)
    }

    fn selected_style(self) -> Style {
        Style::default()
            .fg(self.emphasis)
            .bg(self.selection)
            .add_modifier(Modifier::BOLD)
    }

    fn title_style(self) -> Style {
        Style::default().fg(self.emphasis).add_modifier(Modifier::BOLD)
    }

    fn muted_style(self) -> Style {
        Style::default().fg(self.muted)
    }
}

fn render_background(f: &mut Frame<'_>, area: Rect, theme: Theme) {
    f.render_widget(Block::default().style(theme.base_style()), area);
}

fn panel_block<'a>(title: impl Into<Line<'a>>, focused: bool, theme: Theme) -> Block<'a> {
    let border_style = if focused {
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.border)
    };

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .title(title)
        .title_style(theme.title_style())
        .style(theme.surface_style())
}

fn draw_minimum_size(f: &mut Frame<'_>, area: Rect, theme: Theme) {
    let block = panel_block("Terminal too small", true, theme);
    let text = vec![
        Line::from(Span::styled(
            "Clard needs at least 80x24 cells.",
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("Current: {}x{}", area.width, area.height),
            theme.muted_style(),
        )),
        Line::from("Resize the terminal to continue."),
    ];

    f.render_widget(
        Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        centered_rect(56, 36, area),
    );
}

fn draw_header(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let title = Span::styled("Clard", Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD));
    let page = Span::styled(
        format!(" {} ", page_name(app)),
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    );
    let summary = Span::styled(header_summary(app), theme.muted_style());
    let line = Line::from(vec![
        Span::raw(" "),
        title,
        Span::styled("  proxy manager", theme.muted_style()),
        Span::raw("   "),
        page,
        Span::raw("  "),
        summary,
    ]);

    f.render_widget(
        Paragraph::new(line)
            .block(panel_block("Status", false, theme))
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_navigation(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let titles: Vec<String> = Page::ALL
        .iter()
        .map(|page| format!(" {} {} ", page.key(), i18n::t(app.lang, page_title_key(*page))))
        .collect();
    let tabs = Tabs::new(titles)
        .block(panel_block("Pages", false, theme))
        .select(app.current_page.index())
        .style(Style::default().fg(theme.muted))
        .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));

    f.render_widget(tabs, area);
}

fn draw_footer(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let mut spans = Vec::new();
    for (idx, (key, label)) in footer_keys(app).into_iter().enumerate() {
        if idx > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(key_span(key, theme));
        spans.push(Span::raw(label.to_string()));
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled("│", theme.muted_style()));
    spans.push(Span::raw("  "));

    let message = app.message.clone().unwrap_or_else(|| "Ready".to_string());
    spans.push(Span::styled(message, Style::default().fg(theme.info)));

    f.render_widget(
        Paragraph::new(Line::from(spans)).block(panel_block("Keys", false, theme)),
        area,
    );
}

struct HomeLayout;

impl HomeLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(area);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(cols[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(cols[1]);

        draw_home_core(f, left[0], app, theme);
        draw_home_profile(f, left[1], app, theme);
        draw_home_traffic(f, right[0], app, theme);
        draw_home_service(f, right[1], app, theme);
    }
}

fn draw_home_core(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let state_text = app.home.core_state.as_deref().unwrap_or("unknown");
    let state_color = if state_text == "running" { theme.success } else { theme.muted };
    let pid = app.home.core_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());
    let version = app.home.core_version.clone().unwrap_or_else(|| "-".to_string());
    let tun_text = if app.home.tun_active {
        "On"
    } else {
        "Off"
    };
    let tun_color = if app.home.tun_active {
        theme.success
    } else {
        theme.muted
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("State:", theme.title_style()),
            Span::raw("  "),
            Span::styled(state_text.to_string(), Style::default().fg(state_color)),
        ]),
        Line::from(vec![
            Span::styled("TUN:", theme.title_style()),
            Span::raw("  "),
            Span::styled(tun_text, Style::default().fg(tun_color)),
            Span::styled("  (clard0 / tbl 2023 / rule 9100)", theme.muted_style()),
        ]),
        kv_line("PID", &pid, theme),
        kv_line("Version", &version, theme),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Core / TUN", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_home_profile(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let (name, updated) = app
        .profiles
        .selected()
        .filter(|p| Some(p.uid.as_str()) == app.profiles.current.as_deref())
        .map(|p| {
            let updated = p.updated_at.map(format_unix_time).unwrap_or_else(|| "-".to_string());
            (p.name.clone(), updated)
        })
        .unwrap_or_else(|| ("(none)".to_string(), "-".to_string()));
    let lines = vec![
        kv_line("Profile", &name, theme),
        kv_line("Updated", &updated, theme),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Profile", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_home_traffic(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let up = app.connections.traffic.as_ref().map(|t| format_rate(t.up)).unwrap_or_else(|| "-".to_string());
    let down = app.connections.traffic.as_ref().map(|t| format_rate(t.down)).unwrap_or_else(|| "-".to_string());
    let (up_total, down_total) = app
        .connections
        .connections_data
        .as_ref()
        .map(|d| (format_network_bytes(d.upload_total), format_network_bytes(d.download_total)))
        .unwrap_or_else(|| ("-".to_string(), "-".to_string()));
    let lines = vec![
        kv_line("Upload/s", &up, theme),
        kv_line("Download/s", &down, theme),
        kv_line("Up total", &up_total, theme),
        kv_line("Down total", &down_total, theme),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Traffic", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_home_service(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let helper = app
        .home
        .helper_version
        .clone()
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| "unreachable".to_string());
    let lines = vec![
        kv_line("Helper", &helper, theme),
        Line::from(""),
        Line::from(Span::styled(
            "Use 2 Profiles to import/switch a subscription.",
            theme.muted_style(),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Service", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

struct ProfilesLayout;

impl ProfilesLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &ProfilesState, theme: Theme) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Min(0)].as_ref())
            .split(area);

        draw_profile_list(f, columns[0], state, theme);
        draw_profile_detail(f, columns[1], state, theme);
    }
}

fn draw_profile_list(f: &mut Frame<'_>, area: Rect, state: &ProfilesState, theme: Theme) {
    let items: Vec<ListItem<'_>> = state
        .items
        .iter()
        .map(|p| {
            let is_current = state.current.as_deref() == Some(p.uid.as_str());
            let updating = state.busy == ProfileBusy::Updating && state.busy_uid.as_deref() == Some(p.uid.as_str());
            let marker = if is_current { "●" } else { " " };
            let marker_style = if is_current {
                Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted)
            };
            let name_style = if is_current {
                theme.title_style()
            } else {
                Style::default().fg(theme.fg)
            };
            let status = if updating {
                Span::styled("updating…", Style::default().fg(theme.warning))
            } else if is_current {
                Span::styled("current", Style::default().fg(theme.success))
            } else {
                Span::styled("", Style::default())
            };

            ListItem::new(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::raw(" "),
                Span::styled(p.name.clone(), name_style),
                Span::raw("  "),
                status,
            ]))
        })
        .collect();

    let count = state.items.len();
    let list = List::new(items)
        .block(panel_block(format!("Profiles ({count})"), true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");

    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_profile_detail(f: &mut Frame<'_>, area: Rect, state: &ProfilesState, theme: Theme) {
    let lines = if let Some(p) = state.selected() {
        let updated = p
            .updated_at
            .map(format_unix_time)
            .unwrap_or_else(|| "-".to_string());
        let interval = if p.interval == 0 {
            "off".to_string()
        } else {
            format!("{}s", p.interval)
        };
        let traffic = if p.total > 0 {
            format!(
                "{} / {}",
                format_network_bytes(p.upload + p.download),
                format_network_bytes(p.total)
            )
        } else {
            "-".to_string()
        };
        let expire = p.expire.map(format_unix_time).unwrap_or_else(|| "-".to_string());
        vec![
            kv_line("Name", &p.name, theme),
            kv_line("UID", &p.uid, theme),
            kv_line("Type", "remote", theme),
            kv_line("URL", &p.url, theme),
            kv_line("Traffic", &traffic, theme),
            kv_line("Expire", &expire, theme),
            kv_line("Updated", &updated, theme),
            kv_line("Interval", &interval, theme),
            Line::from(""),
            Line::from(Span::styled(
                "i import · u update · d delete · r rename · [/] reorder · h history · Enter switch",
                theme.muted_style(),
            )),
        ]
    } else {
        vec![
            Line::from(Span::styled("No profiles.", theme.title_style())),
            Line::from(Span::styled("Press i to import a subscription URL.", theme.muted_style())),
        ]
    };

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Detail", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

struct LogsLayout;

impl LogsLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &LogsState, theme: Theme) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(area);

        draw_logs_tabs(f, rows[0], state, theme);
        match state.tab {
            LogsTab::Audit => draw_audit_table(f, rows[1], state, theme),
            _ => draw_log_lines(f, rows[1], state, theme),
        }
    }
}

fn draw_logs_tabs(f: &mut Frame<'_>, area: Rect, state: &LogsState, theme: Theme) {
    let selected = match state.tab {
        LogsTab::App => 0,
        LogsTab::Core => 1,
        LogsTab::Audit => 2,
    };
    let title = if state.tab == LogsTab::Core {
        format!("Logs  (level: {})", state.level_filter_label())
    } else {
        "Logs".to_string()
    };
    let titles = vec![" App ", " Core ", " Audit "];
    let tabs = Tabs::new(titles)
        .block(panel_block(title.as_str(), false, theme))
        .select(selected)
        .style(Style::default().fg(theme.muted))
        .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn draw_log_lines(f: &mut Frame<'_>, area: Rect, state: &LogsState, theme: Theme) {
    let lines = state.visible_lines();
    let items: Vec<ListItem<'_>> = lines
        .iter()
        .map(|line| {
            let style = log_level_style(&line.level, theme);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<8}", line.ts.map_or_else(String::new, format_unix_time)), theme.muted_style()),
                Span::styled(format!("[{:<5}]", line.level), style),
                Span::styled(format!("[{:<4}] ", line.source), Style::default().fg(theme.secondary)),
                Span::styled(line.message.clone(), Style::default().fg(theme.fg)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("Logs  Tab switch  f filter", true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");

    let mut list_state = state.lines_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_audit_table(f: &mut Frame<'_>, area: Rect, state: &LogsState, theme: Theme) {
    let records = state.visible_audit();
    let header = Row::new(
        ["Time", "OP", "Actor", "Result"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let rows = records.iter().map(|r| {
        let result_style = match r.result.as_str() {
            "ok" => Style::default().fg(theme.success),
            "pending" => Style::default().fg(theme.muted),
            _ => Style::default().fg(theme.error),
        };
        Row::new(vec![
            Cell::from(format_unix_time(r.ts)),
            Cell::from(r.op.clone()),
            Cell::from(format!("uid{} pid{}", r.actor.uid, r.actor.pid)),
            Cell::from(r.result.clone()).style(result_style),
        ])
    });

    let title = format!(
        "Audit  f filter  o op  I {}  Enter detail  x export{}",
        state.pair_mode_label(),
        if state.op_filter.is_empty() {
            String::new()
        } else {
            format!("  op=\"{}\"", state.op_filter)
        }
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(19),
            Constraint::Percentage(30),
            Constraint::Percentage(20),
            Constraint::Percentage(12),
        ],
    )
    .header(header)
    .block(panel_block(title.as_str(), true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut table_state = state.table_state.clone();
    f.render_stateful_widget(table, area, &mut table_state);

    // 展开详情（Enter）
    if let Some(detail) = &state.audit_detail {
        draw_audit_detail(f, area, detail, theme);
    }
}

/// 审计行展开详情弹窗：actor、intent、net 前后快照、cfg_sha256、err。
fn draw_audit_detail(f: &mut Frame<'_>, area: Rect, detail: &AuditRow, theme: Theme) {
    let popup = centered_rect(72, 55, area);
    f.render_widget(Clear, popup);
    let mut lines = vec![
        Line::from(Span::styled(format!("{}  ({})", detail.op, detail.op_id), theme.title_style())),
        Line::from(""),
        kv_line("Result", &detail.result, theme),
        kv_line(
            "Actor",
            &format!("uid{} pid{}", detail.actor.uid, detail.actor.pid),
            theme,
        ),
        kv_line("Intent", &detail.intent, theme),
    ];
    if let Some(e) = &detail.err {
        lines.push(kv_line("Error", e, theme));
    }
    if let Some(cfg) = &detail.cfg_sha256 {
        let short = if cfg.len() > 24 { &cfg[..24] } else { cfg };
        lines.push(kv_line("cfg_sha256", short, theme));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("Net before", theme.title_style())));
    lines.push(Line::from(
        detail.net_before.clone().unwrap_or_else(|| "-".to_string()),
    ));
    lines.push(Line::from(Span::styled("Net after", theme.title_style())));
    lines.push(Line::from(
        detail.net_after.clone().unwrap_or_else(|| "-".to_string()),
    ));
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Audit detail  Enter/Esc close", true, theme))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn log_level_style(level: &str, theme: Theme) -> Style {
    match level.to_lowercase().as_str() {
        "error" | "err" => Style::default().fg(theme.error),
        "warn" | "warning" => Style::default().fg(theme.warning),
        "debug" => Style::default().fg(theme.muted),
        _ => Style::default().fg(theme.info),
    }
}

struct SettingsLayout;

impl SettingsLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(area);

        draw_settings_tabs(f, rows[0], state, theme);
        match state.tab {
            SettingsTab::General => draw_settings_general(f, rows[1], state, theme),
            SettingsTab::Tun => draw_settings_tun(f, rows[1], state, theme),
            SettingsTab::Core => draw_settings_core(f, rows[1], state, theme),
            SettingsTab::Service => draw_settings_service(f, rows[1], state, theme),
            SettingsTab::Backup => draw_settings_backup(f, rows[1], state, theme),
            SettingsTab::Logs => draw_settings_logs(f, rows[1], state, theme),
            SettingsTab::About => draw_settings_about(f, rows[1], theme),
        }
    }
}

fn draw_settings_tabs(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let selected = match state.tab {
        SettingsTab::General => 0,
        SettingsTab::Tun => 1,
        SettingsTab::Core => 2,
        SettingsTab::Service => 3,
        SettingsTab::Backup => 4,
        SettingsTab::Logs => 5,
        SettingsTab::About => 6,
    };
    let titles = vec![" General ", " TUN ", " Core ", " Service ", " Backup ", " Logs ", " About "];
    let tabs = Tabs::new(titles)
        .block(panel_block("Settings", false, theme))
        .select(selected)
        .style(Style::default().fg(theme.muted))
        .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn draw_settings_general(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let settings = state.settings.as_ref();
    let items: Vec<ListItem<'_>> = GeneralRow::ALL
        .iter()
        .map(|row| {
            let value = match row {
                GeneralRow::MixedPort => settings
                    .map(|s| s.mixed_port.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                GeneralRow::AutoUpdateHours => settings
                    .map(|s| s.auto_update_interval_hours.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                GeneralRow::Language => settings
                    .map(|s| s.language.clone())
                    .unwrap_or_else(|| "en".to_string()),
                GeneralRow::Theme => settings
                    .map(|s| s.theme.clone())
                    .unwrap_or_else(|| "dark".to_string()),
                GeneralRow::TestUrl => settings
                    .map(|s| {
                        if s.test_url.is_empty() {
                            "default".to_string()
                        } else {
                            s.test_url.clone()
                        }
                    })
                    .unwrap_or_else(|| "default".to_string()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}", row.label()), Style::default().fg(theme.fg)),
                Span::styled(value, Style::default().fg(theme.primary)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("General  Enter edit", true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");
    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_settings_tun(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let settings = state.settings.as_ref();
    let items: Vec<ListItem<'_>> = TunRow::ALL
        .iter()
        .map(|row| {
            let value = match row {
                TunRow::TunEnabled => {
                    if settings.map(|s| s.tun_enabled).unwrap_or(false) {
                        "● on".to_string()
                    } else {
                        "○ off".to_string()
                    }
                }
                TunRow::TunStack => settings
                    .map(|s| {
                        if s.tun_stack.is_empty() {
                            "[system]".to_string()
                        } else {
                            format!("[{}]", s.tun_stack)
                        }
                    })
                    .unwrap_or_else(|| "[system]".to_string()),
                TunRow::TunDnsMode => settings
                    .map(|s| {
                        if s.tun_dns_mode.is_empty() {
                            "[fake-ip]".to_string()
                        } else {
                            format!("[{}]", s.tun_dns_mode)
                        }
                    })
                    .unwrap_or_else(|| "[fake-ip]".to_string()),
                TunRow::DnsHijack => {
                    let v = settings.map(|s| s.dns_hijack.join(",")).unwrap_or_default();
                    if v.is_empty() {
                        "default (any:53,tcp://any:53)".to_string()
                    } else {
                        v
                    }
                }
                TunRow::RouteExclude => {
                    let v = settings
                        .map(|s| s.route_exclude_address.join(","))
                        .unwrap_or_default();
                    if v.is_empty() {
                        "default private nets (10/8,172.16/12,192.168/16,…)".to_string()
                    } else {
                        v
                    }
                }
                TunRow::ExcludeUid => settings
                    .map(|s| {
                        if s.exclude_uid.is_empty() {
                            "(empty)".to_string()
                        } else {
                            s.exclude_uid.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
                        }
                    })
                    .unwrap_or_else(|| "(empty)".to_string()),
                TunRow::ExcludeInterface => settings
                    .map(|s| {
                        if s.exclude_interface.is_empty() {
                            "(empty)".to_string()
                        } else {
                            s.exclude_interface.join(",")
                        }
                    })
                    .unwrap_or_else(|| "(empty)".to_string()),
                TunRow::ExcludeDstPort => settings
                    .map(|s| {
                        if s.exclude_dst_port.is_empty() {
                            "(empty)".to_string()
                        } else {
                            s.exclude_dst_port.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
                        }
                    })
                    .unwrap_or_else(|| "(empty)".to_string()),
                TunRow::StrictRoute => format!(
                    "{} risk: crash residuals = full outage",
                    if settings.map(|s| s.strict_route).unwrap_or(false) {
                        "● on"
                    } else {
                        "○ off"
                    }
                ),
                TunRow::AutoRedirect => format!(
                    "{} risk: nft/iptables residuals",
                    if settings.map(|s| s.auto_redirect).unwrap_or(false) {
                        "● on"
                    } else {
                        "○ off"
                    }
                ),
                TunRow::RecoverDirect => "run cleanup-tun".to_string(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<22}", row.label()), Style::default().fg(theme.fg)),
                Span::styled(value, Style::default().fg(theme.primary)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("TUN  Enter edit / confirm hot-reload", true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");
    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_settings_logs(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let cfg = state.helper_config.as_ref();
    let items: Vec<ListItem<'_>> = LogsRow::ALL
        .iter()
        .map(|row| {
            let value = match row {
                LogsRow::CoreLogLevel => cfg
                    .map(|c| c.log_level.clone())
                    .unwrap_or_else(|| "info".to_string()),
                LogsRow::AppLogMaxBytes => cfg
                    .map(|c| (c.app_log_max_bytes / 1024 / 1024).to_string())
                    .unwrap_or_else(|| "1".to_string()),
                LogsRow::AppLogKeep => cfg.map(|c| c.app_log_keep.to_string()).unwrap_or_else(|| "5".to_string()),
                LogsRow::AuditKeep => cfg.map(|c| c.audit_keep.to_string()).unwrap_or_else(|| "5".to_string()),
                LogsRow::AuditDualWrite => cfg
                    .map(|c| if c.audit_dual_write { "on" } else { "off" }.to_string())
                    .unwrap_or_else(|| "on".to_string()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<20}", row.label()), Style::default().fg(theme.fg)),
                Span::styled(value, Style::default().fg(theme.primary)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel_block("Logs / Audit  Enter shows sudo edit hint", true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");
    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);

    let hint = Paragraph::new(Line::from(vec![Span::styled(
        "These live in /etc/clard/helper.toml (root). Edit with sudo, then: sudo systemctl restart clard-helper",
        theme.muted_style(),
    )]));
    let h = area.height.min(3);
    let hint_area = Rect::new(area.x, area.y + area.height - h, area.width, h);
    f.render_widget(hint, hint_area);
}

fn draw_settings_core(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let state_text = state.core_state.as_deref().unwrap_or("unknown");
    let state_color = match state_text {
        "running" => theme.success,
        _ => theme.muted,
    };
    let pid = state.core_pid.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string());
    let version = state.core_version.clone().unwrap_or_else(|| "-".to_string());
    let sha = state
        .core_sha256
        .clone()
        .map(|s| {
            if s.len() > 16 {
                format!("{}…", &s[..16])
            } else {
                s
            }
        })
        .unwrap_or_else(|| "-".to_string());
    let state_span = Span::styled(state_text.to_string(), Style::default().fg(state_color));
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("{:<10}", "State"), Style::default().fg(theme.primary)),
            state_span,
        ]),
        kv_line("PID", &pid, theme),
        kv_line("Version", &version, theme),
        kv_line("SHA256", &sha, theme),
        Line::from(""),
    ];
    lines.push(Line::from(vec![
        key_span("s", theme),
        Span::raw(" start  "),
        key_span("S", theme),
        Span::raw(" stop (confirm)  "),
        key_span("r", theme),
        Span::raw(" restart"),
    ]));
    lines.push(Line::from(vec![
        key_span("c", theme),
        Span::raw(" check  "),
        key_span("i", theme),
        Span::raw(" upgrade (GitHub latest)"),
    ]));
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Core", true, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_settings_service(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let lines = vec![
        kv_line(
            "Helper",
            &format!("v{}", state.helper_version.clone().unwrap_or_else(|| "-".to_string())),
            theme,
        ),
        Line::from(""),
        Line::from(Span::styled("Install (one-time, root):", theme.title_style())),
        Line::from(Span::styled(
            "  sudo dnf install ./clard-*.rpm",
            Style::default().fg(theme.primary),
        )),
        Line::from(Span::styled("Uninstall (keeps /var/clard data):", theme.title_style())),
        Line::from(Span::styled(
            "  sudo dnf remove clard",
            Style::default().fg(theme.primary),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Run these outside the TUI, then return.",
            theme.muted_style(),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Service", true, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_settings_backup(f: &mut Frame<'_>, area: Rect, state: &SettingsState, theme: Theme) {
    let header = Row::new(
        ["Name", "Created", "Size"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let rows = state.backups.iter().map(|b| {
        Row::new(vec![
            Cell::from(b.name.clone()),
            Cell::from(format_unix_time(b.created_at)),
            Cell::from(format_network_bytes(b.size)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Length(19),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(panel_block("Backup  b create  Enter restore  d delete", true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut table_state = state.backups_state.clone();
    f.render_stateful_widget(table, area, &mut table_state);
}

fn draw_settings_about(f: &mut Frame<'_>, area: Rect, theme: Theme) {
    let paths = [
        ("/run/clard", "runtime sockets"),
        ("/var/clard/lib", "persistent data"),
        ("/var/clard/cache", "downloads / assets"),
        ("/var/clard/log", "audit and logs"),
        ("/var/clard/bin", "mihomo core binary"),
        ("/var/clard/backups", "local backups"),
        ("/etc/clard", "helper config"),
    ];
    let mut lines = vec![kv_line("Clard", env!("CARGO_PKG_VERSION"), theme), Line::from("")];
    for (p, desc) in paths {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<22}", p), Style::default().fg(theme.primary)),
            Span::styled(desc, theme.muted_style()),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("About", true, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

struct RulesLayout;

impl RulesLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &RulesState, theme: Theme) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(area);

        draw_rules_tabs(f, rows[0], state, theme);
        match state.tab {
            RulesTab::Rules => draw_rules_table(f, rows[1], state, theme),
            RulesTab::Providers => draw_rule_providers_table(f, rows[1], state, theme),
        }
    }
}

fn draw_rules_tabs(f: &mut Frame<'_>, area: Rect, state: &RulesState, theme: Theme) {
    let selected = match state.tab {
        RulesTab::Rules => 0,
        RulesTab::Providers => 1,
    };
    let titles = vec![" Rules ", " Rule Providers "];
    let tabs = Tabs::new(titles)
        .block(panel_block("Rules", false, theme))
        .select(selected)
        .style(Style::default().fg(theme.muted))
        .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn draw_rules_table(f: &mut Frame<'_>, area: Rect, state: &RulesState, theme: Theme) {
    let header = Row::new(
        ["#", "Type", "Payload", "Proxy", "Hits", "State"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let rows = state.rules.iter().map(|rule| {
        let hits = rule
            .extra
            .as_ref()
            .map(|e| e.hit_count.to_string())
            .unwrap_or_else(|| "-".to_string());
        let (state_text, state_style) = match rule.extra.as_ref().map(|e| e.disabled) {
            Some(true) => ("disabled", Style::default().fg(theme.muted)),
            Some(false) => ("enabled", Style::default().fg(theme.success)),
            None => ("-", theme.muted_style()),
        };
        Row::new(vec![
            Cell::from(rule.index.to_string()),
            Cell::from(rule.rule_type.clone()),
            Cell::from(rule.payload.clone()),
            Cell::from(rule.proxy.clone()),
            Cell::from(hits),
            Cell::from(state_text).style(state_style),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(16),
            Constraint::Percentage(30),
            Constraint::Percentage(16),
            Constraint::Length(8),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(panel_block("Rules  Enter toggle  f filter", true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut table_state = state.rules_state.clone();
    f.render_stateful_widget(table, area, &mut table_state);
}

fn draw_rule_providers_table(f: &mut Frame<'_>, area: Rect, state: &RulesState, theme: Theme) {
    let header = Row::new(
        ["Name", "Behavior", "Entries", "Updated"]
            .into_iter()
            .map(|h| Cell::from(h).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let rows = state.providers.iter().map(|(name, provider)| {
        Row::new(vec![
            Cell::from(name.clone()),
            Cell::from(rule_behavior_name(&provider.behavior)),
            Cell::from(provider.rule_count.to_string()),
            Cell::from(empty_as_dash(&provider.updated_at).to_string()),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(34),
            Constraint::Percentage(20),
            Constraint::Length(8),
            Constraint::Percentage(38),
        ],
    )
    .header(header)
    .block(panel_block("Rule Providers  u update", true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut table_state = state.providers_state.clone();
    f.render_stateful_widget(table, area, &mut table_state);
}

fn rule_behavior_name(behavior: &RuleBehavior) -> &'static str {
    match behavior {
        RuleBehavior::Domain => "Domain",
        RuleBehavior::IpCidr => "IP-CIDR",
        RuleBehavior::Classical => "Classical",
    }
}

struct ProxyLayout;

impl ProxyLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &ProxyState, theme: Theme) {
        if area.width >= 118 {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(
                    [
                        Constraint::Percentage(30),
                        Constraint::Percentage(45),
                        Constraint::Percentage(25),
                    ]
                    .as_ref(),
                )
                .split(area);

            draw_group_list(f, chunks[0], state, theme);
            draw_proxy_list(f, chunks[1], state, theme);
            draw_proxy_detail(f, chunks[2], state, theme);
            return;
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(8)].as_ref())
            .split(area);
        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)].as_ref())
            .split(rows[0]);

        draw_group_list(f, top[0], state, theme);
        draw_proxy_list(f, top[1], state, theme);
        draw_proxy_detail(f, rows[1], state, theme);
    }
}

fn draw_group_list(f: &mut Frame<'_>, area: Rect, state: &ProxyState, theme: Theme) {
    let items: Vec<ListItem<'_>> = state
        .groups
        .iter()
        .map(|group| {
            let node_count = group.all.as_ref().map_or(0, Vec::len);
            let dot = if group.alive { "●" } else { "○" };
            let dot_style = if group.alive {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.error)
            };
            let now = group.now.as_deref().unwrap_or("-");
            ListItem::new(Line::from(vec![
                Span::styled(dot, dot_style),
                Span::raw(" "),
                Span::styled(group.name.clone(), theme.title_style()),
                Span::styled(format!("  {}", proxy_type_name(group)), theme.muted_style()),
                Span::styled(format!("  {} nodes", node_count), theme.muted_style()),
                Span::styled(format!("  → {}", now), Style::default().fg(theme.info)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel_block(
            "Groups  ←/→ or Tab focus",
            state.focus == ProxyFocus::Groups,
            theme,
        ))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");

    let mut list_state = state.group_list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_proxy_list(f: &mut Frame<'_>, area: Rect, state: &ProxyState, theme: Theme) {
    let mut items = Vec::new();
    if let Some(group) = selected_group(state) {
        if let Some(all) = &group.all {
            for node in all {
                let is_now = group.now.as_deref() == Some(node);
                let extra = group.extra.get(node);
                let alive = extra.map_or(group.alive, |extra| extra.alive);
                let delay = extra.and_then(|extra| latest_delay(&extra.history));
                let dot = if is_now {
                    "◉"
                } else if alive {
                    "●"
                } else {
                    "○"
                };
                let dot_style = if is_now {
                    Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
                } else if alive {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.error)
                };
                let state_text = if is_now {
                    "ACTIVE"
                } else if alive {
                    "ready"
                } else {
                    "down"
                };
                let delay_text = delay.map_or_else(|| "-".to_string(), format_delay);
                let spark = extra.map(|extra| sparkline_history(&extra.history)).unwrap_or_default();

                items.push(ListItem::new(Line::from(vec![
                    Span::styled(dot, dot_style),
                    Span::raw(" "),
                    Span::styled(
                        node.clone(),
                        if is_now {
                            theme.title_style().fg(theme.emphasis)
                        } else {
                            Style::default().fg(theme.fg)
                        },
                    ),
                    Span::styled(format!("  {:>6}", delay_text), delay_style(delay, theme)),
                    Span::styled(format!("  {:<6}", state_text), theme.muted_style()),
                    Span::styled(format!("  {}", spark), Style::default().fg(theme.secondary)),
                ])));
            }
        }
    }

    let list = List::new(items)
        .block(panel_block(
            "Nodes  Enter select  t test  T all  d clear",
            state.focus == ProxyFocus::Proxies,
            theme,
        ))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");

    let mut list_state = state.proxy_list_state.clone();
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_proxy_detail(f: &mut Frame<'_>, area: Rect, state: &ProxyState, theme: Theme) {
    let lines = if let Some(group) = selected_group(state) {
        let selected_node = selected_node(state).unwrap_or("-");
        let history = selected_node_history(group, selected_node);
        let delay = history.and_then(latest_delay);
        let all_count = group.all.as_ref().map_or(0, Vec::len);
        let fixed = group.fixed.as_deref().unwrap_or("-");
        let provider = group.provider_name.as_deref().unwrap_or("-");
        let test_url = group.test_url.as_deref().unwrap_or("-");
        vec![
            kv_line("Group", &group.name, theme),
            kv_line("Current", group.now.as_deref().unwrap_or("-"), theme),
            kv_line("Selected", selected_node, theme),
            kv_line("Type", proxy_type_name(group), theme),
            kv_line("Nodes", &all_count.to_string(), theme),
            kv_line("Delay", &delay.map_or_else(|| "-".to_string(), format_delay), theme),
            kv_line("History", &history.map_or_else(String::new, sparkline_history), theme),
            kv_line("Flags", &proxy_flags(group), theme),
            kv_line("Fixed", fixed, theme),
            kv_line("Provider", provider, theme),
            kv_line("Test URL", test_url, theme),
            kv_line("Interface", empty_as_dash(&group.interface), theme),
        ]
    } else {
        vec![
            Line::from(Span::styled("No proxy groups loaded.", theme.title_style())),
            Line::from(Span::styled(
                "Enter the Proxy workspace to fetch groups from the backend.",
                theme.muted_style(),
            )),
        ]
    };

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Selection Detail", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

struct ConnectionsLayout;

impl ConnectionsLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &ConnectionsState, theme: Theme) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(5), Constraint::Min(0)].as_ref())
            .split(area);
        draw_connection_metrics(f, rows[0], state, theme);

        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)].as_ref())
            .split(rows[1]);

        draw_connections_table(f, columns[0], state, theme);
        draw_connection_detail(f, columns[1], state, theme);
    }
}

fn draw_connection_metrics(f: &mut Frame<'_>, area: Rect, state: &ConnectionsState, theme: Theme) {
    let data = state.connections_data.as_ref();
    let active = state.connections.len().to_string();
    let upload_total = data
        .map(|data| format_network_bytes(data.upload_total))
        .unwrap_or_else(|| "-".to_string());
    let download_total = data
        .map(|data| format_network_bytes(data.download_total))
        .unwrap_or_else(|| "-".to_string());
    let memory = data
        .map(|data| format_network_bytes(u64::from(data.memory)))
        .unwrap_or_else(|| "-".to_string());
    let upload_rate = state
        .traffic
        .as_ref()
        .map(|traffic| format_rate(traffic.up))
        .unwrap_or_else(|| "-".to_string());
    let download_rate = state
        .traffic
        .as_ref()
        .map(|traffic| format_rate(traffic.down))
        .unwrap_or_else(|| "-".to_string());
    let upload_caption = format!("total {}  {}", upload_total, sparkline_u64(&state.upload_history));
    let download_caption = format!("total {}  {}", download_total, sparkline_u64(&state.download_history));

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ]
            .as_ref(),
        )
        .split(area);

    draw_metric(
        f,
        chunks[0],
        "Active",
        &active,
        "live connections",
        theme.success,
        theme,
    );
    draw_metric(
        f,
        chunks[1],
        "Upload / s",
        &upload_rate,
        &upload_caption,
        theme.warning,
        theme,
    );
    draw_metric(
        f,
        chunks[2],
        "Download / s",
        &download_rate,
        &download_caption,
        theme.primary,
        theme,
    );
    draw_metric(f, chunks[3], "Memory", &memory, "backend usage", theme.secondary, theme);
}

fn draw_connections_table(f: &mut Frame<'_>, area: Rect, state: &ConnectionsState, theme: Theme) {
    let upload_header = if state.sort == ConnectionsSort::Upload {
        "Upload ↓"
    } else {
        "Upload"
    };
    let download_header = if state.sort == ConnectionsSort::Download {
        "Download ↓"
    } else {
        "Download"
    };

    let header = Row::new(
        [
            "Host",
            "Process",
            "Net",
            upload_header,
            download_header,
            "Rule",
            "Chain",
        ]
        .into_iter()
        .map(|header| Cell::from(header).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let format_bytes = |bytes: u64| match state.unit {
        ConnUnit::Auto => format_network_bytes(bytes),
        ConnUnit::Kb => format!("{} KB", bytes / 1024),
    };
    let rows = state.connections.iter().map(|connection| {
        let host = connection_host(connection);
        let process = empty_as_dash(&connection.metadata.process);
        let chain = compact_chain(&connection.chains);
        Row::new(vec![
            Cell::from(host),
            Cell::from(process.to_string()),
            Cell::from(network_name(connection)),
            Cell::from(format_bytes(connection.upload)),
            Cell::from(format_bytes(connection.download)),
            Cell::from(empty_as_dash(&connection.rule).to_string()),
            Cell::from(chain),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(28),
            Constraint::Percentage(14),
            Constraint::Length(5),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(14),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(panel_block("Connections  u/d sort  x close  X all  f filter", true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(table, area, &mut list_state);
}

fn draw_connection_detail(f: &mut Frame<'_>, area: Rect, state: &ConnectionsState, theme: Theme) {
    let lines = if let Some(connection) = selected_connection(state) {
        vec![
            kv_line("Host", &connection_host(connection), theme),
            kv_line("ID", &connection.id, theme),
            kv_line("Type", &format!("{:?}", connection.metadata.connection_type), theme),
            kv_line("Network", &network_name(connection), theme),
            kv_line("Source", &source_endpoint(connection), theme),
            kv_line("Target", &destination_endpoint(connection), theme),
            kv_line("Geo", &geo_line(connection), theme),
            kv_line("ASN", &asn_line(connection), theme),
            kv_line("DNS", &format!("{:?}", connection.metadata.dns_mode), theme),
            kv_line("Inbound", &inbound_line(connection), theme),
            kv_line("Rule", &rule_line(connection), theme),
            kv_line("Chains", &connection.chains.join(" → "), theme),
            kv_line("Started", empty_as_dash(&connection.start), theme),
        ]
    } else {
        vec![
            Line::from(Span::styled("No active connection selected.", theme.title_style())),
            Line::from(Span::styled(
                "This page refreshes every second while visible.",
                theme.muted_style(),
            )),
        ]
    };

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Connection Detail", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_input_modal(f: &mut Frame<'_>, area: Rect, input: &InputState, theme: Theme) {
    let popup = centered_rect(70, 24, area);
    f.render_widget(Clear, popup);

    let mut display = input.buffer.clone();
    display.insert(input.cursor, '▏');
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(display, theme.title_style())),
        Line::from(""),
        Line::from(vec![
            key_span("Enter", theme),
            Span::raw(" submit  "),
            key_span("Esc", theme),
            Span::raw(" cancel  "),
            key_span("Ctrl+u", theme),
            Span::raw(" clear"),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block(input.title.as_str(), true, theme))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn draw_history_modal(f: &mut Frame<'_>, area: Rect, history: &HistoryView, theme: Theme) {
    let popup = centered_rect(56, 60, area);
    f.render_widget(Clear, popup);

    let items: Vec<ListItem<'_>> = history
        .versions
        .iter()
        .map(|v| {
            let updated = v.updated_at.map(format_unix_time).unwrap_or_else(|| "-".to_string());
            ListItem::new(Line::from(vec![
                Span::styled(format!("v{}", v.version), theme.title_style()),
                Span::raw("  "),
                Span::styled(updated, theme.muted_style()),
            ]))
        })
        .collect();

    let hint = if items.is_empty() {
        vec![Line::from(Span::styled("(no history)", theme.muted_style()))]
    } else {
        Vec::new()
    };

    let list = List::new(items)
        .block(panel_block("History  Enter restore  Esc close", true, theme))
        .highlight_style(theme.selected_style())
        .highlight_symbol("▸ ");

    let mut list_state = history.list_state.clone();
    f.render_stateful_widget(list, popup, &mut list_state);

    if !hint.is_empty() {
        let hint_area = popup.inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
        f.render_widget(Paragraph::new(hint), hint_area);
    }
}

fn draw_confirm_modal(f: &mut Frame<'_>, area: Rect, confirm: &ConfirmState, theme: Theme) {
    let popup = centered_rect(60, 26, area);
    f.render_widget(Clear, popup);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            confirm.message.clone(),
            Style::default().fg(theme.warning),
        )),
        Line::from(""),
        Line::from(vec![
            key_span("Enter", theme),
            Span::raw(" confirm  "),
            key_span("Esc", theme),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block(confirm.title.as_str(), true, theme))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn draw_help(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let popup = centered_rect(68, 70, area);
    f.render_widget(Clear, popup);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Global", theme.title_style()),
            Span::raw("  "),
            key_span("q", theme),
            Span::raw("quit  "),
            key_span("Esc", theme),
            Span::raw("back/home  "),
            key_span("?", theme),
            Span::raw("help  "),
            key_span("1-7", theme),
            Span::raw("jump"),
        ]),
        Line::from(vec![
            Span::styled("Move", theme.title_style()),
            Span::raw("    arrows or "),
            key_span("h/j/k/l", theme),
            Span::raw("  "),
            key_span("Tab", theme),
            Span::raw("focus"),
        ]),
        Line::from(""),
    ];

    lines.extend(match app.current_page {
        Page::Home => vec![
            Line::from(Span::styled("Home", theme.title_style())),
            Line::from("Overview of helper, core, current profile and traffic."),
            Line::from("2 opens Profiles to import/switch a subscription."),
        ],
        Page::Profiles => vec![
            Line::from(Span::styled("Profiles", theme.title_style())),
            Line::from("i imports a subscription URL; Enter switches current."),
            Line::from("u updates the selected profile; d deletes it (confirm)."),
        ],
        Page::Proxies => vec![
            Line::from(Span::styled("Proxies", theme.title_style())),
            Line::from("Left/right or Tab changes focus between groups and nodes."),
            Line::from("Enter selects the highlighted node; t tests its delay."),
            Line::from("T tests every group; d clears the group's fixed selection."),
        ],
        Page::Connections => vec![
            Line::from(Span::styled("Connections", theme.title_style())),
            Line::from("u sorts by upload, d sorts by download, x closes the selected connection."),
            Line::from("X closes all (confirm); f filters by host/rule/process."),
            Line::from("The list refreshes automatically every second while visible."),
        ],
        Page::Logs => vec![
            Line::from(Span::styled("Logs", theme.title_style())),
            Line::from("Tab switches between App / Core / Audit columns."),
            Line::from("f filters by keyword; e cycles core level (all/info/warn/error/debug)."),
            Line::from("Audit rows show op/actor/result."),
        ],
        Page::Settings => vec![
            Line::from(Span::styled("Settings", theme.title_style())),
            Line::from("Tab switches General / TUN / Core / Service / Backup / About."),
            Line::from("General: Enter edits port/interval, toggles language/theme."),
            Line::from("TUN: Enter toggles TUN (hot reload + read-back verify), stack cycles,"),
            Line::from("     list fields open editors; strict-route/auto-redirect need confirm."),
            Line::from("     Recover direct runs cleanup-tun (fail-open, idempotent)."),
            Line::from("Core: s start, S stop, r restart. Backup: b create, d delete."),
        ],
        Page::Rules => vec![
            Line::from(Span::styled("Rules", theme.title_style())),
            Line::from("Enter enables/disables the selected rule (hot apply)."),
            Line::from("f filters by type/payload/proxy; Tab shows rule providers."),
            Line::from("In providers view u updates the selected provider."),
        ],
    });

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "All visible actions are also summarized in the footer.",
        theme.muted_style(),
    )));

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Help", true, theme))
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn draw_metric(f: &mut Frame<'_>, area: Rect, title: &str, value: &str, caption: &str, color: Color, theme: Theme) {
    let lines = vec![
        Line::from(Span::styled(
            value.to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(caption.to_string(), theme.muted_style())),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block(title, false, theme))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn kv_line(label: &str, value: &str, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<10}", label), Style::default().fg(theme.primary)),
        Span::styled(value.to_string(), Style::default().fg(theme.fg)),
    ])
}

fn key_span(key: &str, theme: Theme) -> Span<'static> {
    Span::styled(
        format!("[{}]", key),
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    )
}

fn page_name(app: &APP) -> &str {
    i18n::t(app.lang, page_title_key(app.current_page))
}

fn page_title_key(page: Page) -> &'static str {
    match page {
        Page::Home => "page.home",
        Page::Profiles => "page.profiles",
        Page::Proxies => "page.proxies",
        Page::Connections => "page.connections",
        Page::Logs => "page.logs",
        Page::Settings => "page.settings",
        Page::Rules => "page.rules",
    }
}

fn header_summary(app: &APP) -> String {
    match app.current_page {
        Page::Home => {
            let state = app.home.core_state.as_deref().unwrap_or("unknown");
            format!("core {state} · helper {}", app.home.helper_version.as_deref().unwrap_or("-"))
        }
        Page::Profiles => "subscription profiles".to_string(),
        Page::Proxies => {
            let groups = app.proxies.groups.len();
            let nodes: usize = app
                .proxies
                .groups
                .iter()
                .filter_map(|group| group.all.as_ref())
                .map(Vec::len)
                .sum();
            format!("{groups} groups · {nodes} selectable nodes")
        }
        Page::Connections => {
            if let Some(data) = &app.connections.connections_data {
                format!(
                    "{} active · up {} · down {} · mem {}",
                    app.connections.connections.len(),
                    format_network_bytes(data.upload_total),
                    format_network_bytes(data.download_total),
                    format_network_bytes(u64::from(data.memory))
                )
            } else {
                "loading connections".to_string()
            }
        }
        Page::Logs => "app · core · audit".to_string(),
        Page::Settings => "general · tun · core · service · backup".to_string(),
        Page::Rules => {
            format!("{} rules · {} providers", app.rules.rules.len(), app.rules.providers.len())
        }
    }
}

fn footer_keys(app: &APP) -> Vec<(&'static str, &'static str)> {
    let mut keys = Vec::new();
    match app.current_page {
        Page::Home => {
            keys.push(("1-7", "page"));
            keys.push(("2", "profiles"));
        }
        Page::Profiles => {
            keys.push(("↑↓/jk", "move"));
            keys.push(("i/u", "import/upd"));
            keys.push(("d/r", "del/rename"));
            keys.push(("[/]", "reorder"));
            keys.push(("h", "history"));
            keys.push(("Enter", "switch"));
        }
        Page::Proxies => {
            let focus = if app.proxies.focus == ProxyFocus::Groups {
                "nodes focus"
            } else {
                "groups focus"
            };
            keys.push(("↑↓/jk", "move"));
            keys.push(("Tab/←→", focus));
            keys.push(("Enter", "select"));
            keys.push(("t/T", "test"));
            keys.push(("d", "clear"));
            keys.push(("f/s", "filter/sort"));
        }
        Page::Connections => {
            keys.push(("↑↓/jk", "move"));
            keys.push(("u/d", "sort"));
            keys.push(("x/X", "close"));
            keys.push(("f", "filter"));
            keys.push(("c", "unit"));
        }
        Page::Logs => {
            keys.push(("↑↓/jk", "move"));
            keys.push(("Tab", "app/core/audit"));
            keys.push(("f", "filter"));
        }
        Page::Settings => {
            keys.push(("Tab/←→", "tab"));
            keys.push(("↑↓/jk", "move"));
            keys.push(("Enter", "edit"));
        }
        Page::Rules => {
            keys.push(("↑↓/jk", "move"));
            keys.push(("Tab", "providers"));
            keys.push(("Enter", "toggle"));
            keys.push(("f", "filter"));
        }
    }

    keys.push(("?", "help"));
    keys.push(("Esc", "home"));
    keys.push(("q", "quit"));
    keys
}

fn selected_group(state: &ProxyState) -> Option<&ProxyModel> {
    state.group_list_state.selected().and_then(|idx| state.groups.get(idx))
}

fn selected_node(state: &ProxyState) -> Option<&str> {
    let group = selected_group(state)?;
    let proxy_idx = state.proxy_list_state.selected()?;
    group.all.as_ref()?.get(proxy_idx).map(String::as_str)
}

fn selected_node_history<'a>(group: &'a ProxyModel, node: &str) -> Option<&'a [DelayHistory]> {
    group.extra.get(node).map(|extra| extra.history.as_slice())
}

fn latest_delay(history: &[DelayHistory]) -> Option<u16> {
    history.last().map(|entry| entry.delay)
}

fn proxy_type_name(group: &ProxyModel) -> &'static str {
    match group.proxy_type {
        ProxyType::Direct => "Direct",
        ProxyType::Reject => "Reject",
        ProxyType::RejectDrop => "RejectDrop",
        ProxyType::Compatible => "Compatible",
        ProxyType::Pass => "Pass",
        ProxyType::Dns => "DNS",
        ProxyType::Shadowsocks => "SS",
        ProxyType::ShadowsocksR => "SSR",
        ProxyType::Snell => "Snell",
        ProxyType::Socks5 => "Socks5",
        ProxyType::Http => "HTTP",
        ProxyType::Vmess => "Vmess",
        ProxyType::Vless => "Vless",
        ProxyType::Trojan => "Trojan",
        ProxyType::Hysteria => "Hysteria",
        ProxyType::Hysteria2 => "Hysteria2",
        ProxyType::WireGuard => "WireGuard",
        ProxyType::Tuic => "Tuic",
        ProxyType::Ssh => "SSH",
        ProxyType::Mieru => "Mieru",
        ProxyType::AnyTLS => "AnyTLS",
        ProxyType::Relay => "Relay",
        ProxyType::Selector => "Selector",
        ProxyType::Fallback => "Fallback",
        ProxyType::URLTest => "URLTest",
        ProxyType::LoadBalance => "LoadBalance",
    }
}

fn proxy_flags(group: &ProxyModel) -> String {
    let mut flags = Vec::new();
    if group.udp {
        flags.push("udp");
    }
    if group.uot {
        flags.push("uot");
    }
    if group.xudp {
        flags.push("xudp");
    }
    if group.tfo {
        flags.push("tfo");
    }
    if group.mptcp {
        flags.push("mptcp");
    }
    if group.smux {
        flags.push("smux");
    }
    if flags.is_empty() {
        "-".to_string()
    } else {
        flags.join(" · ")
    }
}

fn selected_connection(state: &ConnectionsState) -> Option<&Connection> {
    state.list_state.selected().and_then(|idx| state.connections.get(idx))
}

fn connection_host(connection: &Connection) -> String {
    if !connection.metadata.host.is_empty() {
        connection.metadata.host.clone()
    } else if !connection.metadata.sniff_host.is_empty() {
        connection.metadata.sniff_host.clone()
    } else if !connection.metadata.remote_destination.is_empty() {
        connection.metadata.remote_destination.clone()
    } else {
        connection.metadata.destination_ip.clone()
    }
}

fn source_endpoint(connection: &Connection) -> String {
    format!(
        "{}:{}",
        empty_as_dash(&connection.metadata.source_ip),
        empty_as_dash(&connection.metadata.source_port)
    )
}

fn destination_endpoint(connection: &Connection) -> String {
    format!(
        "{}:{}",
        empty_as_dash(&connection.metadata.destination_ip),
        empty_as_dash(&connection.metadata.destination_port)
    )
}

fn geo_line(connection: &Connection) -> String {
    let source = connection
        .metadata
        .source_geo_ip
        .as_ref()
        .map(|geo| geo.join("/"))
        .unwrap_or_else(|| "-".to_string());
    let target = connection
        .metadata
        .destination_geo_ip
        .as_ref()
        .map(|geo| geo.join("/"))
        .unwrap_or_else(|| "-".to_string());
    format!("{} → {}", source, target)
}

fn asn_line(connection: &Connection) -> String {
    format!(
        "{} → {}",
        empty_as_dash(&connection.metadata.source_ip_asn),
        empty_as_dash(&connection.metadata.destination_ip_asn)
    )
}

fn inbound_line(connection: &Connection) -> String {
    format!(
        "{} {}:{}",
        empty_as_dash(&connection.metadata.inbound_name),
        empty_as_dash(&connection.metadata.inbound_ip),
        empty_as_dash(&connection.metadata.inbound_port)
    )
}

fn rule_line(connection: &Connection) -> String {
    if connection.rule_payload.is_empty() {
        empty_as_dash(&connection.rule).to_string()
    } else {
        format!("{} / {}", empty_as_dash(&connection.rule), connection.rule_payload)
    }
}

fn compact_chain(chains: &[String]) -> String {
    match chains {
        [] => "-".to_string(),
        [one] => one.clone(),
        [first, .., last] => format!("{} → {}", first, last),
    }
}

fn network_name(connection: &Connection) -> String {
    format!("{:?}", connection.metadata.network)
}

fn empty_as_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

fn delay_style(delay: Option<u16>, theme: Theme) -> Style {
    match delay {
        Some(delay) if delay < 180 => Style::default().fg(theme.success),
        Some(delay) if delay < 500 => Style::default().fg(theme.warning),
        Some(_) => Style::default().fg(theme.error),
        None => theme.muted_style(),
    }
}

fn format_delay(delay: u16) -> String {
    format!("{} ms", delay)
}

fn format_rate(bytes_per_second: u64) -> String {
    format!("{}/s", format_network_bytes(bytes_per_second))
}

/// unix 秒 → UTC `YYYY-MM-DD HH:MM:SS`（Howard Hinnant 民用日期算法，无外部依赖）。
fn format_unix_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}")
}

/// 天数（自 1970-01-01 起）→ (year, month, day)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_network_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        return format!("{} {}", bytes, UNITS[unit]);
    }

    let formatted = format!("{:.1}", value);
    let formatted = formatted.strip_suffix(".0").unwrap_or(&formatted);

    format!("{} {}", formatted, UNITS[unit])
}

fn sparkline_history(history: &[DelayHistory]) -> String {
    let values: Vec<u16> = history.iter().map(|entry| entry.delay).collect();
    sparkline_u16(&values)
}

fn sparkline_u16(values: &[u16]) -> String {
    let values: Vec<u64> = values.iter().map(|value| u64::from(*value)).collect();
    sparkline_u64(&values)
}

fn sparkline_u64(values: &[u64]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() {
        return String::new();
    }

    let start = values.len().saturating_sub(18);
    let visible = &values[start..];
    let min = visible.iter().min().copied().unwrap_or(0);
    let max = visible.iter().max().copied().unwrap_or(min);
    let span = max.saturating_sub(min).max(1);

    visible
        .iter()
        .map(|value| {
            let idx = (value.saturating_sub(min) * 7 / span) as usize;
            BARS[idx]
        })
        .collect()
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ]
            .as_ref(),
        )
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            [
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ]
            .as_ref(),
        )
        .split(vertical[1])[1]
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        })
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, format_network_bytes, format_unix_time};

    #[test]
    fn format_unix_time_matches_known_instants() {
        assert_eq!(format_unix_time(0), "1970-01-01 00:00:00");
        assert_eq!(format_unix_time(1_600_000_000), "2020-09-13 12:26:40");
    }

    #[test]
    fn civil_from_days_covers_leap_years() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_518), (2020, 9, 13));
        // 2000-02-29 是闰日；11016 天 = 951782400 秒
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn format_network_bytes_upgrades_when_value_exceeds_three_digits() {
        assert_eq!(format_network_bytes(999), "999 B");
        assert_eq!(format_network_bytes(1000), "1 KB");
        assert_eq!(format_network_bytes(10 * 1024), "10 KB");
        assert_eq!(format_network_bytes(1536), "1.5 KB");
        assert_eq!(format_network_bytes(1000 * 1024), "1 MB");
        assert_eq!(format_network_bytes(1024 * 1024), "1 MB");
    }
}
