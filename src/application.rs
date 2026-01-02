//! ClardRs Abscissa Application


use crate::{commands::EntryPoint, config::ClardRsConfig};
use crate::ipc::backend::{Backend, Protocol};


/// ClardRs Application
#[derive(Debug)]
pub struct ClardRsApp {
    config: ClardRsConfig,
    pub backend: Backend
}


impl Default for ClardRsApp {
    fn default() -> Self {
        Self {
            backend: Backend::builder().set_unix_socket("/tmp/verge/verge-mihomo.sock").build().unwrap(),
            config: ClardRsConfig::default()
        }
    }
}