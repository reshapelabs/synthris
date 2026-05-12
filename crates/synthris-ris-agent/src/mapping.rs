use anyhow::{Result, anyhow};
use synthris_core::{
    BackgroundMode, CfuSpec, LookPreset, OpacityClass, PhasePreset, SimulationRequest, TemperatureSpec,
    TimeSpec,
};
use uuid::Uuid;

use crate::config::AgentConfig;
use crate::job_parser::parse_job_name_overrides;
use crate::ris_client::RisJob;

pub const PRESET_FRONTLIT: Uuid = Uuid::from_u128(0x00000000000000000000000000000000);
pub const PRESET_BACKLIT: Uuid = Uuid::from_u128(0x11111111111111111111111111111111);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IlluminationSelection {
    Frontlit,
    Backlit,
}

#[derive(Debug, Clone)]
pub struct JobResolvedParams {
    pub cfu: CfuSpec,
    pub base_seed: u64,
    pub temperature_c: f32,
    pub organism_id: String,
    pub respect_capture_interval: bool,
}

pub fn map_preset_to_illumination(preset_id: Uuid) -> Result<IlluminationSelection> {
    if preset_id == PRESET_FRONTLIT {
        Ok(IlluminationSelection::Frontlit)
    } else if preset_id == PRESET_BACKLIT {
        Ok(IlluminationSelection::Backlit)
    } else {
        Err(anyhow!("unknown camera preset id: {preset_id}"))
    }
}

pub fn resolve_job_params(job: &RisJob, config: &AgentConfig) -> Result<JobResolvedParams> {
    let overrides = parse_job_name_overrides(job.name.as_deref().unwrap_or(""))?;

    let cfu = overrides.cfu.unwrap_or(CfuSpec::Exact(50));
    let base_seed = overrides.seed.unwrap_or_else(|| seed_from_uuid(job.id));
    let organism_id = overrides.organism_id.unwrap_or_else(|| "zenth".to_string());
    let respect_capture_interval = overrides.respect_capture_interval.unwrap_or(false);

    let temperature_c = match job.setpoint_temperature {
        Some(v) if v.is_finite() => v,
        _ => config.default_temperature_c,
    };

    Ok(JobResolvedParams {
        cfu,
        base_seed,
        temperature_c,
        organism_id,
        respect_capture_interval,
    })
}

pub fn build_simulation_request(
    organism_id: &str,
    illumination_id: &str,
    params: &JobResolvedParams,
    time: TimeSpec,
    seed: u64,
    width: u32,
    height: u32,
    render_scale: f32,
) -> SimulationRequest {
    SimulationRequest {
        organism_id: organism_id.to_string(),
        illumination_id: illumination_id.to_string(),
        background_mode: BackgroundMode::PlateImage,
        cfu: params.cfu.clone(),
        time,
        temperature: TemperatureSpec {
            constant_c: params.temperature_c,
        },
        phase: PhasePreset::Mid,
        look: LookPreset::Realistic,
        opacity_class: OpacityClass::Standard,
        seed,
        width,
        height,
        show_colony_ids: false,
        render_scale,
    }
}

pub fn build_time_spec(capture_interval_s: u64, image_count: u32) -> TimeSpec {
    let count = image_count.max(1) as u64;
    let step = capture_interval_s.max(1);
    let duration = step.saturating_mul(count.saturating_sub(1));

    TimeSpec {
        start_after_seconds: 0,
        duration_seconds: duration.max(1),
        step_seconds: step,
    }
}

pub fn seed_from_uuid(id: Uuid) -> u64 {
    let bytes = id.as_bytes();
    let mut h = 0xcbf29ce484222325u64;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn derive_plate_seed(base_seed: u64, plate_id: Uuid) -> u64 {
    base_seed ^ seed_from_uuid(plate_id)
}

#[cfg(test)]
mod tests {
    use super::{build_time_spec, resolve_job_params, seed_from_uuid};
    use crate::config::AgentConfig;
    use crate::ris_client::RisJob;
    use uuid::Uuid;

    fn cfg() -> AgentConfig {
        AgentConfig {
            ris_api_base_url: "http://localhost:8080".to_string(),
            ris_api_key: "x".to_string(),
            risfw_version: "1".to_string(),
            rishw_version: "2".to_string(),
            poll_interval: std::time::Duration::from_secs(2),
            default_temperature_c: 22.0,
            default_scale_factor: 1.0,
            s3_annotation_endpoint: None,
            s3_annotation_bucket: None,
        }
    }

    fn job(temp: Option<f32>) -> RisJob {
        RisJob {
            id: Uuid::parse_str("f295a49c-f91f-4ede-aec2-08e8ab5f4b84").expect("uuid"),
            name: Some("sample cfu=75 seed=44 organism=morrow".to_string()),
            status: Some("pending".to_string()),
            run_duration: 3600,
            capture_interval: 60,
            image_count: 3,
            setpoint_temperature: temp,
            camera_preset_id_1: None,
            camera_preset_id_2: None,
            camera_preset_id_3: None,
            camera_preset_id_4: None,
            camera_preset_id_5: None,
        }
    }

    #[test]
    fn uses_setpoint_temperature_when_present() {
        let resolved = resolve_job_params(&job(Some(31.5)), &cfg()).expect("resolve");
        assert!((resolved.temperature_c - 31.5).abs() < f32::EPSILON);
    }

    #[test]
    fn falls_back_to_default_temperature() {
        let resolved = resolve_job_params(&job(None), &cfg()).expect("resolve");
        assert!((resolved.temperature_c - 22.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolves_organism_override() {
        let mut j = job(Some(28.0));
        j.name = Some("sample org=ecoli pace=1".to_string());
        let resolved = resolve_job_params(&j, &cfg()).expect("resolve");
        assert_eq!(resolved.organism_id, "ecoli");
        assert!(resolved.respect_capture_interval);
    }

    #[test]
    fn falls_back_to_zenth_when_no_organism_override() {
        let mut j = job(Some(28.0));
        j.name = Some("sample cfu=75 seed=44".to_string());
        let resolved = resolve_job_params(&j, &cfg()).expect("resolve");
        assert_eq!(resolved.organism_id, "zenth");
    }

    #[test]
    fn uuid_seed_is_stable() {
        let id = Uuid::parse_str("f295a49c-f91f-4ede-aec2-08e8ab5f4b84").expect("uuid");
        assert_eq!(seed_from_uuid(id), seed_from_uuid(id));
    }

    #[test]
    fn build_time_spec_handles_single_image() {
        let spec = build_time_spec(300, 1);
        assert_eq!(spec.step_seconds, 300);
        assert_eq!(spec.duration_seconds, 1);
    }
}
