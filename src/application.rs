//! ClardRs Abscissa Application

use crate::ipc::backend::{Backend, Protocol};
use crate::{commands::EntryPoint, config::ClardRsConfig};

/// ClardRs Application
#[derive(Debug)]
pub struct ClardRsApp {
    config: ClardRsConfig,
    pub backend: Backend,
}

impl ClardRsApp {

}

impl Default for ClardRsApp {
    fn default() -> Self {
        Self {
            backend: Backend::builder()
                .set_unix_socket("/tmp/verge/verge-mihomo.sock")
                .build()
                .unwrap(),
            config: ClardRsConfig::default(),
        }
    }
}
