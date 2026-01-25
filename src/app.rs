pub mod state;
use state::MenuState;
/// WindowState
/// 用于记录窗口的状态，MENU、Preview、PROXY、CONNECTIONS、RULE、TEST
#[derive(Debug, PartialEq, Eq)]
pub enum WindowState {
    /// 菜单页面
    Memu,
    /// 预览页面
    Preview,
    /// 代理查看选择页面
    Proxy,
    /// 连接流量统计页面
    Connects,
    /// 规则页面
    Rules,
    /// ip 测试页面
    NetTest,
}

#[derive(Debug)]
pub struct APP {
    pub current_page: WindowState,
    pub menusate: MenuState,
}

impl APP {
    pub fn init() -> Self {
        APP {
            current_page: WindowState::Memu,
            menusate: MenuState::init(),
        }
    }

    pub fn on_up_key(&mut self) {
        match self.current_page {
            WindowState::Memu => self.menusate.prev(),
            WindowState::Preview => todo!(),
            WindowState::Proxy => todo!(),
            WindowState::Connects => todo!(),
            WindowState::Rules => todo!(),
            WindowState::NetTest => todo!(),
        }
    }

    pub fn on_down_key(&mut self) {
        match self.current_page {
            WindowState::Memu => self.menusate.next(),
            WindowState::Preview => todo!(),
            WindowState::Proxy => todo!(),
            WindowState::Connects => todo!(),
            WindowState::Rules => todo!(),
            WindowState::NetTest => todo!(),
        }
    }

    pub fn on_left_key(&mut self) {}

    pub fn on_right_key(&mut self) {}

    pub fn on_home_key(&mut self) {}

    pub fn on_end_key(&mut self) {}

    pub fn on_pagedown_key(&mut self) {}

    pub fn on_pageup_key(&mut self) {}

    pub fn on_backspace_key(&mut self) {}

    pub fn on_delete_key(&mut self) {}

    pub fn on_tab_key(&mut self) {}

    pub fn on_esc_key(&mut self) {}

    pub fn on_enter_key(&mut self) {}

    pub fn on_char(&mut self, char: char) {}
}
