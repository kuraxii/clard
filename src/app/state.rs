use tokio::sync::mpsc::UnboundedSender;

use crate::event::ClardEvent;

#[derive(Debug)]
pub struct MenuState {
    current_item: MenuItem,
}

impl MenuState {
    pub fn init() -> Self {
        Self {
            current_item: MenuItem::Proxy,
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
        let _ = (char, event_sender);
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MenuItem {
    Proxy,
    Connections,
    NetTest,
}

impl MenuItem {
    pub const ALL: [Self; 3] = [Self::Proxy, Self::Connections, Self::NetTest];
    pub fn next(&self) -> Self {
        match self {
            Self::Proxy => Self::Connections,
            Self::Connections => Self::NetTest,
            Self::NetTest => Self::NetTest,
        }
    }
    pub fn prev(&self) -> Self {
        match self {
            Self::Proxy => Self::Proxy,
            Self::Connections => Self::Proxy,
            Self::NetTest => Self::Connections,
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
            MenuItem::Proxy => "Proxy",
            MenuItem::Connections => "Connections",
            MenuItem::NetTest => "NetTest",
        }
    }
}
