

use serde::{Deserialize, Serialize};
use crate::error::Result;


#[derive(clap::Parser, Debug)]
pub struct BackendVersionCmd;

impl BackendVersionCmd {
    fn get(&self, unix_sock: &str) -> Result<()> {

        // let url = http::get_http_url("version");
        // let bv = http::blocking::get_uds::<BackendVersion>(unix_sock, &url);
        // println!("version: {:?}", bv);

        Ok(())
    }
}


///
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BackendVersion {
    pub meta: bool,
    pub version: String,
}
