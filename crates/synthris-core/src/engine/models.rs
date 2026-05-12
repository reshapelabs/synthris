use anyhow::Result;
use image::Rgb;

use crate::profiles::{
    GeometryModelSpec, GrowthModelSpec, IlluminationProfile, OpticalMaterialProfile,
    OpticsModelSpec, OrganismProfile, SeedingModelSpec,
};
use crate::request::{LookPreset, OpacityClass, PhasePreset};

use super::growth::{self, GompertzRadiusV2Params, GrowthInput, GrowthState};
use super::optics::{self, AttenuationBlendV2Params, OpticalMaterial, OpticsInput};
use super::rng::Lcg;
use super::seeding::{self, ColonySeed, PoissonDiscDelayV1Params, SeedingInput};
use super::shape::{
    self, AnisotropicBlobV1Params, GeometryInput, GeometrySample, RadialDomeV2Params,
};

pub trait GrowthModel: Send + Sync {
    fn eval_state(
        &self,
        age_h: f32,
        temperature_c: f32,
        phase: PhasePreset,
        temp_opt_offset_c: f32,
    ) -> GrowthState;
    fn inferred_rmax(&self, temperature_c: f32) -> f32;
}

pub trait SeedingModel: Send + Sync {
    fn seed(&self, input: &SeedingInput<'_>, rng: &mut Lcg) -> Vec<ColonySeed>;
}

pub trait GeometryModel: Send + Sync {
    fn sample(&self, input: &GeometryInput) -> Option<GeometrySample>;
}

pub trait OpticsModel: Send + Sync {
    fn shade(
        &self,
        pixel: &mut Rgb<u8>,
        shape: &GeometrySample,
        growth_state: GrowthState,
        phenotype: crate::profiles::PhenotypeKind,
        opacity_class: OpacityClass,
        look: LookPreset,
        colony_kappa_scale: f32,
        colony_radius_ratio: f32,
    );
}

pub struct ModelBundle {
    pub growth: Box<dyn GrowthModel>,
    pub seeding: Box<dyn SeedingModel>,
    pub geometry: Box<dyn GeometryModel>,
    pub optics: Box<dyn OpticsModel>,
}

pub fn build_model_bundle(
    organism: &OrganismProfile,
    illumination: &IlluminationProfile,
    render_scale: f32,
) -> Result<ModelBundle> {
    let (growth, seeding) = build_growth_and_seeding_models(organism, render_scale);
    let geometry = geometry_from_spec(&organism.geometry_model);
    let optics = optics_from_spec(
        &illumination.optics_model,
        illumination,
        organism.optical_material.clone(),
    );

    Ok(ModelBundle {
        growth,
        seeding,
        geometry,
        optics,
    })
}

pub(crate) fn build_growth_and_seeding_models(
    organism: &OrganismProfile,
    render_scale: f32,
) -> (Box<dyn GrowthModel>, Box<dyn SeedingModel>) {
    let scale = render_scale.clamp(0.05, 4.0);
    let growth_spec = scale_growth_spec(&organism.growth_model, scale);
    let seeding_spec = scale_seeding_spec(&organism.seeding_model, scale);
    let growth = growth_from_spec(&growth_spec, organism.temperature_cardinal.clone());
    let seeding = seeding_from_spec(&seeding_spec);
    (growth, seeding)
}

fn scale_growth_spec(spec: &GrowthModelSpec, scale: f32) -> GrowthModelSpec {
    match spec {
        GrowthModelSpec::GompertzRadiusV2 {
            mu_max_ref_h,
            lag_ref_h,
            n0_log10,
            nmax_log10,
            r0_px,
            rmax_ref_px,
            phase_early_scale,
            phase_mid_scale,
            phase_late_scale,
            rmax_temp_floor,
        } => {
            let scaled_r0 = (r0_px * scale).max(0.6);
            let scaled_rmax = (rmax_ref_px * scale).max(scaled_r0 + 0.5);
            GrowthModelSpec::GompertzRadiusV2 {
                mu_max_ref_h: *mu_max_ref_h,
                lag_ref_h: *lag_ref_h,
                n0_log10: *n0_log10,
                nmax_log10: *nmax_log10,
                r0_px: scaled_r0,
                rmax_ref_px: scaled_rmax,
                phase_early_scale: *phase_early_scale,
                phase_mid_scale: *phase_mid_scale,
                phase_late_scale: *phase_late_scale,
                rmax_temp_floor: *rmax_temp_floor,
            }
        }
    }
}

fn scale_seeding_spec(spec: &SeedingModelSpec, scale: f32) -> SeedingModelSpec {
    match spec {
        SeedingModelSpec::PoissonDiscDelayV1 {
            min_dist_factor,
            min_dist_floor_px,
            attempts_per_colony,
            onset,
            kappa_jitter_low,
            kappa_jitter_high,
            opacity_scale_translucent,
            opacity_scale_standard,
            opacity_scale_dense,
            morphology_jitter,
            temp_opt_jitter_sigma_c,
        } => SeedingModelSpec::PoissonDiscDelayV1 {
            min_dist_factor: *min_dist_factor,
            min_dist_floor_px: (min_dist_floor_px * scale).max(1.0),
            attempts_per_colony: *attempts_per_colony,
            onset: onset.clone(),
            kappa_jitter_low: *kappa_jitter_low,
            kappa_jitter_high: *kappa_jitter_high,
            opacity_scale_translucent: *opacity_scale_translucent,
            opacity_scale_standard: *opacity_scale_standard,
            opacity_scale_dense: *opacity_scale_dense,
            morphology_jitter: *morphology_jitter,
            temp_opt_jitter_sigma_c: *temp_opt_jitter_sigma_c,
        },
    }
}

fn growth_from_spec(
    spec: &GrowthModelSpec,
    temp_cardinal: crate::profiles::TemperatureCardinalProfile,
) -> Box<dyn GrowthModel> {
    match spec {
        GrowthModelSpec::GompertzRadiusV2 {
            mu_max_ref_h,
            lag_ref_h,
            n0_log10,
            nmax_log10,
            r0_px,
            rmax_ref_px,
            phase_early_scale,
            phase_mid_scale,
            phase_late_scale,
            rmax_temp_floor,
        } => Box::new(GompertzGrowthModel {
            params: GompertzRadiusV2Params {
                mu_max_ref_h: *mu_max_ref_h,
                lag_ref_h: *lag_ref_h,
                n0_log10: *n0_log10,
                nmax_log10: *nmax_log10,
                r0_px: *r0_px,
                rmax_ref_px: *rmax_ref_px,
                phase_early_scale: *phase_early_scale,
                phase_mid_scale: *phase_mid_scale,
                phase_late_scale: *phase_late_scale,
                rmax_temp_floor: *rmax_temp_floor,
            },
            temp_cardinal,
        }),
    }
}

fn seeding_from_spec(spec: &SeedingModelSpec) -> Box<dyn SeedingModel> {
    match spec {
        SeedingModelSpec::PoissonDiscDelayV1 {
            min_dist_factor,
            min_dist_floor_px,
            attempts_per_colony,
            onset,
            kappa_jitter_low,
            kappa_jitter_high,
            opacity_scale_translucent,
            opacity_scale_standard,
            opacity_scale_dense,
            morphology_jitter,
            temp_opt_jitter_sigma_c,
        } => Box::new(PoissonSeedingModel {
            params: PoissonDiscDelayV1Params {
                min_dist_factor: *min_dist_factor,
                min_dist_floor_px: *min_dist_floor_px,
                attempts_per_colony: *attempts_per_colony,
                onset: onset.clone(),
                kappa_jitter_low: *kappa_jitter_low,
                kappa_jitter_high: *kappa_jitter_high,
                opacity_scale_translucent: *opacity_scale_translucent,
                opacity_scale_standard: *opacity_scale_standard,
                opacity_scale_dense: *opacity_scale_dense,
                morphology_jitter: *morphology_jitter,
                temp_opt_jitter_sigma_c: *temp_opt_jitter_sigma_c,
            },
        }),
    }
}

fn geometry_from_spec(spec: &GeometryModelSpec) -> Box<dyn GeometryModel> {
    match spec {
        GeometryModelSpec::RadialDomeV2 {
            edge_hardness,
            thickness_power,
        } => Box::new(RadialDomeGeometryModel {
            params: RadialDomeV2Params {
                edge_hardness: *edge_hardness,
                thickness_power: *thickness_power,
            },
        }),
        GeometryModelSpec::AnisotropicBlobV1 {
            edge_hardness,
            thickness_power,
            anisotropy,
            angular_wobble,
            wobble_frequency,
        } => Box::new(AnisotropicBlobGeometryModel {
            params: AnisotropicBlobV1Params {
                edge_hardness: *edge_hardness,
                thickness_power: *thickness_power,
                anisotropy: *anisotropy,
                angular_wobble: *angular_wobble,
                wobble_frequency: *wobble_frequency,
            },
        }),
    }
}

fn optics_from_spec(
    spec: &OpticsModelSpec,
    illumination: &IlluminationProfile,
    material: OpticalMaterialProfile,
) -> Box<dyn OpticsModel> {
    match spec {
        OpticsModelSpec::AttenuationBlendV2 {
            backlit,
            frontlit,
            look,
            opacity,
        } => Box::new(AttenuationBlendOpticsModel {
            params: AttenuationBlendV2Params {
                backlit: backlit.clone(),
                frontlit: frontlit.clone(),
                look: look.clone(),
                opacity: opacity.clone(),
            },
            mode: illumination.mode,
            colony_rgb: illumination.colony_rgb,
            backlit_absorbance: illumination.backlit_absorbance,
            frontlit_contrast: illumination.frontlit_contrast,
            material: OpticalMaterial {
                kappa_ref: material.kappa_ref,
                thickness_exp: material.thickness_exp,
                translucency: material.translucency,
                pigment_rgb: material.pigment_rgb,
                pigment_strength: material.pigment_strength,
            },
        }),
    }
}

struct GompertzGrowthModel {
    params: GompertzRadiusV2Params,
    temp_cardinal: crate::profiles::TemperatureCardinalProfile,
}

impl GrowthModel for GompertzGrowthModel {
    fn eval_state(
        &self,
        age_h: f32,
        temperature_c: f32,
        phase: PhasePreset,
        temp_opt_offset_c: f32,
    ) -> GrowthState {
        growth::gompertz_state_v1(
            &self.params,
            &GrowthInput {
                temperature_c,
                phase,
                age_h,
                temp_cardinal: &self.temp_cardinal,
                temp_opt_offset_c,
            },
        )
    }

    fn inferred_rmax(&self, temperature_c: f32) -> f32 {
        growth::inferred_rmax(&self.params, &self.temp_cardinal, temperature_c)
    }
}

struct PoissonSeedingModel {
    params: PoissonDiscDelayV1Params,
}

impl SeedingModel for PoissonSeedingModel {
    fn seed(&self, input: &SeedingInput<'_>, rng: &mut Lcg) -> Vec<ColonySeed> {
        seeding::poisson_disc_with_delay(&self.params, input, rng)
    }
}

struct RadialDomeGeometryModel {
    params: RadialDomeV2Params,
}

impl GeometryModel for RadialDomeGeometryModel {
    fn sample(&self, input: &GeometryInput) -> Option<GeometrySample> {
        shape::radial_dome_v2(&self.params, input)
    }
}

struct AnisotropicBlobGeometryModel {
    params: AnisotropicBlobV1Params,
}

impl GeometryModel for AnisotropicBlobGeometryModel {
    fn sample(&self, input: &GeometryInput) -> Option<GeometrySample> {
        shape::anisotropic_blob_v1(&self.params, input)
    }
}

struct AttenuationBlendOpticsModel {
    params: AttenuationBlendV2Params,
    mode: crate::request::IlluminationMode,
    colony_rgb: [u8; 3],
    backlit_absorbance: f32,
    frontlit_contrast: f32,
    material: OpticalMaterial,
}

impl OpticsModel for AttenuationBlendOpticsModel {
    fn shade(
        &self,
        pixel: &mut Rgb<u8>,
        shape: &GeometrySample,
        growth_state: GrowthState,
        phenotype: crate::profiles::PhenotypeKind,
        opacity_class: OpacityClass,
        look: LookPreset,
        colony_kappa_scale: f32,
        colony_radius_ratio: f32,
    ) {
        optics::attenuation_blend_v2(
            pixel,
            &OpticsInput {
                illumination_mode: self.mode,
                colony_rgb: self.colony_rgb,
                backlit_absorbance: self.backlit_absorbance,
                frontlit_contrast: self.frontlit_contrast,
                material: self.material,
                opacity_class,
                look,
                colony_kappa_scale,
                colony_radius_ratio,
                growth_state,
                phenotype,
            },
            shape,
            &self.params,
        );
    }
}
