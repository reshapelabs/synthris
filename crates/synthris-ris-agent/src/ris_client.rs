use anyhow::{Context, Result, anyhow};
use reqwest::header::{HeaderMap, HeaderValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::config::AgentConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct RisJob {
    pub id: Uuid,
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "de_u64_lossy")]
    pub run_duration: u64,
    #[serde(default, deserialize_with = "de_u64_lossy")]
    pub capture_interval: u64,
    #[serde(default, deserialize_with = "de_u32_lossy")]
    pub image_count: u32,
    pub setpoint_temperature: Option<f32>,
    pub camera_preset_id_1: Option<Uuid>,
    pub camera_preset_id_2: Option<Uuid>,
    pub camera_preset_id_3: Option<Uuid>,
    pub camera_preset_id_4: Option<Uuid>,
    pub camera_preset_id_5: Option<Uuid>,
}

impl RisJob {
    pub fn is_pending(&self) -> bool {
        self.status
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("pending"))
            .unwrap_or(false)
    }

    pub fn is_started(&self) -> bool {
        self.status
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("started"))
            .unwrap_or(false)
    }

    pub fn camera_preset_ids(&self) -> Vec<Uuid> {
        [
            self.camera_preset_id_1,
            self.camera_preset_id_2,
            self.camera_preset_id_3,
            self.camera_preset_id_4,
            self.camera_preset_id_5,
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RisPlate {
    pub id: Uuid,
    pub plate_type_id: String,
    pub wells: Vec<RisWell>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RisWell {
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
struct JobStatusUpdate {
    id: Uuid,
    status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlateImageAdd {
    pub job_id: Uuid,
    pub captured_at: String,
    pub plate_id: Uuid,
    pub well: Well,
    pub well_id: Option<Uuid>,
    pub camera_preset_id: Uuid,
    pub time_index: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Well {
    pub col: i32,
    pub row: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlateImageOut {
    pub id: Uuid,
    pub image_url: String,
}

#[derive(Debug, Clone)]
pub struct RisClient {
    http: reqwest::Client,
    base_url: String,
}

impl RisClient {
    pub fn new(config: &AgentConfig) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "RISFW-Version",
            HeaderValue::from_str(&config.risfw_version)
                .context("invalid RISFW_VERSION header value")?,
        );
        headers.insert(
            "RISHW-Version",
            HeaderValue::from_str(&config.rishw_version)
                .context("invalid RISHW_VERSION header value")?,
        );
        headers.insert(
            "X-API-Key",
            HeaderValue::from_str(&config.ris_api_key)
                .context("invalid RIS_API_KEY header value")?,
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("failed to build reqwest client")?;

        Ok(Self {
            http,
            base_url: config.ris_api_base_url.trim_end_matches('/').to_string(),
        })
    }

    pub async fn get_jobs(&self) -> Result<Vec<RisJob>> {
        let url = format!("{}/jobs/", self.base_url);
        let res = self.http.get(url).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("get_jobs failed: status={status} body={body}"));
        }
        let body = res.text().await?;
        decode_items::<RisJob>(&body, "jobs")
    }

    pub async fn get_job(&self, job_id: Uuid) -> Result<RisJob> {
        let url = format!("{}/jobs/{job_id}", self.base_url);
        let res = self.http.get(url).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("get_job failed: status={status} body={body}"));
        }
        Ok(res.json::<RisJob>().await?)
    }

    pub async fn update_job_status(&self, job_id: Uuid, status: &str) -> Result<()> {
        let url = format!("{}/jobs/{}", self.base_url, job_id);
        let payload = JobStatusUpdate {
            id: job_id,
            status: status.to_string(),
        };
        let res = self.http.put(url).json(&payload).send().await?;
        if !res.status().is_success() {
            let status_code = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "update_job_status failed: status={status_code} body={body}"
            ));
        }
        Ok(())
    }

    pub async fn get_plates(&self, job_id: Uuid) -> Result<Vec<RisPlate>> {
        let url = format!("{}/plates/", self.base_url);
        let res = self
            .http
            .get(url)
            .query(&[("job_id", job_id.to_string())])
            .send()
            .await?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("get_plates failed: status={status} body={body}"));
        }
        let body = res.text().await?;
        decode_items::<RisPlate>(&body, "plates")
    }

    pub async fn add_plate_image_metadata(&self, body: &PlateImageAdd) -> Result<PlateImageOut> {
        let primary_url = format!("{}/plates/images/", self.base_url);
        let primary_res = self.http.post(primary_url).json(body).send().await?;
        if primary_res.status().is_success() {
            return Ok(primary_res.json::<PlateImageOut>().await?);
        }
        let primary_status = primary_res.status();
        let primary_body = primary_res.text().await.unwrap_or_default();

        // Only try fallback for older deployments where /plates/images/ does not exist.
        if primary_status != reqwest::StatusCode::NOT_FOUND {
            return Err(anyhow!(
                "add_plate_image_metadata primary endpoint failed: status={primary_status} body={primary_body}"
            ));
        }

        // Backward compatibility for older API deployments.
        let fallback_url = format!("{}/plates/image-metadata", self.base_url);
        let fallback_res = self.http.post(fallback_url).json(body).send().await?;
        if !fallback_res.status().is_success() {
            let fallback_status = fallback_res.status();
            let fallback_body = fallback_res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "add_plate_image_metadata failed: primary_status={primary_status} primary_body={primary_body}; fallback_status={fallback_status} fallback_body={fallback_body}"
            ));
        }
        Ok(fallback_res.json::<PlateImageOut>().await?)
    }

    pub async fn upload_jpeg_to_signed_url(
        &self,
        signed_url: &str,
        jpeg_bytes: &[u8],
    ) -> Result<()> {
        let res = self
            .http
            .put(signed_url)
            .header("content-type", "image/jpeg")
            .body(jpeg_bytes.to_vec())
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "upload to signed url failed: status={status} body={body}"
            ));
        }

        Ok(())
    }

    pub async fn upload_annotation(
        &self,
        endpoint: &str,
        bucket: &str,
        plate_image_id: Uuid,
        body: &[u8],
    ) -> Result<()> {
        let url = format!(
            "{}/{}/{}",
            endpoint.trim_end_matches('/'),
            bucket,
            plate_image_id
        );
        let res = reqwest::Client::new()
            .put(&url)
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status();
            let resp_body = res.text().await.unwrap_or_default();
            return Err(anyhow!(
                "upload annotation failed: status={status} body={resp_body}"
            ));
        }
        Ok(())
    }
}

fn decode_items<T>(body: &str, label: &str) -> Result<Vec<T>>
where
    T: DeserializeOwned,
{
    let value: serde_json::Value =
        serde_json::from_str(body).with_context(|| format!("invalid {label} json payload"))?;

    let items = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("items").and_then(|v| v.as_array()) {
        arr.clone()
    } else if let Some(arr) = value.get("data").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        return Err(anyhow!(
            "unexpected {label} payload shape; expected array or object with items/data array"
        ));
    };

    let mut decoded = Vec::with_capacity(items.len());
    for item in items {
        match serde_json::from_value::<T>(item.clone()) {
            Ok(v) => decoded.push(v),
            Err(err) => warn!(error = %err, "{label}: skipping malformed item"),
        }
    }

    Ok(decoded)
}

fn de_u64_lossy<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => 0,
        Some(serde_json::Value::Number(n)) => parse_number_to_u64(&n).unwrap_or(0),
        Some(serde_json::Value::String(s)) => parse_string_to_u64(&s).unwrap_or(0),
        _ => 0,
    })
}

fn de_u32_lossy<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => 0,
        Some(serde_json::Value::Number(n)) => parse_number_to_u64(&n)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
        Some(serde_json::Value::String(s)) => parse_string_to_u64(&s)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0),
        _ => 0,
    })
}

fn parse_number_to_u64(n: &serde_json::Number) -> Option<u64> {
    n.as_u64().or_else(|| {
        n.as_f64().and_then(|f| {
            if f.is_finite() && f >= 0.0 {
                Some(f.round() as u64)
            } else {
                None
            }
        })
    })
}

fn parse_string_to_u64(s: &str) -> Option<u64> {
    s.parse::<u64>().ok().or_else(|| {
        s.parse::<f64>().ok().and_then(|f| {
            if f.is_finite() && f >= 0.0 {
                Some(f.round() as u64)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::RisJob;
    use serde::Deserialize;
    use serde_json::json;
    use uuid::Uuid;

    #[derive(Debug, Deserialize)]
    struct ParseProbe {
        #[serde(deserialize_with = "super::de_u64_lossy")]
        v64: u64,
        #[serde(deserialize_with = "super::de_u32_lossy")]
        v32: u32,
    }

    #[test]
    fn lossy_number_parsing_accepts_integral_floats() {
        let probe: ParseProbe = serde_json::from_value(json!({
            "v64": 60.0,
            "v32": 48.0
        }))
        .expect("probe");
        assert_eq!(probe.v64, 60);
        assert_eq!(probe.v32, 48);
    }

    #[test]
    fn lossy_number_parsing_accepts_float_strings() {
        let probe: ParseProbe = serde_json::from_value(json!({
            "v64": "120.0",
            "v32": "24.0"
        }))
        .expect("probe");
        assert_eq!(probe.v64, 120);
        assert_eq!(probe.v32, 24);
    }

    #[test]
    fn started_status_matches_only_started() {
        let id = Uuid::nil();
        let mk = |status: &str| RisJob {
            id,
            name: None,
            status: Some(status.to_string()),
            run_duration: 0,
            capture_interval: 0,
            image_count: 0,
            setpoint_temperature: None,
            camera_preset_id_1: None,
            camera_preset_id_2: None,
            camera_preset_id_3: None,
            camera_preset_id_4: None,
            camera_preset_id_5: None,
        };
        assert!(!mk("Pending").is_started());
        assert!(mk("started").is_started());
        assert!(!mk("Finished").is_started());
    }
}
