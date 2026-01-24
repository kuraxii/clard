mod state;

use std::default;

use tokio::sync::mpsc;
/// WindowState
/// 用于记录窗口的状态，MENU、Preview、PROXY、CONNECTIONS、RULE、TEST
enum WindowState {
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
    NetTest
}





pub struct APP{
    window_state: WindowState,
}

impl APP {
    pub fn init() -> Self{

        APP { window_state: WindowState::Memu }
    }
}