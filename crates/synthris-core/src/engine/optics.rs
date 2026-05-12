use image::Rgb;

use crate::profiles::{
    BacklitOpticsParams, FrontlitOpticsParams, LookScaleParams, OpacityScaleParams, PhenotypeKind,
};
use crate::request::{IlluminationMode, LookPreset, OpacityClass};

use super::growth::GrowthState;
use super::shape::GeometrySample;

#[derive(Debug, Clone, Copy)]
pub struct OpticalMaterial {
    pub kappa_ref: f32,
    pub thickness_exp: f32,
    pub translucency: f32,
    pub pigment_rgb: [u8; 3],
    pub pigment_strength: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct OpticsInput {
    pub illumination_mode: IlluminationMode,
    pub colony_rgb: [u8; 3],
    pub backlit_absorbance: f32,
    pub frontlit_contrast: f32,
    pub material: OpticalMaterial,
    pub opacity_class: OpacityClass,
    pub look: LookPreset,
    pub colony_kappa_scale: f32,
    pub colony_radius_ratio: f32,
    pub growth_state: GrowthState,
    pub phenotype: PhenotypeKind,
}

#[derive(Debug, Clone)]
pub struct AttenuationBlendV2Params {
    pub backlit: BacklitOpticsParams,
    pub frontlit: FrontlitOpticsParams,
    pub look: LookScaleParams,
    pub opacity: OpacityScaleParams,
}

pub fn attenuation_blend_v2(
    pixel: &mut Rgb<u8>,
    input: &OpticsInput,
    shape: &GeometrySample,
    params: &AttenuationBlendV2Params,
) {
    let class_scale = match input.opacity_class {
        OpacityClass::Translucent => params.opacity.translucent,
        OpacityClass::Standard => params.opacity.standard,
        OpacityClass::Dense => params.opacity.dense,
    };

    let look_scale = match input.look {
        LookPreset::Clean => params.look.clean,
        LookPreset::Realistic => params.look.realistic,
        LookPreset::Gritty => params.look.gritty,
    };

    let phenotype_kappa = match input.phenotype {
        PhenotypeKind::SmoothRound => 1.0,
        PhenotypeKind::RoughIrregular => 1.12,
        PhenotypeKind::MucoidSpread => 0.88,
    };

    let kappa = input.material.kappa_ref.max(0.01)
        * class_scale
        * look_scale
        * input.colony_kappa_scale
        * phenotype_kappa
        * (0.7 + 0.6 * input.growth_state.biomass_norm);

    let base_thickness = input
        .colony_radius_ratio
        .clamp(0.0, 1.5)
        .powf(input.material.thickness_exp.max(0.2));
    let thickness = base_thickness
        * shape.thickness.clamp(0.0, 1.5)
        * (0.75 + 0.5 * input.growth_state.biomass_norm);

    match input.illumination_mode {
        IlluminationMode::Backlit => apply_backlit(
            pixel,
            input,
            shape,
            shape.edge_weight,
            kappa,
            thickness,
            &params.backlit,
        ),
        IlluminationMode::Frontlit => apply_frontlit(pixel, input, shape, &params.frontlit),
    }
}

fn apply_backlit(
    pixel: &mut Rgb<u8>,
    input: &OpticsInput,
    shape: &GeometrySample,
    edge_weight: f32,
    kappa: f32,
    thickness: f32,
    constants: &BacklitOpticsParams,
) {
    let absorb = input.backlit_absorbance.max(constants.min_absorbance);
    let edge_multiplier =
        constants.attenuation_edge_base + constants.attenuation_edge_gain * edge_weight;
    let attenuation = (-kappa * thickness * absorb * edge_multiplier).exp();

    let viability = input.growth_state.viability_norm.clamp(0.2, 1.0);
    let translucency = input
        .material
        .translucency
        .clamp(constants.translucency_min, constants.translucency_max)
        * (0.7 + 0.4 * viability);

    for channel in 0..3 {
        let bg = pixel.0[channel] as f32;
        let tint = input.colony_rgb[channel] as f32
            * (1.0 - attenuation)
            * constants.tint_strength
            * (0.6 + 0.5 * input.growth_state.biomass_norm);
        let value = bg * attenuation * translucency + tint;
        pixel.0[channel] = value.clamp(0.0, 255.0) as u8;
    }

    apply_pigment(pixel, input, shape, 1.15);
}

fn apply_frontlit(
    pixel: &mut Rgb<u8>,
    input: &OpticsInput,
    shape: &GeometrySample,
    constants: &FrontlitOpticsParams,
) {
    let contrast = input.frontlit_contrast.max(constants.min_contrast)
        * (0.85 + 0.3 * input.growth_state.roughness_norm);

    for channel in 0..3 {
        let target = input.colony_rgb[channel] as f32
            * (constants.target_edge_base + constants.target_edge_gain * shape.edge_weight)
            * contrast;
        pixel.0[channel] = blend(
            pixel.0[channel],
            target.clamp(0.0, 255.0) as u8,
            constants.blend_alpha,
        );
    }

    apply_pigment(pixel, input, shape, 0.45);
}

fn blend(base: u8, overlay: u8, alpha: f32) -> u8 {
    let a = alpha.clamp(0.0, 1.0);
    ((base as f32 * (1.0 - a)) + (overlay as f32 * a)) as u8
}

fn apply_pigment(
    pixel: &mut Rgb<u8>,
    input: &OpticsInput,
    shape: &GeometrySample,
    mode_scale: f32,
) {
    let thickness_visibility = shape.thickness.clamp(0.0, 1.0);
    let growth_visibility = (0.35 + 0.65 * input.growth_state.biomass_norm).clamp(0.0, 1.0);
    let alpha = (input.material.pigment_strength
        * mode_scale
        * shape.coverage.clamp(0.0, 1.0)
        * (0.45 + 0.55 * thickness_visibility)
        * growth_visibility)
        .clamp(0.0, 1.0);

    for channel in 0..3 {
        pixel.0[channel] = blend(pixel.0[channel], input.material.pigment_rgb[channel], alpha);
    }
}
