use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Sparkline, Table, Tabs},
};

use crate::app::{
    APP, WindowState,
    checker::UnlockStatus,
    connections::ConnectionsState,
    nettest::{NetTestState, NetTestTab, TestStatus},
    preview::PreviewState,
    proxy::{ProxyFocus, ProxyState},
    rules::RulesState,
    state::MenuItem,
};

#[derive(Debug, Default)]
pub struct Painter;
impl Painter {
    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>, app: &APP) {
        let _ = terminal.draw(|f| {
            let area = f.area();
            match &app.current_page {
                WindowState::Memu => {
                    MenuLayout::draw_menu(f, area, app);
                }
                WindowState::Proxy(state) => {
                    ProxyLayout::draw_proxy(f, area, state, &app.message);
                }
                WindowState::Preview(state) => {
                    PreviewLayout::draw_preview(f, area, state, &app.message);
                }
                WindowState::Connects(state) => {
                    ConnectionsLayout::draw_connections(f, area, state, &app.message);
                }
                WindowState::NetTest(state) => {
                    NetTestLayout::draw_nettest(f, area, state, &app.message);
                }
                WindowState::Rules(state) => {
                    RulesLayout::draw_rules(f, area, state, &app.message);
                }
            }
        });
    }
}

struct RulesLayout;
impl RulesLayout {
    fn draw_rules(f: &mut Frame<'_>, area: Rect, state: &RulesState, msg: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(area);

        let header_cells = ["Type", "Payload", "Proxy", "Size"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1)
            .bottom_margin(1);

        let rows = state.rules.iter().map(|rule| {
            Row::new(vec![
                Cell::from(format!("{:?}", rule.rule_type)),
                Cell::from(rule.payload.clone()),
                Cell::from(rule.proxy.clone()),
                Cell::from(rule.size.to_string()),
            ])
        });

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(16),
                Constraint::Percentage(54),
                Constraint::Percentage(20),
                Constraint::Percentage(10),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Rules [Esc to Menu, Up/Down to navigate]"),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
        .highlight_symbol(">> ");

        let mut list_state = state.list_state.clone();
        f.render_stateful_widget(table, chunks[0], &mut list_state);

        let footer_text = msg
            .clone()
            .unwrap_or_else(|| format!("Loaded {} rules", state.rules.len()));
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(footer, chunks[1]);
    }
}

struct MenuLayout;
impl MenuLayout {
    fn draw_menu(f: &mut Frame<'_>, area: Rect, app: &APP) {
        let iterms: Vec<ListItem<'_>> = MenuItem::ALL
            .iter()
            .map(|mi| {
                let label = mi.to_str();
                if app.menusate.current() == *mi {
                    ListItem::new(label).style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
                } else {
                    ListItem::new(label)
                }
            })
            .collect();
        let menu_list = List::new(iterms).block(Block::default().borders(Borders::ALL).title("Menu [Enter]"));
        f.render_widget(menu_list, area);
    }
}

struct PreviewLayout;
impl PreviewLayout {
    fn draw_preview(f: &mut Frame<'_>, area: Rect, state: &PreviewState, msg: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(
                [
                    Constraint::Length(10),
                    Constraint::Length(7),
                    Constraint::Length(7),
                    Constraint::Min(0),
                ]
                .as_ref(),
            )
            .split(area);

        // System + Core + IP Block
        let core_version = state
            .version
            .as_ref()
            .map(|v| format!("{} (meta: {})", v.version, v.meta))
            .unwrap_or_else(|| "Loading...".to_string());

        let direct_ip_text = state
            .direct_ip
            .as_ref()
            .map(|ip| format!("{} / {}", ip.ip, ip.region))
            .unwrap_or_else(|| "N/A".to_string());
        let proxy_ip_text = state
            .proxy_ip
            .as_ref()
            .map(|ip| format!("{} / {}", ip.ip, ip.region))
            .unwrap_or_else(|| "N/A".to_string());

        let version_text = format!(
            "Core Version: {}\nVerge Version: {}\nDistribution: {}\nKernel: {}\nDirect IP: {}\nProxy IP: {}",
            core_version,
            state.system_info.verge_version,
            state.system_info.distribution,
            state.system_info.kernel_version,
            direct_ip_text,
            proxy_ip_text
        );
        let version_p = Paragraph::new(version_text).block(Block::default().borders(Borders::ALL).title("System Info"));
        f.render_widget(version_p, chunks[0]);

        // Traffic/Connections Block
        let conn_text = match &state.connections {
            Some(c) => format!(
                "Total Download: {} bytes\nTotal Upload: {} bytes\nMemory: {} bytes\nActive Connections: {}",
                c.download_total,
                c.upload_total,
                c.memory,
                c.connections.as_ref().map_or(0, |v| v.len())
            ),
            None => "Loading connection data...".to_string(),
        };
        let conn_p =
            Paragraph::new(conn_text).block(Block::default().borders(Borders::ALL).title("System [Esc to Menu]"));
        f.render_widget(conn_p, chunks[1]);

        // Traffic Sparkline Block
        let traffic_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(chunks[2]);

        let up_data: Vec<u64> = state.traffic_history.iter().map(|t| t.up).collect();
        let down_data: Vec<u64> = state.traffic_history.iter().map(|t| t.down).collect();

        let up_max = up_data.iter().max().copied().unwrap_or(1);
        let down_max = down_data.iter().max().copied().unwrap_or(1);

        let current_up = state.current_traffic.as_ref().map_or(0, |t| t.up);
        let current_down = state.current_traffic.as_ref().map_or(0, |t| t.down);

        let up_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(format!("Upload ({} B/s)", current_up))
                    .borders(Borders::ALL),
            )
            .data(&up_data)
            .max(up_max)
            .style(Style::default().fg(Color::Green));

        let down_sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title(format!("Download ({} B/s)", current_down))
                    .borders(Borders::ALL),
            )
            .data(&down_data)
            .max(down_max)
            .style(Style::default().fg(Color::Cyan));

        f.render_widget(up_sparkline, traffic_chunks[0]);
        f.render_widget(down_sparkline, traffic_chunks[1]);

        // Message Block
        let msg_text = msg
            .clone()
            .unwrap_or_else(|| "Press [Esc] to return to Menu".to_string());
        let msg_p = Paragraph::new(msg_text).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(msg_p, chunks[3]);
    }
}

struct ProxyLayout;
impl ProxyLayout {
    fn draw_proxy(f: &mut Frame<'_>, area: Rect, state: &ProxyState, msg: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(area);

        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)].as_ref())
            .split(chunks[0]);

        // Render Groups
        let groups_block = Block::default()
            .borders(Borders::ALL)
            .title("Groups [Left/Right to switch, Esc to Menu]")
            .style(if state.focus == ProxyFocus::Groups {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });

        let group_items: Vec<ListItem<'_>> = state
            .groups
            .iter()
            .map(|g| {
                ListItem::new(Line::from(vec![
                    Span::raw(g.name.clone()),
                    Span::raw(format!(" [{}]", g.now.clone().unwrap_or_default())),
                ]))
            })
            .collect();

        let group_list = List::new(group_items)
            .block(groups_block)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
            .highlight_symbol(">> ");

        let mut group_list_state = state.group_list_state.clone();
        f.render_stateful_widget(group_list, main_chunks[0], &mut group_list_state);

        // Render Proxies for selected group
        let proxies_block = Block::default()
            .borders(Borders::ALL)
            .title("Proxies [Enter to select, 'd' or 't' to test delay]")
            .style(if state.focus == ProxyFocus::Proxies {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            });

        let mut proxy_items = Vec::new();
        if let Some(g_idx) = state.group_list_state.selected() {
            if let Some(group) = state.groups.get(g_idx) {
                if let Some(all) = &group.all {
                    for node in all {
                        let is_now = group.now.as_deref() == Some(node);
                        let text = if is_now {
                            Span::styled(
                                format!("* {}", node),
                                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                            )
                        } else {
                            Span::raw(format!("  {}", node))
                        };
                        proxy_items.push(ListItem::new(Line::from(vec![text])));
                    }
                }
            }
        }

        let proxy_list = List::new(proxy_items)
            .block(proxies_block)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
            .highlight_symbol(">> ");

        let mut proxy_list_state = state.proxy_list_state.clone();
        f.render_stateful_widget(proxy_list, main_chunks[1], &mut proxy_list_state);

        // Render Footer (Messages)
        let footer_text = msg.clone().unwrap_or_else(|| "Proxy Manager".to_string());
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(footer, chunks[1]);
    }
}

struct ConnectionsLayout;
impl ConnectionsLayout {
    fn draw_connections(f: &mut Frame<'_>, area: Rect, state: &ConnectionsState, msg: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(area);

        let header_cells = ["Host", "Network", "Upload", "Download", "Rule", "Chains"]
            .iter()
            .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
        let header = Row::new(header_cells)
            .style(Style::default().bg(Color::DarkGray))
            .height(1)
            .bottom_margin(1);

        let rows = state.connections.iter().map(|c| {
            let host = if c.metadata.host.is_empty() {
                c.metadata.destination_ip.clone()
            } else {
                c.metadata.host.clone()
            };

            let network_str = format!("{:?}", c.metadata.network);
            let up_str = format!("{} B", c.upload);
            let down_str = format!("{} B", c.download);
            let rule_str = c.rule.clone();
            let chain_str = c.chains.join(" -> ");

            Row::new(vec![
                Cell::from(host),
                Cell::from(network_str),
                Cell::from(up_str),
                Cell::from(down_str),
                Cell::from(rule_str),
                Cell::from(chain_str),
            ])
        });

        let t = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Percentage(10),
                Constraint::Percentage(10),
                Constraint::Percentage(10),
                Constraint::Percentage(15),
                Constraint::Percentage(25),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Connections [Esc to Menu, 'x' or 'd' to close connection]"),
        )
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
        .highlight_symbol(">> ");

        let mut list_state = state.list_state.clone();
        f.render_stateful_widget(t, chunks[0], &mut list_state);

        let footer_text = msg.clone().unwrap_or_else(|| "Connections Manager".to_string());
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(footer, chunks[1]);
    }
}

struct NetTestLayout;
impl NetTestLayout {
    fn draw_nettest(f: &mut Frame<'_>, area: Rect, state: &NetTestState, msg: &Option<String>) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)].as_ref())
            .split(area);

        // Draw Tabs
        let titles = vec!["Node Latency", "Active Node Analysis"];
        let tab_idx = match state.tab {
            NetTestTab::Latency => 0,
            NetTestTab::Analysis => 1,
        };
        let tabs = Tabs::new(titles)
            .block(Block::default().borders(Borders::ALL).title("NetTest [Tab to switch]"))
            .select(tab_idx)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).fg(Color::Yellow));
        f.render_widget(tabs, chunks[0]);

        match state.tab {
            NetTestTab::Latency => {
                let header_cells = ["Node", "Status", "Latency", "Visual"]
                    .iter()
                    .map(|h| Cell::from(*h).style(Style::default().fg(Color::Yellow)));
                let header = Row::new(header_cells)
                    .style(Style::default().bg(Color::DarkGray))
                    .height(1)
                    .bottom_margin(1);

                let rows = state.nodes.iter().map(|n| {
                    let mut lat_str = String::from("-");
                    let mut status_str = "Pending";
                    let mut vis_str = String::new();
                    let mut fg_color = Color::Gray;

                    match &n.status {
                        TestStatus::Pending => {}
                        TestStatus::Testing => {
                            status_str = "Testing...";
                            fg_color = Color::Cyan;
                        }
                        TestStatus::Done(lat) => {
                            status_str = "Online";
                            lat_str = format!("{}ms", lat);
                            if *lat < 200 {
                                fg_color = Color::Green;
                                vis_str = "███████████▌".to_string();
                            } else if *lat < 500 {
                                fg_color = Color::Yellow;
                                vis_str = "██████▌".to_string();
                            } else {
                                fg_color = Color::Red;
                                vis_str = "██▌".to_string();
                            }
                        }
                        TestStatus::Timeout => {
                            status_str = "Timeout";
                            fg_color = Color::Red;
                            vis_str = "✕".to_string();
                        }
                        TestStatus::Error(_) => {
                            status_str = "Error";
                            fg_color = Color::Red;
                            vis_str = "✕".to_string();
                        }
                    }

                    Row::new(vec![
                        Cell::from(n.name.clone()),
                        Cell::from(status_str).style(Style::default().fg(fg_color)),
                        Cell::from(lat_str),
                        Cell::from(vis_str).style(Style::default().fg(fg_color)),
                    ])
                });

                let t = Table::new(
                    rows,
                    [
                        Constraint::Percentage(40),
                        Constraint::Percentage(20),
                        Constraint::Percentage(15),
                        Constraint::Percentage(25),
                    ],
                )
                .header(header)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Latency [Esc to Menu, 't' to Test All, 's' to Sort]"),
                )
                .row_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
                .highlight_symbol(">> ");

                let mut list_state = state.list_state.clone();
                f.render_stateful_widget(t, chunks[1], &mut list_state);
            }
            NetTestTab::Analysis => {
                let analysis_chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
                    .split(chunks[1]);

                let res = &state.analysis_result;
                let mut ip_text = String::new();
                ip_text.push_str("[Direct IP / Domestic View]\n");
                if let Some(ref ip) = res.direct_ip {
                    ip_text.push_str(&format!("  IP: {}  Region: {}\n\n", ip.ip, ip.region));
                } else if state.analysis_testing {
                    ip_text.push_str("  Testing...\n\n");
                } else {
                    ip_text.push_str("  Failed or not tested\n\n");
                }

                ip_text.push_str("[Proxy IP / International View]\n");
                if let Some(ref ip) = res.proxy_ip {
                    ip_text.push_str(&format!("  IP: {}  Region: {}\n\n", ip.ip, ip.region));
                } else if state.analysis_testing {
                    ip_text.push_str("  Testing...\n\n");
                } else {
                    ip_text.push_str("  Failed or not tested\n\n");
                }

                let ip_p =
                    Paragraph::new(ip_text).block(Block::default().borders(Borders::ALL).title("IP Information"));
                f.render_widget(ip_p, analysis_chunks[0]);

                let format_status = |name: &str, status: &UnlockStatus| -> String {
                    let s = match status {
                        UnlockStatus::Testing => "Testing...".to_string(),
                        UnlockStatus::Unlocked(reg) => format!("OK Unlocked ({})", reg),
                        UnlockStatus::OriginalsOnly => "WARN Originals Only".to_string(),
                        UnlockStatus::Blocked(r) => format!("NO Blocked ({})", r),
                        UnlockStatus::Error(e) => format!("ERR {}", e),
                    };
                    format!("{:<12} {}", name, s)
                };

                let stream_text = [
                    format_status("[YouTube]", &res.youtube_status),
                    format_status("[Netflix]", &res.netflix_status),
                    format_status("[Spotify]", &res.spotify_status),
                    format_status("[Bilibili]", &res.bilibili_status),
                ]
                .join("\n");

                let stream_p = Paragraph::new(stream_text).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Streaming Services Unlock Status"),
                );
                f.render_widget(stream_p, analysis_chunks[1]);
            }
        }

        let mut footer_text = msg.clone().unwrap_or_else(|| "NetTest Manager".to_string());
        if state.is_testing || state.analysis_testing {
            footer_text = format!("{} [Testing in progress...]", footer_text);
        }
        let footer = Paragraph::new(footer_text).block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(footer, chunks[2]);
    }
}
