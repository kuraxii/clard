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
        Self::ALL
            .iter()
            .position(|&x| x == *self)
            .and_then(|position| Self::ALL.get(position + 1))
            .cloned()
            .unwrap_or(*self)
    }
    pub fn prev(&self) -> Self {
        Self::ALL
            .iter()
            .position(|&x| x == *self)
            .and_then(|position| Self::ALL.get(position - 1))
            .cloned()
            .unwrap_or(*self)
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
