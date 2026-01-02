use reqwest::Client;
use tokio::runtime::Runtime;
use crate::error::Result;


#[derive(clap::Parser, Debug)]
pub struct ProxiesCmd;


impl ProxiesCmd {
    fn get(&self, unix_sock: &str) -> Result<()> {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = Client::builder().unix_socket(unix_sock).build()?;

            let response = client.get("http://localhost/proxies").send().await?;
            println!("{}", response.error_for_status()?.text().await?);
            Ok(())
        })
    }
}


