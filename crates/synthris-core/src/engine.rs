mod background;
mod codec;
mod growth;
mod models;
mod optics;
mod rng;
mod seeding;
mod shape;
mod timeline;
mod trace_metrics;

use anyhow::{Result, bail};
use image::RgbImage;
use serde::Serialize;
use std::time::Duration;
use tracing::{debug, info_span};
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;

use crate::plate::SimulationBackground;
use crate::profiles::{IlluminationProfile, OrganismProfile};
use crate::request::{
    ColonyAnnotation, GeneratedFrame, LookPreset, OpacityClass, SimulationManifest, SimulationRequest,
};
use crate::roi::Roi;
use background::render_background;
use codec::encode_jpeg;
use models::{GrowthModel, ModelBundle, SeedingModel, build_model_bundle};
use rng::Lcg;
use seeding::{ColonySeed, SeedingInput, resolve_cfu_count};
use shape::GeometryInput;
use timeline::build_elapsed_timeline;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub max_colony_radius_px: u32,
    pub default_width: u32,
    pub default_height: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_colony_radius_px: 64,
            default_width: 1024,
            default_height: 1024,
        }
    }
}

const JPEG_QUALITY_FAST_BALANCED: u8 = 82;

#[derive(Debug, Clone)]
pub struct Engine {
    config: EngineConfig,
}

pub struct FrameIterator {
    request: SimulationRequest,
    cfu_count: u32,
    width: u32,
    height: u32,
    elapsed: Vec<u64>,
    next_index: usize,
    roi: Roi,
    models: ModelBundle,
    colony_seeds: Vec<ColonySeed>,
    background_template: RgbImage,
    frame_img: RgbImage,
    jpeg_capacity_estimate: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GrowthTraceSample {
    pub elapsed_seconds: u64,
    pub seeded_colonies: u32,
    pub visible_colonies: u32,
    pub occupied_pixels: u32,
    pub roi_pixels: u32,
    pub occupied_area_pct_of_roi: f32,
    pub mean_colony_r: f32,
    pub mean_colony_g: f32,
    pub mean_colony_b: f32,
    pub mean_radius_px: f32,
    pub median_radius_px: f32,
    pub p90_radius_px: f32,
    pub stddev_radius_px: f32,
    pub max_radius_px: f32,
}

pub struct GrowthTraceIterator {
    request: SimulationRequest,
    elapsed: Vec<u64>,
    next_index: usize,
    models: ModelBundle,
    background_template: RgbImage,
    frame_img: RgbImage,
    width: u32,
    height: u32,
    roi: Roi,
    roi_pixels: u32,
    colony_seeds: Vec<ColonySeed>,
    seeded_colonies: u32,
}

pub struct RawFrame<'a> {
    pub elapsed_seconds: u64,
    pub width: u32,
    pub height: u32,
    pub rgb_bytes: &'a [u8],
}

impl FrameIterator {
    pub fn manifest(&self) -> SimulationManifest {
        SimulationManifest {
            organism_id: self.request.organism_id.clone(),
            illumination_id: self.request.illumination_id.clone(),
            background_mode: self.request.background_mode,
            cfu_count: self.cfu_count,
            temperature_c: self.request.temperature.constant_c,
            start_after_seconds: self.request.time.start_after_seconds,
            duration_seconds: self.request.time.duration_seconds,
            step_seconds: self.request.time.step_seconds,
            elapsed_seconds: self.elapsed.clone(),
            render_scale: self.request.render_scale,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.elapsed.len()
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn frame_dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn next_raw(&mut self) -> Option<Result<RawFrame<'_>>> {
        let (elapsed_seconds, paint_elapsed, _annotations) = match self.render_next()? {
            Ok(v) => v,
            Err(err) => return Some(Err(err)),
        };

        debug!(
            elapsed_seconds,
            paint_ms = paint_elapsed.as_millis(),
            "frame complete (raw)"
        );

        Some(Ok(RawFrame {
            elapsed_seconds,
            width: self.width,
            height: self.height,
            rgb_bytes: self.frame_img.as_raw(),
        }))
    }

    fn render_next(&mut self) -> Option<Result<(u64, Duration, Vec<ColonyAnnotation>)>> {
        let elapsed_seconds = *self.elapsed.get(self.next_index)?;
        self.next_index += 1;

        let frame_span = info_span!("frame", elapsed_seconds);
        let _frame_guard = frame_span.enter();
        self.frame_img
            .as_mut()
            .copy_from_slice(self.background_template.as_raw());
        let t_h = (self.request.time.start_after_seconds + elapsed_seconds) as f32 / 3600.0;

        let paint_start = Instant::now();
        let mut annotations = Vec::new();
        for (idx, colony) in self.colony_seeds.iter().enumerate() {
            let age_h = (t_h - colony.onset_h).max(0.0);
            let growth_state = self.models.growth.eval_state(
                age_h,
                self.request.temperature.constant_c,
                self.request.phase,
                colony.temp_opt_offset_c,
            );
            let radius = growth_state.radius_px;

            if radius <= 0.05 {
                continue;
            }

            annotations.push(ColonyAnnotation {
                x: colony.x,
                y: colony.y,
                radius_px: radius,
                phenotype_idx: 0,
            });

            paint_colony(
                &mut self.frame_img,
                colony,
                age_h,
                growth_state,
                self.request.temperature.constant_c,
                self.request.opacity_class,
                self.request.look,
                &self.roi,
                &self.models,
            );

            if self.request.show_colony_ids {
                paint_id_mark(&mut self.frame_img, colony.x, colony.y, idx as u32);
            }
        }

        Some(Ok((elapsed_seconds, paint_start.elapsed(), annotations)))
    }
}

impl Iterator for FrameIterator {
    type Item = Result<GeneratedFrame>;

    fn next(&mut self) -> Option<Self::Item> {
        let (elapsed_seconds, paint_elapsed, annotations) = match self.render_next()? {
            Ok(v) => v,
            Err(err) => return Some(Err(err)),
        };

        let encode_start = Instant::now();
        let mut bytes = match encode_jpeg(&self.frame_img, JPEG_QUALITY_FAST_BALANCED) {
            Ok(v) => v,
            Err(err) => return Some(Err(err)),
        };
        if bytes.capacity() < self.jpeg_capacity_estimate {
            bytes.reserve(self.jpeg_capacity_estimate - bytes.capacity());
        }
        let encode_elapsed = encode_start.elapsed();
        self.jpeg_capacity_estimate = bytes.len().saturating_mul(11) / 10;
        debug!(
            elapsed_seconds,
            paint_ms = paint_elapsed.as_millis(),
            encode_ms = encode_elapsed.as_millis(),
            "frame complete"
        );

        Some(Ok(GeneratedFrame {
            elapsed_seconds,
            image_jpeg_bytes: bytes,
            annotations,
        }))
    }
}

impl Iterator for GrowthTraceIterator {
    type Item = GrowthTraceSample;

    fn next(&mut self) -> Option<Self::Item> {
        let elapsed_seconds = *self.elapsed.get(self.next_index)?;
        self.next_index += 1;
        let t_h = (self.request.time.start_after_seconds + elapsed_seconds) as f32 / 3600.0;
        Some(trace_metrics::compute_trace_sample(
            &self.request,
            elapsed_seconds,
            t_h,
            self.seeded_colonies,
            self.width,
            self.height,
            self.roi_pixels,
            &self.roi,
            &self.colony_seeds,
            &self.models,
            &self.background_template,
            &mut self.frame_img,
        ))
    }
}

impl Engine {
    pub fn new(config: EngineConfig) -> Self {
        Self { config }
    }

    pub fn frame_iter(
        &self,
        request: &SimulationRequest,
        organism: &OrganismProfile,
        illumination: &IlluminationProfile,
        background: &SimulationBackground,
    ) -> Result<FrameIterator> {
        self.validate_request(request)?;
        let models = build_model_bundle(organism, illumination, request.render_scale)?;
        let shared = self.prepare_iteration_state(
            request,
            organism,
            models.growth.as_ref(),
            models.seeding.as_ref(),
            background,
        )?;
        let (background_template, roi) = render_background(
            shared.width,
            shared.height,
            background,
            illumination,
        )?;
        let frame_img = background_template.clone();
        let jpeg_capacity_estimate =
            (shared.width as usize * shared.height as usize / 4).max(16 * 1024);

        Ok(FrameIterator {
            request: request.clone(),
            cfu_count: shared.cfu_count,
            width: shared.width,
            height: shared.height,
            elapsed: shared.elapsed,
            next_index: 0,
            roi,
            models,
            colony_seeds: shared.colony_seeds,
            background_template,
            frame_img,
            jpeg_capacity_estimate,
        })
    }

    pub fn trace_iter(
        &self,
        request: &SimulationRequest,
        organism: &OrganismProfile,
        illumination: &IlluminationProfile,
        background: &SimulationBackground,
    ) -> Result<GrowthTraceIterator> {
        self.validate_request(request)?;
        let models = build_model_bundle(organism, illumination, request.render_scale)?;
        let shared = self.prepare_iteration_state(
            request,
            organism,
            models.growth.as_ref(),
            models.seeding.as_ref(),
            background,
        )?;
        let (background_template, roi) = render_background(
            shared.width,
            shared.height,
            background,
            illumination,
        )?;
        let frame_img = background_template.clone();
        let roi_pixels = count_roi_pixels(shared.width, shared.height, &roi);
        Ok(GrowthTraceIterator {
            request: request.clone(),
            elapsed: shared.elapsed,
            next_index: 0,
            models,
            background_template,
            frame_img,
            width: shared.width,
            height: shared.height,
            roi,
            roi_pixels,
            colony_seeds: shared.colony_seeds,
            seeded_colonies: shared.cfu_count,
        })
    }

    pub fn render_single_frame(
        &self,
        request: &SimulationRequest,
        organism: &OrganismProfile,
        illumination: &IlluminationProfile,
        background: &SimulationBackground,
        elapsed_seconds: u64,
    ) -> Result<GeneratedFrame> {
        let mut single = request.clone();
        single.time.start_after_seconds = single
            .time
            .start_after_seconds
            .saturating_add(elapsed_seconds);
        single.time.duration_seconds = 0;
        single.time.step_seconds = single.time.step_seconds.max(1);

        let mut iter = self.frame_iter(&single, organism, illumination, background)?;
        iter.next()
            .transpose()?
            .ok_or_else(|| anyhow::anyhow!("engine produced no frame"))
    }

    fn validate_request(&self, request: &SimulationRequest) -> Result<()> {
        if request.time.step_seconds == 0 {
            bail!("step_seconds must be > 0");
        }
        Ok(())
    }

    fn prepare_iteration_state(
        &self,
        request: &SimulationRequest,
        organism: &OrganismProfile,
        growth_model: &dyn GrowthModel,
        seeding_model: &dyn SeedingModel,
        background: &SimulationBackground,
    ) -> Result<IterationState> {
        let width = request.width.max(64);
        let height = request.height.max(64);
        let roi = background.growth_area_for_canvas(width, height);

        let cfu_count = resolve_cfu_count(&request.cfu, request.seed);
        let mut rng = Lcg::new(request.seed);
        let inferred_rmax = growth_model
            .inferred_rmax(request.temperature.constant_c)
            .max(8.0) as u32;
        let seeding_input = SeedingInput {
            roi: &roi,
            count: cfu_count,
            max_radius_px: self.config.max_colony_radius_px.min(inferred_rmax),
            opacity_class: request.opacity_class,
            organism,
        };
        let colony_seeds = seeding_model.seed(&seeding_input, &mut rng);
        let elapsed =
            build_elapsed_timeline(request.time.duration_seconds, request.time.step_seconds);

        Ok(IterationState {
            width,
            height,
            elapsed,
            cfu_count,
            colony_seeds,
        })
    }
}

struct IterationState {
    width: u32,
    height: u32,
    elapsed: Vec<u64>,
    cfu_count: u32,
    colony_seeds: Vec<ColonySeed>,
}

fn percentile_sorted(values: &[f32], percentile: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }

    let idx = ((values.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round() as usize;
    values[idx]
}

fn count_roi_pixels(width: u32, height: u32, roi: &Roi) -> u32 {
    let mut total = 0u32;
    for y in 0..height {
        for x in 0..width {
            if roi.contains(x as i32, y as i32) {
                total += 1;
            }
        }
    }
    total
}

#[allow(clippy::too_many_arguments)]
fn paint_colony(
    img: &mut RgbImage,
    colony: &ColonySeed,
    age_h: f32,
    growth_state: growth::GrowthState,
    temperature_c: f32,
    opacity_class: OpacityClass,
    look: LookPreset,
    roi: &Roi,
    models: &ModelBundle,
) {
    let radius = growth_state.radius_px;
    let (w, h) = img.dimensions();
    let radius_i = radius.max(1.0) as i32;
    let min_x = (colony.x - radius_i).max(0);
    let max_x = (colony.x + radius_i).min((w.saturating_sub(1)) as i32);
    let min_y = (colony.y - radius_i).max(0);
    let max_y = (colony.y + radius_i).min((h.saturating_sub(1)) as i32);
    let radius_ratio = radius / models.growth.inferred_rmax(temperature_c);

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if !roi.contains(x, y) {
                continue;
            }

            let dx = x - colony.x;
            let dy = y - colony.y;
            let Some(shape_sample) = models.geometry.sample(&GeometryInput {
                dx: dx as f32,
                dy: dy as f32,
                radius_px: radius,
                age_h,
                growth_state,
                morphology: colony.morphology,
            }) else {
                continue;
            };

            if shape_sample.coverage <= 0.0 {
                continue;
            }

            let pixel = img.get_pixel_mut(x as u32, y as u32);
            models.optics.shade(
                pixel,
                &shape_sample,
                growth_state,
                colony.morphology.phenotype,
                opacity_class,
                look,
                colony.kappa_scale,
                radius_ratio,
            );
        }
    }
}

fn paint_id_mark(img: &mut RgbImage, x: i32, y: i32, id: u32) {
    let r = (2 + (id % 3)) as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 {
                continue;
            }
            let (w, h) = img.dimensions();
            if (px as u32) < w && (py as u32) < h {
                img.get_pixel_mut(px as u32, py as u32).0 = [245, 245, 245];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use image::RgbImage;

    use super::growth::cardinal_phi;
    use super::{Engine, EngineConfig, percentile_sorted};
    use crate::plate::{PlateBaseline, SimulationBackground};
    use crate::profiles::{
        BacklitOpticsParams, FrontlitOpticsParams, GeometryModelSpec, GrowthModelSpec,
        IlluminationProfile, LognormalDelaySpec, LookScaleParams, OpacityScaleParams,
        OpticalMaterialProfile, OpticsModelSpec, OrganismProfile, PhenotypeKind, PhenotypeProfile,
        SeedingModelSpec, TemperatureCardinalProfile,
    };
    use crate::request::{
        BackgroundMode, CfuSpec, IlluminationMode, LookPreset, OpacityClass, PhasePreset,
        SimulationRequest, TemperatureSpec, TimeSpec,
    };
    use crate::roi::Roi;

    fn test_optics_model() -> OpticsModelSpec {
        OpticsModelSpec::AttenuationBlendV2 {
            backlit: BacklitOpticsParams {
                min_absorbance: 0.2,
                attenuation_edge_base: 0.5,
                attenuation_edge_gain: 0.5,
                tint_strength: 0.25,
                translucency_min: 0.2,
                translucency_max: 1.2,
            },
            frontlit: FrontlitOpticsParams {
                min_contrast: 0.2,
                target_edge_base: 0.7,
                target_edge_gain: 0.3,
                blend_alpha: 0.68,
            },
            look: LookScaleParams {
                clean: 0.9,
                realistic: 1.0,
                gritty: 1.15,
            },
            opacity: OpacityScaleParams {
                translucent: 0.8,
                standard: 1.0,
                dense: 1.3,
            },
        }
    }

    fn test_organism() -> OrganismProfile {
        OrganismProfile {
            id: "morrow".into(),
            temperature_cardinal: TemperatureCardinalProfile {
                t_min_c: 4.0,
                t_opt_c: 30.0,
                t_max_c: 44.0,
                alpha: 1.2,
                beta: 1.6,
            },
            optical_material: OpticalMaterialProfile {
                kappa_ref: 1.2,
                thickness_exp: 1.4,
                translucency: 0.9,
                pigment_rgb: [212, 190, 145],
                pigment_strength: 0.35,
            },
            growth_model: GrowthModelSpec::GompertzRadiusV2 {
                mu_max_ref_h: 0.8,
                lag_ref_h: 3.0,
                n0_log10: 1.0,
                nmax_log10: 8.0,
                r0_px: 2.0,
                rmax_ref_px: 35.0,
                phase_early_scale: 0.8,
                phase_mid_scale: 1.0,
                phase_late_scale: 1.25,
                rmax_temp_floor: 0.6,
            },
            seeding_model: SeedingModelSpec::PoissonDiscDelayV1 {
                min_dist_factor: 0.9,
                min_dist_floor_px: 8.0,
                attempts_per_colony: 600,
                onset: LognormalDelaySpec {
                    mean_min: 20.0,
                    sigma: 0.5,
                    max_h: 2.0,
                },
                kappa_jitter_low: 0.9,
                kappa_jitter_high: 1.1,
                opacity_scale_translucent: 0.8,
                opacity_scale_standard: 1.0,
                opacity_scale_dense: 1.3,
                morphology_jitter: 0.2,
                temp_opt_jitter_sigma_c: 0.0,
            },
            geometry_model: GeometryModelSpec::AnisotropicBlobV1 {
                edge_hardness: 1.0,
                thickness_power: 1.1,
                anisotropy: 0.2,
                angular_wobble: 0.07,
                wobble_frequency: 6,
            },
            phenotypes: vec![PhenotypeProfile {
                id: PhenotypeKind::SmoothRound,
                weight: 1.0,
                edge_roughness: 0.1,
                spread_bias: 1.0,
                core_density: 1.0,
            }],
        }
    }

    fn test_illumination(mode: IlluminationMode) -> IlluminationProfile {
        IlluminationProfile {
            id: match mode {
                IlluminationMode::Backlit => "backlit".into(),
                IlluminationMode::Frontlit => "frontlit".into(),
            },
            mode,
            background_rgb: [170, 165, 160],
            colony_rgb: [120, 110, 105],
            backlit_absorbance: 1.8,
            frontlit_contrast: 1.0,
            optics_model: test_optics_model(),
        }
    }

    fn test_request(background_mode: BackgroundMode) -> SimulationRequest {
        SimulationRequest {
            organism_id: "morrow".into(),
            illumination_id: "backlit".into(),
            background_mode,
            cfu: CfuSpec::Exact(20),
            time: TimeSpec {
                start_after_seconds: 0,
                duration_seconds: 24 * 3600,
                step_seconds: 12 * 3600,
            },
            temperature: TemperatureSpec { constant_c: 30.0 },
            phase: PhasePreset::Mid,
            look: LookPreset::Realistic,
            opacity_class: OpacityClass::Standard,
            seed: 7,
            width: 256,
            height: 256,
            show_colony_ids: false,
            render_scale: 1.0,
        }
    }

    fn test_plate_background() -> SimulationBackground {
        let plate = PlateBaseline::new(
            RgbImage::new(256, 256),
            Roi::Rect {
                x: 0,
                y: 0,
                width: 256,
                height: 256,
            },
        )
        .expect("plate baseline");
        SimulationBackground::PlateBaseline(plate)
    }

    #[test]
    fn cardinal_phi_peaks_near_optimum() {
        let card = TemperatureCardinalProfile {
            t_min_c: 5.0,
            t_opt_c: 30.0,
            t_max_c: 45.0,
            alpha: 1.3,
            beta: 1.5,
        };
        let cold = cardinal_phi(10.0, &card);
        let opt = cardinal_phi(30.0, &card);
        let hot = cardinal_phi(40.0, &card);
        assert!(opt >= cold);
        assert!(opt >= hot);
    }

    #[test]
    fn percentile_sorted_handles_empty_and_bounds() {
        let values = [1.0, 2.0, 4.0, 8.0, 16.0];
        assert_eq!(percentile_sorted(&[], 0.5), 0.0);
        assert_eq!(percentile_sorted(&values, -1.0), 1.0);
        assert_eq!(percentile_sorted(&values, 0.5), 4.0);
        assert_eq!(percentile_sorted(&values, 1.0), 16.0);
    }

    #[test]
    fn simulation_emits_time_based_frames() {
        let engine = Engine::new(EngineConfig::default());
        let req = SimulationRequest {
            time: TimeSpec {
                start_after_seconds: 0,
                duration_seconds: 48 * 3600,
                step_seconds: 24 * 3600,
            },
            cfu: CfuSpec::Exact(50),
            ..test_request(BackgroundMode::PlateImage)
        };
        let organism = test_organism();
        let illum = test_illumination(IlluminationMode::Backlit);
        let bg = test_plate_background();
        let samples: Vec<_> = engine
            .trace_iter(&req, &organism, &illum, &bg)
            .expect("trace_iter")
            .collect();
        let elapsed: Vec<_> = samples.iter().map(|s| s.elapsed_seconds).collect();
        assert_eq!(samples.len(), 3);
        assert_eq!(elapsed, vec![0, 86_400, 172_800]);
    }

    #[test]
    fn frame_and_trace_iter_match_elapsed_and_count() {
        let engine = Engine::new(EngineConfig::default());
        let req = SimulationRequest {
            cfu: CfuSpec::Exact(25),
            time: TimeSpec {
                start_after_seconds: 0,
                duration_seconds: 8 * 3600,
                step_seconds: 2 * 3600,
            },
            ..test_request(BackgroundMode::PlateImage)
        };
        let organism = test_organism();
        let illum = test_illumination(IlluminationMode::Backlit);
        let bg = test_plate_background();

        let trace_elapsed: Vec<_> = engine
            .trace_iter(&req, &organism, &illum, &bg)
            .expect("trace_iter")
            .map(|s| s.elapsed_seconds)
            .collect();
        let mut iter = engine
            .frame_iter(&req, &organism, &illum, &bg)
            .expect("frame_iter");
        let manifest = iter.manifest();
        let mut elapsed = Vec::new();
        for frame in &mut iter {
            elapsed.push(frame.expect("frame").elapsed_seconds);
        }
        assert_eq!(elapsed, trace_elapsed);
        assert_eq!(manifest.elapsed_seconds, trace_elapsed);
    }

    #[test]
    fn blankfield_mode_works_without_plate_baseline() {
        let engine = Engine::new(EngineConfig::default());
        let req = test_request(BackgroundMode::Blankfield);
        let organism = test_organism();
        let illum = test_illumination(IlluminationMode::Backlit);
        let samples: Vec<_> = engine
            .trace_iter(&req, &organism, &illum, &SimulationBackground::Blankfield)
            .expect("trace_iter")
            .collect();
        assert_eq!(samples.len(), 3);
    }
}
