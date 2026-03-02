use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{
    APP, WindowState,
    state::MenuItem,
    proxy::{ProxyState, ProxyFocus},
    preview::PreviewState,
    connections::ConnectionsState,
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
                _ => {
                    let p = Paragraph::new(format!("Not implemented yet. Press [Esc] to go back.\nMessage: {:?}", app.message))
                        .block(Block::default().borders(Borders::ALL).title("Info"));
                    f.render_widget(p, area);
                }
            }
        });
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
            .constraints([
                Constraint::Length(5),
                Constraint::Length(7),
                Constraint::Min(0),
            ].as_ref())
            .split(area);

        // Version Block
        let version_text = match &state.version {
            Some(v) => format!("Version: {}\nMeta: {}", v.version, v.meta),
            None => "Loading version...".to_string(),
        };
        let version_p = Paragraph::new(version_text)
            .block(Block::default().borders(Borders::ALL).title("Core Version"));
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
            None => "Loading traffic data...".to_string(),
        };
        let conn_p = Paragraph::new(conn_text)
            .block(Block::default().borders(Borders::ALL).title("Traffic & System [Esc to Menu]"));
        f.render_widget(conn_p, chunks[1]);

        // Message Block
        let msg_text = msg.clone().unwrap_or_else(|| "Press [Esc] to return to Menu".to_string());
        let msg_p = Paragraph::new(msg_text)
            .block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(msg_p, chunks[2]);
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
            .style(if state.focus == ProxyFocus::Groups { Style::default().fg(Color::Yellow) } else { Style::default() });

        let group_items: Vec<ListItem<'_>> = state.groups.iter().map(|g| {
            ListItem::new(Line::from(vec![
                Span::raw(g.name.clone()),
                Span::raw(format!(" [{}]", g.now.clone().unwrap_or_default())),
            ]))
        }).collect();

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
            .style(if state.focus == ProxyFocus::Proxies { Style::default().fg(Color::Yellow) } else { Style::default() });

        let mut proxy_items = Vec::new();
        if let Some(g_idx) = state.group_list_state.selected() {
            if let Some(group) = state.groups.get(g_idx) {
                if let Some(all) = &group.all {
                    for node in all {
                        let is_now = group.now.as_deref() == Some(node);
                        let text = if is_now {
                            Span::styled(format!("* {}", node), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
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
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::ALL).title("Status"));
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

        let items: Vec<ListItem<'_>> = state.connections.iter().map(|c| {
            let host = if c.metadata.host.is_empty() {
                c.metadata.destination_ip.clone()
            } else {
                c.metadata.host.clone()
            };
            let info = format!(
                "[{:?}] {}:{} -> {}:{} | U:{} D:{} | {}",
                c.metadata.network,
                c.metadata.source_ip,
                c.metadata.source_port,
                host,
                c.metadata.destination_port,
                c.upload,
                c.download,
                c.rule
            );
            ListItem::new(info)
        }).collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Connections [Esc to Menu, 'x' or 'd' to close connection]");
            
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::DarkGray))
            .highlight_symbol(">> ");
            
        let mut list_state = state.list_state.clone();
        f.render_stateful_widget(list, chunks[0], &mut list_state);

        let footer_text = msg.clone().unwrap_or_else(|| "Connections Manager".to_string());
        let footer = Paragraph::new(footer_text)
            .block(Block::default().borders(Borders::ALL).title("Status"));
        f.render_widget(footer, chunks[1]);
    }
}
