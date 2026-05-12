mod config;
mod job_parser;
mod mapping;
mod ris_client;
mod runner;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let config = config::AgentConfig::from_env()?;
    runner::run_agent(config).await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("synthris_ris_agent=info,synthris_core=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
