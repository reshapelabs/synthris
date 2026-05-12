use anyhow::{Context, Result, anyhow, bail};
use std::process::Command;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{info, warn};

use serde::Serialize;
use synthris_core::{
    CfuSpec, ColonyAnnotation, Engine, EngineConfig, GeneratedFrame, ProfileDb, ProfileDbConfig,
    TimeSpec,
};

use crate::config::AgentConfig;
use crate::mapping::{
    IlluminationSelection, build_simulation_request, build_time_spec, derive_plate_seed,
    map_preset_to_illumination, resolve_job_params, resolve_plate_profile_id,
};
use crate::ris_client::{PlateImageAdd, RisClient, RisJob, Well};

const RUNNING_JOB_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn run_agent(config: AgentConfig) -> Result<()> {
    let client = RisClient::new(&config)?;
    let db = ProfileDb::load(&ProfileDbConfig::default())?;
    let engine = Engine::new(EngineConfig::default());
    fail_started_jobs_on_boot(&client).await?;

    loop {
        tokio::time::sleep(config.poll_interval).await;

        let jobs = match client.get_jobs().await {
            Ok(v) => v,
            Err(err) => {
                warn!(error = %err, "failed to poll jobs");
                continue;
            }
        };

        info!(job_count = jobs.len(), "polled jobs");

        if let Some(job) = jobs.into_iter().find(RisJob::is_pending) {
            process_single_job(&client, &db, &engine, &config, &job).await?;
        }
    }
}

async fn process_single_job(
    client: &RisClient,
    db: &ProfileDb,
    engine: &Engine,
    config: &AgentConfig,
    job: &RisJob,
) -> Result<()> {
    info!(job_id = %job.id, job_name = ?job.name, "starting job");

    client.update_job_status(job.id, "Started").await?;

    enum JobProcessOutcome {
        Finished,
        StoppedByUser,
    }

    let mut stop_check = JobStopMonitor::new();
    let process_result: Result<JobProcessOutcome> = async {
        let plates = client.get_plates(job.id).await?;
        if plates.is_empty() {
            bail!("no plates found for job {}", job.id);
        }

        let params = resolve_job_params(job, config)?;
        validate_requested_organism(db, &params.organism_id)?;
        if job.setpoint_temperature.is_none() {
            warn!(
                job_id = %job.id,
                default_temp_c = config.default_temperature_c,
                "setpoint_temperature missing; using fallback"
            );
        }

        let presets = resolve_presets(job)?;
        let time_spec = build_time_spec(job.capture_interval, job.image_count);
        let expected_frames = job.image_count.max(1) as usize;
        let expected_duration = job
            .capture_interval
            .saturating_mul((job.image_count.max(1) as u64).saturating_sub(1));
        if job.run_duration > 0 && job.run_duration != expected_duration {
            warn!(
                job_id = %job.id,
                run_duration = job.run_duration,
                timeline_duration = expected_duration,
                "run_duration differs from image_count/capture_interval timeline"
            );
        }

        info!(
            job_id = %job.id,
            organism = %params.organism_id,
            cfu = %cfu_spec_label(&params.cfu),
            seed = params.base_seed,
            temperature_c = params.temperature_c,
            scale_factor = config.default_scale_factor,
            pace_to_capture_interval = params.respect_capture_interval,
            image_count = expected_frames,
            capture_interval_s = job.capture_interval.max(1),
            timeline_duration_s = time_spec.duration_seconds,
            timeline_step_s = time_spec.step_seconds,
            presets = presets.len(),
            plates = plates.len(),
            "job parameters resolved"
        );

        let total_images =
            expected_frames * presets.len() * plates.iter().map(|p| p.wells.len()).sum::<usize>();
        let mut current_image = 0usize;
        let schedule_start = Instant::now();

        struct RenderStream {
            plate_id: uuid::Uuid,
            preset_id: uuid::Uuid,
            well_ids: Vec<uuid::Uuid>,
            frames: synthris_core::engine::FrameIterator,
        }

        let mut streams: Vec<RenderStream> = Vec::new();
        for preset_id in &presets {
            for plate in &plates {
                let illumination = map_preset_to_illumination(*preset_id)?;
                let plate_profile_id =
                    resolve_plate_profile_id(&plate.plate_type_id, illumination)?;
                let illumination_id = illumination_profile_id(illumination);
                let plate_seed = derive_plate_seed(params.base_seed, plate.id);
                let (source_width, source_height) =
                    source_dimensions_for_plate_profile(db, &plate_profile_id)
                        .unwrap_or((1024, 1024));
                let (width, height) =
                    scaled_dimensions(source_width, source_height, config.default_scale_factor);

                let req = build_simulation_request(
                    &params.organism_id,
                    illumination_id,
                    &plate_profile_id,
                    &params,
                    TimeSpec {
                        start_after_seconds: time_spec.start_after_seconds,
                        duration_seconds: time_spec.duration_seconds,
                        step_seconds: time_spec.step_seconds,
                    },
                    plate_seed,
                    width,
                    height,
                    config.default_scale_factor,
                );
                let frames = engine.frame_iter(&req, db).with_context(|| {
                    format!(
                        "failed creating frame stream plate={} preset={}",
                        plate.id, preset_id
                    )
                })?;
                streams.push(RenderStream {
                    plate_id: plate.id,
                    preset_id: *preset_id,
                    well_ids: plate.wells.iter().map(|w| w.id).collect(),
                    frames,
                });
            }
        }

        for time_index in 0..expected_frames {
            if stop_check.should_stop(client, job.id).await? {
                info!(job_id = %job.id, time_index, "job stop requested; halting uploads");
                return Ok(JobProcessOutcome::StoppedByUser);
            }

            if params.respect_capture_interval {
                wait_for_time_index_mark(
                    schedule_start,
                    Duration::from_secs(job.capture_interval.max(1)),
                    time_index,
                )
                .await;
            }

            for stream in &mut streams {
                let frame: GeneratedFrame = stream
                    .frames
                    .next()
                    .transpose()
                    .with_context(|| {
                        format!(
                            "failed rendering frame plate={} preset={} time_index={}",
                            stream.plate_id, stream.preset_id, time_index
                        )
                    })?
                    .ok_or_else(|| {
                        anyhow!(
                            "frame stream ended early for plate={} preset={} at time_index={}",
                            stream.plate_id,
                            stream.preset_id,
                            time_index
                        )
                    })?;

                for well_id in &stream.well_ids {
                    if stop_check.should_stop(client, job.id).await? {
                        info!(
                            job_id = %job.id,
                            plate_id = %stream.plate_id,
                            well_id = %well_id,
                            time_index,
                            "job stop requested; halting uploads"
                        );
                        return Ok(JobProcessOutcome::StoppedByUser);
                    }

                    current_image += 1;
                    info!(
                        job_id = %job.id,
                        plate_id = %stream.plate_id,
                        well_id = %well_id,
                        time_index,
                        current = current_image,
                        total = total_images,
                        "uploading frame"
                    );

                    let payload = PlateImageAdd {
                        job_id: job.id,
                        captured_at: current_timestamp_rfc3339()?,
                        plate_id: stream.plate_id,
                        // Well{} and well_id are mutually exclusive in the API.
                        // well_id is "better" but harder to use in multiwell_image mode since the same frame applies to multiple wells
                        well: Well { col: 0, row: 0 },
                        well_id: None,
                        camera_preset_id: stream.preset_id,
                        time_index: time_index as i32,
                    };

                    let out = client.add_plate_image_metadata(&payload).await?;
                    client
                        .upload_jpeg_to_signed_url(&out.image_url, &frame.image_jpeg_bytes)
                        .await?;

                    if let (Some(endpoint), Some(bucket)) =
                        (&config.s3_annotation_endpoint, &config.s3_annotation_bucket)
                    {
                        let (img_w, img_h) =
                            scaled_dimensions(stream.frames.width(), stream.frames.height(), 1.0);
                        let annotation = AnnotationPayload {
                            image_width: img_w,
                            image_height: img_h,
                            colonies: &frame.annotations,
                        };
                        match serde_json::to_vec(&annotation) {
                            Ok(body) => {
                                if let Err(err) = client
                                    .upload_annotation(endpoint, bucket, out.id, &body)
                                    .await
                                {
                                    warn!(
                                        plate_image_id = %out.id,
                                        error = %err,
                                        "failed to upload annotation sidecar"
                                    );
                                }
                            }
                            Err(err) => {
                                warn!(error = %err, "failed to serialize annotation");
                            }
                        }
                    }
                }
            }
        }
        Ok(JobProcessOutcome::Finished)
    }
    .await;

    match process_result {
        Ok(JobProcessOutcome::Finished) => {
            client.update_job_status(job.id, "Finished").await?;
            info!(job_id = %job.id, "job finished");
            Ok(())
        }
        Ok(JobProcessOutcome::StoppedByUser) => {
            info!(job_id = %job.id, "job stopped by user request");
            Ok(())
        }
        Err(err) => {
            let _ = client.update_job_status(job.id, "Failed").await;
            Err(err)
        }
    }
}

#[derive(Debug)]
struct JobStopMonitor {
    next_check_at: Instant,
    stopped: bool,
}

impl JobStopMonitor {
    fn new() -> Self {
        Self {
            next_check_at: Instant::now(),
            stopped: false,
        }
    }

    async fn should_stop(&mut self, client: &RisClient, job_id: uuid::Uuid) -> Result<bool> {
        if self.stopped {
            return Ok(true);
        }

        let now = Instant::now();
        if now < self.next_check_at {
            return Ok(false);
        }

        let job = client.get_job(job_id).await?;
        let is_stopped = job
            .status
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("stopped"))
            .unwrap_or(false);
        self.stopped = is_stopped;
        self.next_check_at = now + RUNNING_JOB_STATUS_POLL_INTERVAL;
        Ok(is_stopped)
    }
}

async fn fail_started_jobs_on_boot(client: &RisClient) -> Result<()> {
    let jobs = client.get_jobs().await?;
    let started: Vec<_> = jobs.into_iter().filter(RisJob::is_started).collect();
    if started.is_empty() {
        info!("startup stale job cleanup complete: no started jobs");
        return Ok(());
    }

    warn!(
        started_job_count = started.len(),
        "startup stale job cleanup: marking started jobs as failed"
    );
    for job in started {
        client.update_job_status(job.id, "Failed").await?;
        warn!(
            job_id = %job.id,
            job_name = ?job.name,
            status = ?job.status,
            "marked started job as failed on startup"
        );
    }
    Ok(())
}

fn source_dimensions_for_plate_profile(
    db: &ProfileDb,
    plate_profile_id: &str,
) -> Option<(u32, u32)> {
    let plate = db.plate(plate_profile_id)?;
    let image_path = plate.image_path.as_ref()?;
    if !image_path.exists() {
        return None;
    }
    image::image_dimensions(image_path).ok()
}

fn resolve_presets(job: &RisJob) -> Result<Vec<uuid::Uuid>> {
    let presets = job.camera_preset_ids();
    if presets.is_empty() {
        bail!("no camera presets for job {}", job.id);
    }
    Ok(presets)
}

fn illumination_profile_id(i: IlluminationSelection) -> &'static str {
    match i {
        IlluminationSelection::Frontlit => "frontlit",
        IlluminationSelection::Backlit => "backlit",
    }
}

fn cfu_spec_label(cfu: &CfuSpec) -> String {
    match cfu {
        CfuSpec::Exact(v) => format!("exact:{v}"),
        CfuSpec::Range { min, max } => format!("range:{min}-{max}"),
    }
}

fn scaled_dimensions(width: u32, height: u32, scale_factor: f32) -> (u32, u32) {
    let w = ((width as f32) * scale_factor).round().max(64.0) as u32;
    let h = ((height as f32) * scale_factor).round().max(64.0) as u32;
    (w, h)
}

async fn wait_for_time_index_mark(start: Instant, step: Duration, time_index: usize) {
    let deadline = start + step.saturating_mul(time_index as u32);
    let now = Instant::now();
    if now < deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        let lag = now.duration_since(deadline);
        if lag > Duration::from_millis(250) {
            warn!(
                lag_ms = lag.as_millis(),
                time_index, "capture interval deadline already passed; processing immediately"
            );
        }
    }
}

#[derive(Serialize)]
struct AnnotationPayload<'a> {
    image_width: u32,
    image_height: u32,
    colonies: &'a [ColonyAnnotation],
}

fn validate_requested_organism(db: &ProfileDb, organism_id: &str) -> Result<()> {
    if db.organism(organism_id).is_some() {
        return Ok(());
    }

    let mut available: Vec<&str> = db.organisms.keys().map(String::as_str).collect();
    available.sort_unstable();

    bail!(
        "unknown organism profile '{organism_id}'. Available organisms: {}",
        available.join(", ")
    );
}

fn current_timestamp_rfc3339() -> Result<String> {
    let out = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%S.%3NZ")
        .output()?;
    if !out.status.success() {
        bail!("failed to generate timestamp with date command");
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::{scaled_dimensions, validate_requested_organism};
    use synthris_core::{OrganismProfile, ProfileDb};

    #[test]
    fn validate_requested_organism_accepts_known_id() {
        let mut db = ProfileDb::default();
        db.organisms.insert(
            "morrow".to_string(),
            serde_json::from_str::<OrganismProfile>(
                r#"{
                    "id":"morrow",
                    "temperature_cardinal":{"t_min_c":2.0,"t_opt_c":30.0,"t_max_c":45.0,"alpha":1.25,"beta":1.6},
                    "optical_material":{"kappa_ref":1.0,"thickness_exp":1.0,"translucency":0.9,"pigment_rgb":[212,190,145],"pigment_strength":0.35},
                    "growth_model":{"type":"gompertz_radius_v2","mu_max_ref_h":0.8,"lag_ref_h":1.0,"n0_log10":1.0,"nmax_log10":8.0,"r0_px":1.0,"rmax_ref_px":10.0},
                    "seeding_model":{"type":"poisson_disc_delay_v1","onset":{"mean_min":20.0,"sigma":0.5,"max_h":2.0}},
                    "geometry_model":{"type":"radial_dome_v2"},
                    "phenotypes":[{"id":"smooth_round","weight":1.0}]
                }"#,
            )
            .expect("organism profile"),
        );

        validate_requested_organism(&db, "morrow").expect("known organism should validate");
    }

    #[test]
    fn validate_requested_organism_lists_available_ids() {
        let mut db = ProfileDb::default();
        db.organisms.insert(
            "ecoli".to_string(),
            serde_json::from_str::<OrganismProfile>(
                r#"{
                    "id":"ecoli",
                    "temperature_cardinal":{"t_min_c":2.0,"t_opt_c":30.0,"t_max_c":45.0,"alpha":1.25,"beta":1.6},
                    "optical_material":{"kappa_ref":1.0,"thickness_exp":1.0,"translucency":0.9,"pigment_rgb":[212,190,145],"pigment_strength":0.35},
                    "growth_model":{"type":"gompertz_radius_v2","mu_max_ref_h":0.8,"lag_ref_h":1.0,"n0_log10":1.0,"nmax_log10":8.0,"r0_px":1.0,"rmax_ref_px":10.0},
                    "seeding_model":{"type":"poisson_disc_delay_v1","onset":{"mean_min":20.0,"sigma":0.5,"max_h":2.0}},
                    "geometry_model":{"type":"radial_dome_v2"},
                    "phenotypes":[{"id":"smooth_round","weight":1.0}]
                }"#,
            )
            .expect("organism profile"),
        );

        let err = validate_requested_organism(&db, "unknown").expect_err("must fail");
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown organism profile 'unknown'"));
        assert!(msg.contains("ecoli"));
    }

    #[test]
    fn scaled_dimensions_applies_factor_with_minimum() {
        assert_eq!(scaled_dimensions(3434, 3434, 0.25), (859, 859));
        assert_eq!(scaled_dimensions(100, 120, 0.1), (64, 64));
    }
}
