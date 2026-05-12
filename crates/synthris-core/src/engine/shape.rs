use crate::profiles::PhenotypeKind;

use super::growth::GrowthState;

#[derive(Debug, Clone, Copy)]
pub struct ColonyMorphology {
    pub angle_rad: f32,
    pub anisotropy_scale: f32,
    pub wobble_phase: f32,
    pub edge_roughness: f32,
    pub spread_bias: f32,
    pub core_density: f32,
    pub phenotype: PhenotypeKind,
}

#[derive(Debug, Clone, Copy)]
pub struct GeometryInput {
    pub dx: f32,
    pub dy: f32,
    pub radius_px: f32,
    pub age_h: f32,
    pub growth_state: GrowthState,
    pub morphology: ColonyMorphology,
}

#[derive(Debug, Clone, Copy)]
pub struct GeometrySample {
    pub coverage: f32,
    pub edge_weight: f32,
    pub thickness: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct RadialDomeV2Params {
    pub edge_hardness: f32,
    pub thickness_power: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct AnisotropicBlobV1Params {
    pub edge_hardness: f32,
    pub thickness_power: f32,
    pub anisotropy: f32,
    pub angular_wobble: f32,
    pub wobble_frequency: u32,
}

pub fn radial_dome_v2(
    params: &RadialDomeV2Params,
    input: &GeometryInput,
) -> Option<GeometrySample> {
    if input.radius_px <= 0.0 {
        return None;
    }
    let spread = input.morphology.spread_bias.max(0.5);
    let r = (input.dx * input.dx + input.dy * input.dy).sqrt() / (input.radius_px * spread);
    sample_shape(
        r,
        params.edge_hardness,
        params.thickness_power,
        input.growth_state,
        input.morphology,
    )
}

pub fn anisotropic_blob_v1(
    params: &AnisotropicBlobV1Params,
    input: &GeometryInput,
) -> Option<GeometrySample> {
    if input.radius_px <= 0.0 {
        return None;
    }

    let cos_a = input.morphology.angle_rad.cos();
    let sin_a = input.morphology.angle_rad.sin();

    let x_rot = input.dx * cos_a + input.dy * sin_a;
    let y_rot = -input.dx * sin_a + input.dy * cos_a;

    let phenotype_anisotropy = match input.morphology.phenotype {
        PhenotypeKind::SmoothRound => 0.8,
        PhenotypeKind::RoughIrregular => 1.25,
        PhenotypeKind::MucoidSpread => 1.0,
    };

    let base_anisotropy = (1.0
        + params.anisotropy * input.morphology.anisotropy_scale * phenotype_anisotropy)
        .clamp(0.65, 1.65);
    let theta = y_rot.atan2(x_rot);
    let wobble_gain = params.angular_wobble.clamp(0.0, 0.6)
        * (0.8 + input.morphology.edge_roughness * 0.7)
        * (0.7 + input.growth_state.roughness_norm * 0.6)
        * (0.9 + (input.age_h / 24.0).clamp(0.0, 1.0) * 0.2);

    let wobble = 1.0
        + wobble_gain
            * ((theta * params.wobble_frequency.max(1) as f32) + input.morphology.wobble_phase)
                .sin();

    let spread = input.morphology.spread_bias.max(0.5);
    let ax = input.radius_px * base_anisotropy * spread;
    let by = input.radius_px / base_anisotropy * spread;
    let r_elliptic = ((x_rot / ax).powi(2) + (y_rot / by).powi(2)).sqrt() / wobble.max(0.2);

    sample_shape(
        r_elliptic,
        params.edge_hardness,
        params.thickness_power,
        input.growth_state,
        input.morphology,
    )
}

fn sample_shape(
    normalized_radius: f32,
    edge_hardness: f32,
    thickness_power: f32,
    growth_state: GrowthState,
    morphology: ColonyMorphology,
) -> Option<GeometrySample> {
    if normalized_radius > 1.0 {
        return None;
    }

    let roughness_boost = 1.0 + morphology.edge_roughness * growth_state.roughness_norm;
    let edge =
        (1.0 - normalized_radius.clamp(0.0, 1.0)).powf((edge_hardness * roughness_boost).max(0.1));

    let density = morphology.core_density.max(0.5);
    let thickness =
        edge.powf((thickness_power * density).max(0.1)) * (0.5 + 0.5 * growth_state.biomass_norm);

    Some(GeometrySample {
        coverage: 1.0,
        edge_weight: edge,
        thickness,
    })
}
