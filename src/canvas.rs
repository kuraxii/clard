use std::default;

use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{
    APP, WindowState,
    state::{MenuItem, MenuState},
};

#[derive(Debug, Default)]
pub struct Painter;
impl Painter {
    pub fn draw(&mut self, terminal: &mut Terminal<impl Backend>, app: &APP) {
        let _ = terminal.draw(|f| {
            let area = f.area();
            let _ = match app.current_page {
                WindowState::Memu => {
                    MenuLayout::draw_muen(f, area, app);
                }
                _ => {}
            };
        });
    }
}

struct MenuLayout;
impl MenuLayout {
    fn draw_muen(f: &mut Frame<'_>, area: Rect, app: &APP) {
        let iterms: Vec<ListItem<'_>> = MenuItem::ALL
            .iter()
            .map(|mi| {
                let label = mi.to_str();
                if app.menusate.current() == *mi {
                    ListItem::new(label).style(Style::default().add_modifier(Modifier::BOLD))
                } else {
                    ListItem::new(label)
                }
            })
            .collect();
        let menu_list = List::new(iterms).block(Block::default().borders(Borders::ALL).title("菜单 [Tab]"));
        f.render_widget(menu_list, area);
    }
}
