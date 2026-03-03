use crate::ipc::models::{BackendVersion, Connections};

#[derive(Debug)]
pub struct PreviewState {
    pub version: Option<BackendVersion>,
    pub connections: Option<Connections>,
}

impl PreviewState {
    pub fn new() -> Self {
        Self {
            version: None,
            connections: None,
        }
    }

    pub fn update_version(&mut self, version: BackendVersion) {
        self.version = Some(version);
    }

    pub fn update_connections(&mut self, connections: Connections) {
        self.connections = Some(connections);
    }
}
