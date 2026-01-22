//! `start` subcommand - example of how to write a subcommand

use std::collections::HashMap;


use serde::{Deserialize, Serialize};

use crate::{error::Result };
/// Groups 子命
///
/// clash 所有的代理组
#[derive(clap::Parser, Debug)]
pub struct GroupsCmd;

#[derive(clap::Subcommand, Debug)]
pub enum ProxiesAction {
    /// 列表所有代理
    List,
    /// 设置某个代理: `proxies set NAME [VALUE]`
    Set { name: String, value: Option<String> },
}

impl GroupsCmd {
    async fn get(&self) -> Result<()> {

        // let response = APP
        //     .backend
        //     .request(Method::GET, url)?
        //     .send()
        //     .await?
        //     .error_for_status()?;
        // let body = response.text().await?;
        // let json_config: Config = serde_json::from_str(&body).map_err(|err| {
        //     eprintln!("failed to parse response: {err}");
        //     eprintln!("raw response:\n{body}");
        //     err
        // })?;

        // let pretty_json = serde_json::to_string_pretty(&json_config)?;

        // println!("group:\n{}", pretty_json);
        Ok(())
    }
}



