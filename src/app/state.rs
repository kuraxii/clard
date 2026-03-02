use tokio::sync::mpsc::UnboundedSender;

use crate::event::ClardEvent;

#[derive(Debug)]
pub struct MenuState {
    current_item: MenuItem,
}

impl MenuState {
    pub fn init() -> Self {
        Self {
            current_item: MenuItem::Preview,
        }
    }

    pub fn current(&self) -> MenuItem {
        self.current_item
    }

    pub fn next(&mut self) {
        self.current_item = self.current_item.next()
    }

    pub fn prev(&mut self) {
        self.current_item = self.current_item.prev()
    }

    pub fn first(&mut self) {
        self.current_item = MenuItem::first();
    }

    pub fn last(&mut self) {
        self.current_item = MenuItem::last();
    }

    pub fn on_char(&self, char: char, event_sender: UnboundedSender<ClardEvent>) {
        match char {
            'q' => {
                let _ = event_sender.send(ClardEvent::Terminal);
            }
            _ => {}
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MenuItem {
    Preview,
    Proxy,
    Connections,
    Rules,
    NetTest,
}

impl MenuItem {
    pub const ALL: [Self; 5] = [
        Self::Preview,
        Self::Proxy,
        Self::Connections,
        Self::Rules,
        Self::NetTest,
    ];
    pub fn next(&self) -> Self {
        match self {
            Self::Preview => Self::Proxy,
            Self::Proxy => Self::Connections,
            Self::Connections => Self::Rules,
            Self::Rules => Self::NetTest,
            Self::NetTest => Self::NetTest, // 或者循环到 Preview
        }
    }
    pub fn prev(&self) -> Self {
        match self {
            Self::Preview => Self::Preview,
            Self::Proxy => Self::Preview,
            Self::Connections => Self::Proxy,
            Self::Rules => Self::Connections,
            Self::NetTest => Self::Rules,
        }
    }

    pub fn first() -> Self {
        *Self::ALL.first().unwrap()
    }

    pub fn last() -> Self {
        *Self::ALL.last().unwrap()
    }

    pub fn to_str(&self) -> &str {
        match self {
            MenuItem::Preview => "Preview",
            MenuItem::Proxy => "Proxy",
            MenuItem::Connections => "Connections",
            MenuItem::Rules => "Rules",
            MenuItem::NetTest => "NetTest",
        }
    }
}

