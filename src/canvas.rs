use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Tabs, Wrap},
};

use crate::{
    app::{
        APP, WindowState,
        checker::{AnalysisResult, IPInfo, UnlockStatus},
        connections::{ConnectionsSort, ConnectionsState},
        nettest::{NetTestState, NetTestTab, SortOrder, TestStatus},
        proxy::{ProxyFocus, ProxyState},
        state::MenuItem,
    },
    ipc::models::{Connection, DelayHistory, Proxy as ProxyModel},
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

            match &app.current_page {
                WindowState::Memu => MenuLayout::draw(f, shell[2], app, theme),
                WindowState::Proxy(state) => ProxyLayout::draw(f, shell[2], state, theme),
                WindowState::Connects(state) => ConnectionsLayout::draw(f, shell[2], state, theme),
                WindowState::NetTest(state) => NetTestLayout::draw(f, shell[2], state, theme),
            }

            draw_footer(f, shell[3], app, theme);

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
    let title = Span::styled(
        " Clard ",
        Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD),
    );
    let page = Span::styled(
        format!(" {} ", page_name(app)),
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
    );
    let summary = Span::styled(header_summary(app), theme.muted_style());
    let line = Line::from(vec![
        title,
        Span::styled("mihomo / Clash terminal dashboard", theme.muted_style()),
        Span::raw("  "),
        page,
        Span::raw("  "),
        summary,
    ]);

    f.render_widget(
        Paragraph::new(line)
            .block(panel_block("Overview", false, theme))
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_navigation(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
    let selected = selected_tab_index(app);
    let tabs = Tabs::new(vec![" 1  Proxy ", " 2  Connections ", " 3  Net Test "])
        .block(panel_block("Workspaces", false, theme))
        .select(selected)
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

struct MenuLayout;

impl MenuLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, app: &APP, theme: Theme) {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(34), Constraint::Min(0)].as_ref())
            .split(area);

        let menu_items: Vec<ListItem<'_>> = MenuItem::ALL
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let selected = app.menusate.current() == *item;
                let (title, desc, action) = menu_copy(*item);
                let marker = if selected { "▶" } else { " " };
                let style = if selected {
                    Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.fg)
                };

                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(marker, Style::default().fg(theme.primary)),
                        Span::raw(" "),
                        Span::styled(format!("{}  {}", idx + 1, title), style),
                    ]),
                    Line::from(vec![Span::raw("   "), Span::styled(desc, theme.muted_style())]),
                    Line::from(vec![
                        Span::raw("   "),
                        Span::styled(action, Style::default().fg(theme.info)),
                    ]),
                ])
            })
            .collect();

        let menu = List::new(menu_items).block(panel_block("Choose Workspace", true, theme));
        f.render_widget(menu, columns[0]);

        let right_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(7), Constraint::Length(7), Constraint::Min(0)].as_ref())
            .split(columns[1]);

        let data_lines = vec![
            kv_line(
                "Proxy",
                "proxy groups, current selected node, node health, delay history",
                theme,
            ),
            kv_line(
                "Connections",
                "active sessions, upload/download totals, memory, routing chains",
                theme,
            ),
            kv_line(
                "Net Test",
                "batch latency, per-node history, direct/proxy IP, streaming unlock status",
                theme,
            ),
        ];
        f.render_widget(
            Paragraph::new(data_lines)
                .block(panel_block("Available Data Surface", false, theme))
                .wrap(Wrap { trim: true }),
            right_rows[0],
        );

        let api_lines = vec![
            kv_line(
                "HTTP",
                "version, caches, groups, proxies, connections, rules, configs",
                theme,
            ),
            kv_line(
                "Actions",
                "select node, test delay, close selected connection, refresh data",
                theme,
            ),
            kv_line(
                "Realtime",
                "connection page refreshes every second while visible",
                theme,
            ),
        ];
        f.render_widget(
            Paragraph::new(api_lines)
                .block(panel_block("Backend Coverage", false, theme))
                .wrap(Wrap { trim: true }),
            right_rows[1],
        );

        let guide = vec![
            Line::from(vec![
                Span::styled("Layout model", theme.title_style()),
                Span::raw("  Persistent multi-panel dashboard"),
            ]),
            Line::from(""),
            Line::from("Use the number keys to jump directly, or open a page and keep its context visible."),
            Line::from("The focused panel is highlighted; the footer always shows actions that apply now."),
            Line::from("Press [?] anytime for the full key reference."),
        ];
        f.render_widget(
            Paragraph::new(guide)
                .block(panel_block("Interaction Model", false, theme))
                .wrap(Wrap { trim: true }),
            right_rows[2],
        );
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
            "Nodes  Enter select  t test",
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
    let upload = data
        .map(|data| format_network_bytes(data.upload_total))
        .unwrap_or_else(|| "-".to_string());
    let download = data
        .map(|data| format_network_bytes(data.download_total))
        .unwrap_or_else(|| "-".to_string());
    let memory = data
        .map(|data| format_network_bytes(u64::from(data.memory)))
        .unwrap_or_else(|| "-".to_string());

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
    draw_metric(f, chunks[1], "Upload", &upload, "total uploaded", theme.warning, theme);
    draw_metric(
        f,
        chunks[2],
        "Download",
        &download,
        "total downloaded",
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

    let rows = state.connections.iter().map(|connection| {
        let host = connection_host(connection);
        let process = empty_as_dash(&connection.metadata.process);
        let chain = compact_chain(&connection.chains);
        Row::new(vec![
            Cell::from(host),
            Cell::from(process.to_string()),
            Cell::from(network_name(connection)),
            Cell::from(format_network_bytes(connection.upload)),
            Cell::from(format_network_bytes(connection.download)),
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
    .block(panel_block("Connections  u/d sort  x close selected", true, theme))
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

struct NetTestLayout;

impl NetTestLayout {
    fn draw(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)].as_ref())
            .split(area);

        draw_nettest_tabs(f, chunks[0], state, theme);

        match state.tab {
            NetTestTab::Latency => draw_latency_view(f, chunks[1], state, theme),
            NetTestTab::Analysis => draw_analysis_view(f, chunks[1], state, theme),
        }
    }
}

fn draw_nettest_tabs(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
    let selected = match state.tab {
        NetTestTab::Latency => 0,
        NetTestTab::Analysis => 1,
    };
    let titles = vec![" Latency ", " Active Node Analysis "];
    let tabs = Tabs::new(titles)
        .block(panel_block("Net Test", false, theme))
        .select(selected)
        .style(Style::default().fg(theme.muted))
        .highlight_style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn draw_latency_view(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(0)].as_ref())
        .split(area);

    draw_latency_metrics(f, chunks[0], state, theme);
    draw_latency_table(f, chunks[1], state, theme);
}

fn draw_latency_metrics(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
    let total = state.nodes.len();
    let done = state
        .nodes
        .iter()
        .filter(|node| matches!(node.status, TestStatus::Done(_)))
        .count();
    let testing = state
        .nodes
        .iter()
        .filter(|node| matches!(node.status, TestStatus::Testing))
        .count();
    let latencies: Vec<u16> = state
        .nodes
        .iter()
        .filter_map(|node| match node.status {
            TestStatus::Done(latency) => Some(latency),
            _ => None,
        })
        .collect();
    let fastest = latencies.iter().min().copied();
    let avg = if latencies.is_empty() {
        None
    } else {
        Some(latencies.iter().map(|latency| u32::from(*latency)).sum::<u32>() / latencies.len() as u32)
    };

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
        "Progress",
        &format!("{} / {}", done, total),
        &progress_bar(done, total, 12),
        theme.primary,
        theme,
    );
    draw_metric(
        f,
        chunks[1],
        "Testing",
        &testing.to_string(),
        if state.is_testing { "running" } else { "idle" },
        theme.info,
        theme,
    );
    draw_metric(
        f,
        chunks[2],
        "Fastest",
        &fastest.map_or_else(|| "-".to_string(), format_delay),
        "lower is better",
        theme.success,
        theme,
    );
    draw_metric(
        f,
        chunks[3],
        "Average",
        &avg.map_or_else(|| "-".to_string(), |latency| format!("{} ms", latency)),
        sort_order_name(&state.sort_order),
        theme.secondary,
        theme,
    );
}

fn draw_latency_table(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
    let header = Row::new(
        ["Node", "Status", "Latency", "History", "Quality"]
            .into_iter()
            .map(|header| Cell::from(header).style(Style::default().fg(theme.emphasis).add_modifier(Modifier::BOLD))),
    )
    .height(1)
    .bottom_margin(1)
    .style(Style::default().bg(theme.overlay));

    let rows = state.nodes.iter().map(|node| {
        let (status, style) = test_status_style(&node.status, theme);
        let delay = match node.status {
            TestStatus::Done(latency) => format_delay(latency),
            _ => "-".to_string(),
        };
        let quality = match node.status {
            TestStatus::Done(latency) => latency_bar(latency, 16),
            TestStatus::Testing => "testing".to_string(),
            TestStatus::Timeout => "timeout".to_string(),
            TestStatus::Error(_) => "error".to_string(),
            TestStatus::Pending => "pending".to_string(),
        };

        Row::new(vec![
            Cell::from(node.name.clone()),
            Cell::from(status).style(style),
            Cell::from(delay).style(style),
            Cell::from(sparkline_u16(&node.history)).style(Style::default().fg(theme.secondary)),
            Cell::from(quality).style(style),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(14),
            Constraint::Percentage(12),
            Constraint::Percentage(18),
            Constraint::Percentage(16),
        ],
    )
    .header(header)
    .block(panel_block("Latency  t/r test all  s sort", true, theme))
    .row_highlight_style(theme.selected_style())
    .highlight_symbol("▸ ");

    let mut list_state = state.list_state.clone();
    f.render_stateful_widget(table, area, &mut list_state);
}

fn draw_analysis_view(f: &mut Frame<'_>, area: Rect, state: &NetTestState, theme: Theme) {
    if area.width >= 108 {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)].as_ref())
            .split(area);
        draw_ip_panel(f, chunks[0], &state.analysis_result, state.analysis_testing, theme);
        draw_streaming_panel(f, chunks[1], &state.analysis_result, state.analysis_testing, theme);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)].as_ref())
        .split(area);
    draw_ip_panel(f, chunks[0], &state.analysis_result, state.analysis_testing, theme);
    draw_streaming_panel(f, chunks[1], &state.analysis_result, state.analysis_testing, theme);
}

fn draw_ip_panel(f: &mut Frame<'_>, area: Rect, result: &AnalysisResult, testing: bool, theme: Theme) {
    let lines = vec![
        ip_section("Direct IP", "Domestic view", result.direct_ip.as_ref(), testing, theme),
        Line::from(""),
        ip_section(
            "Proxy IP",
            "International view",
            result.proxy_ip.as_ref(),
            testing,
            theme,
        ),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("IP Identity", false, theme))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_streaming_panel(f: &mut Frame<'_>, area: Rect, result: &AnalysisResult, testing: bool, theme: Theme) {
    let mut lines = vec![
        unlock_line("YouTube", &result.youtube_status, testing, theme),
        unlock_line("Netflix", &result.netflix_status, testing, theme),
        unlock_line("Spotify", &result.spotify_status, testing, theme),
        unlock_line("Bilibili", &result.bilibili_status, testing, theme),
    ];
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Analysis runs through the local mixed proxy http://127.0.0.1:7890.",
        theme.muted_style(),
    )));

    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block(
                "Streaming Unlock",
                matches!(result.youtube_status, UnlockStatus::Testing)
                    || matches!(result.netflix_status, UnlockStatus::Testing)
                    || matches!(result.spotify_status, UnlockStatus::Testing)
                    || matches!(result.bilibili_status, UnlockStatus::Testing),
                theme,
            ))
            .wrap(Wrap { trim: true }),
        area,
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
            Span::raw("back/close  "),
            key_span("?", theme),
            Span::raw("help  "),
            key_span("1/2/3", theme),
            Span::raw("jump"),
        ]),
        Line::from(vec![
            Span::styled("Move", theme.title_style()),
            Span::raw("    arrows or "),
            key_span("h/j/k/l", theme),
            Span::raw("  "),
            key_span("Tab", theme),
            Span::raw("focus / tab switch"),
        ]),
        Line::from(""),
    ];

    lines.extend(match &app.current_page {
        WindowState::Memu => vec![
            Line::from(Span::styled("Menu", theme.title_style())),
            Line::from("Enter opens the selected workspace. Number keys jump directly."),
        ],
        WindowState::Proxy(_) => vec![
            Line::from(Span::styled("Proxy", theme.title_style())),
            Line::from("Left/right or Tab changes focus between groups and nodes."),
            Line::from("Enter selects the highlighted node for the active group."),
            Line::from("t or d tests delay for the highlighted node."),
        ],
        WindowState::Connects(_) => vec![
            Line::from(Span::styled("Connections", theme.title_style())),
            Line::from("u sorts by upload, d sorts by download, x closes the selected connection."),
            Line::from("The list refreshes automatically every second while visible."),
        ],
        WindowState::NetTest(state) => match state.tab {
            NetTestTab::Latency => vec![
                Line::from(Span::styled("Net Test / Latency", theme.title_style())),
                Line::from("t or r tests every known node concurrently."),
                Line::from("s cycles sort order: latency asc/desc and name asc/desc."),
                Line::from("Tab switches to active-node analysis."),
            ],
            NetTestTab::Analysis => vec![
                Line::from(Span::styled("Net Test / Analysis", theme.title_style())),
                Line::from("Tab returns to latency testing."),
                Line::from("Entering this tab triggers IP and streaming unlock checks."),
            ],
        },
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

fn selected_tab_index(app: &APP) -> usize {
    match &app.current_page {
        WindowState::Memu => match app.menusate.current() {
            MenuItem::Proxy => 0,
            MenuItem::Connections => 1,
            MenuItem::NetTest => 2,
        },
        WindowState::Proxy(_) => 0,
        WindowState::Connects(_) => 1,
        WindowState::NetTest(_) => 2,
    }
}

fn page_name(app: &APP) -> &'static str {
    match &app.current_page {
        WindowState::Memu => "Menu",
        WindowState::Proxy(_) => "Proxy",
        WindowState::Connects(_) => "Connections",
        WindowState::NetTest(_) => "Net Test",
    }
}

fn header_summary(app: &APP) -> String {
    match &app.current_page {
        WindowState::Memu => "3 workspaces · keyboard-first · async actions".to_string(),
        WindowState::Proxy(state) => {
            let groups = state.groups.len();
            let nodes: usize = state
                .groups
                .iter()
                .filter_map(|group| group.all.as_ref())
                .map(Vec::len)
                .sum();
            format!("{} groups · {} selectable nodes", groups, nodes)
        }
        WindowState::Connects(state) => {
            if let Some(data) = &state.connections_data {
                format!(
                    "{} active · up {} · down {} · mem {}",
                    state.connections.len(),
                    format_network_bytes(data.upload_total),
                    format_network_bytes(data.download_total),
                    format_network_bytes(u64::from(data.memory))
                )
            } else {
                "loading connections".to_string()
            }
        }
        WindowState::NetTest(state) => {
            let completed = state
                .nodes
                .iter()
                .filter(|node| matches!(node.status, TestStatus::Done(_)))
                .count();
            let suffix = if state.is_testing || state.analysis_testing {
                " · running"
            } else {
                ""
            };
            format!("{} nodes · {} tested{}", state.nodes.len(), completed, suffix)
        }
    }
}

fn footer_keys(app: &APP) -> Vec<(&'static str, &'static str)> {
    match &app.current_page {
        WindowState::Memu => vec![
            ("↑↓/jk", "select"),
            ("Enter", "open"),
            ("1-3", "jump"),
            ("?", "help"),
            ("q", "quit"),
        ],
        WindowState::Proxy(state) => {
            let focus = if state.focus == ProxyFocus::Groups {
                "nodes focus"
            } else {
                "groups focus"
            };
            vec![
                ("↑↓/jk", "move"),
                ("Tab/←→", focus),
                ("Enter", "select"),
                ("t", "test"),
                ("Esc", "menu"),
            ]
        }
        WindowState::Connects(_) => vec![
            ("↑↓/jk", "move"),
            ("u/d", "sort"),
            ("x", "close"),
            ("?", "help"),
            ("Esc", "menu"),
        ],
        WindowState::NetTest(state) => match state.tab {
            NetTestTab::Latency => vec![
                ("↑↓/jk", "move"),
                ("t/r", "test all"),
                ("s", "sort"),
                ("Tab", "analysis"),
                ("Esc", "menu"),
            ],
            NetTestTab::Analysis => vec![("Tab", "latency"), ("?", "help"), ("Esc", "menu"), ("q", "quit")],
        },
    }
}

fn menu_copy(item: MenuItem) -> (&'static str, &'static str, &'static str) {
    match item {
        MenuItem::Proxy => ("Proxy", "代理组与节点选择", "Enter: open · t: node delay"),
        MenuItem::Connections => ("Connections", "实时连接、流量与路由链路", "u/d: sort · x: close"),
        MenuItem::NetTest => ("Net Test", "节点延迟与流媒体解锁分析", "t: test all · Tab: switch"),
    }
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
        crate::ipc::models::ProxyType::Direct => "Direct",
        crate::ipc::models::ProxyType::Reject => "Reject",
        crate::ipc::models::ProxyType::RejectDrop => "RejectDrop",
        crate::ipc::models::ProxyType::Compatible => "Compatible",
        crate::ipc::models::ProxyType::Pass => "Pass",
        crate::ipc::models::ProxyType::Dns => "DNS",
        crate::ipc::models::ProxyType::Shadowsocks => "SS",
        crate::ipc::models::ProxyType::ShadowsocksR => "SSR",
        crate::ipc::models::ProxyType::Snell => "Snell",
        crate::ipc::models::ProxyType::Socks5 => "Socks5",
        crate::ipc::models::ProxyType::Http => "HTTP",
        crate::ipc::models::ProxyType::Vmess => "Vmess",
        crate::ipc::models::ProxyType::Vless => "Vless",
        crate::ipc::models::ProxyType::Trojan => "Trojan",
        crate::ipc::models::ProxyType::Hysteria => "Hysteria",
        crate::ipc::models::ProxyType::Hysteria2 => "Hysteria2",
        crate::ipc::models::ProxyType::WireGuard => "WireGuard",
        crate::ipc::models::ProxyType::Tuic => "Tuic",
        crate::ipc::models::ProxyType::Ssh => "SSH",
        crate::ipc::models::ProxyType::Mieru => "Mieru",
        crate::ipc::models::ProxyType::AnyTLS => "AnyTLS",
        crate::ipc::models::ProxyType::Relay => "Relay",
        crate::ipc::models::ProxyType::Selector => "Selector",
        crate::ipc::models::ProxyType::Fallback => "Fallback",
        crate::ipc::models::ProxyType::URLTest => "URLTest",
        crate::ipc::models::ProxyType::LoadBalance => "LoadBalance",
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

fn ip_section(title: &str, subtitle: &str, ip: Option<&IPInfo>, testing: bool, theme: Theme) -> Line<'static> {
    let (status, color) = match ip {
        Some(_) => ("READY", theme.success),
        None if testing => ("TESTING", theme.info),
        None => ("MISSING", theme.warning),
    };
    let detail = ip.map_or_else(|| "-".to_string(), |ip| format!("{}  {}", ip.ip, ip.region));

    Line::from(vec![
        Span::styled(format!("{:<10}", title), theme.title_style()),
        Span::styled(format!("{:<8}", status), Style::default().fg(color)),
        Span::styled(format!("{}  ", subtitle), theme.muted_style()),
        Span::raw(detail),
    ])
}

fn unlock_line(service: &str, status: &UnlockStatus, testing: bool, theme: Theme) -> Line<'static> {
    let (label, detail, color) = match status {
        UnlockStatus::Testing if testing => ("TESTING", "checking".to_string(), theme.info),
        UnlockStatus::Testing => ("WAIT", "not tested".to_string(), theme.warning),
        UnlockStatus::Unlocked(region) => ("OK", format!("unlocked ({})", region), theme.success),
        UnlockStatus::OriginalsOnly => ("PARTIAL", "originals only".to_string(), theme.warning),
        UnlockStatus::Blocked(reason) => ("BLOCKED", reason.clone(), theme.error),
        UnlockStatus::Error(err) => ("ERROR", err.clone(), theme.error),
    };

    Line::from(vec![
        Span::styled(format!("{:<10}", service), theme.title_style()),
        Span::styled(format!("{:<9}", label), Style::default().fg(color)),
        Span::raw(detail),
    ])
}

fn test_status_style(status: &TestStatus, theme: Theme) -> (String, Style) {
    match status {
        TestStatus::Pending => ("pending".to_string(), theme.muted_style()),
        TestStatus::Testing => ("testing".to_string(), Style::default().fg(theme.info)),
        TestStatus::Done(latency) => ("online".to_string(), delay_style(Some(*latency), theme)),
        TestStatus::Timeout => ("timeout".to_string(), Style::default().fg(theme.error)),
        TestStatus::Error(_) => ("error".to_string(), Style::default().fg(theme.error)),
    }
}

fn delay_style(delay: Option<u16>, theme: Theme) -> Style {
    match delay {
        Some(delay) if delay < 180 => Style::default().fg(theme.success),
        Some(delay) if delay < 500 => Style::default().fg(theme.warning),
        Some(_) => Style::default().fg(theme.error),
        None => theme.muted_style(),
    }
}

fn sort_order_name(sort_order: &SortOrder) -> &'static str {
    match sort_order {
        SortOrder::None => "unsorted",
        SortOrder::LatencyAsc => "latency asc",
        SortOrder::LatencyDesc => "latency desc",
        SortOrder::NameAsc => "name asc",
        SortOrder::NameDesc => "name desc",
    }
}

fn format_delay(delay: u16) -> String {
    format!("{} ms", delay)
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

fn progress_bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return "░".repeat(width);
    }
    let filled = ((done * width) + total - 1) / total;
    format!("{}{}", "█".repeat(filled), "░".repeat(width.saturating_sub(filled)))
}

fn latency_bar(delay: u16, width: usize) -> String {
    let score = if delay < 180 {
        width
    } else if delay < 500 {
        width.saturating_mul(2) / 3
    } else if delay < 1000 {
        width / 3
    } else {
        width / 6
    };
    format!("{}{}", "█".repeat(score), "░".repeat(width.saturating_sub(score)))
}

fn sparkline_history(history: &[DelayHistory]) -> String {
    let values: Vec<u16> = history.iter().map(|entry| entry.delay).collect();
    sparkline_u16(&values)
}

fn sparkline_u16(values: &[u16]) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if values.is_empty() {
        return String::new();
    }

    let start = values.len().saturating_sub(18);
    let visible = &values[start..];
    let min = visible.iter().min().copied().unwrap_or(0);
    let max = visible.iter().max().copied().unwrap_or(min);
    let span = u32::from(max.saturating_sub(min)).max(1);

    visible
        .iter()
        .map(|value| {
            let idx = (u32::from(value.saturating_sub(min)) * 7 / span) as usize;
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
    use super::format_network_bytes;

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
