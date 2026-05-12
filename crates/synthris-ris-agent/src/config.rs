use std::env;
use std::time::Duration;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub ris_api_base_url: String,
    pub ris_api_key: String,
    pub risfw_version: String,
    pub rishw_version: String,
    pub poll_interval: Duration,
    pub default_temperature_c: f32,
    pub default_scale_factor: f32,
    pub s3_annotation_endpoint: Option<String>,
    pub s3_annotation_bucket: Option<String>,
}

impl AgentConfig {
    pub fn from_env() -> Result<Self> {
        let ris_api_base_url = env::var("RIS_API_BASE_URL")
            .context("RIS_API_BASE_URL environment variable is required")?;
        let ris_api_key =
            env::var("RIS_API_KEY").context("RIS_API_KEY environment variable is required")?;
        let risfw_version =
            env::var("RISFW_VERSION").context("RISFW_VERSION environment variable is required")?;
        let rishw_version =
            env::var("RISHW_VERSION").context("RISHW_VERSION environment variable is required")?;

        let poll_interval_seconds = env::var("POLL_INTERVAL_SECONDS")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(2.0);

        let default_temperature_c = env::var("DEFAULT_TEMPERATURE_C")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(22.0);
        let default_scale_factor = env::var("DEFAULT_SCALE_FACTOR")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.05, 1.0);

        let s3_annotation_endpoint = env::var("S3_ANNOTATION_ENDPOINT").ok();
        let s3_annotation_bucket = env::var("S3_ANNOTATION_BUCKET").ok();

        Ok(Self {
            ris_api_base_url,
            ris_api_key,
            risfw_version,
            rishw_version,
            poll_interval: Duration::from_secs_f64(poll_interval_seconds.max(0.1)),
            default_temperature_c,
            default_scale_factor,
            s3_annotation_endpoint,
            s3_annotation_bucket,
        })
    }
}
