use std::f32::consts::TAU;

use crate::profiles::{LognormalDelaySpec, OrganismProfile, PhenotypeProfile};
use crate::request::{CfuSpec, OpacityClass};
use crate::roi::Roi;

use super::rng::Lcg;
use super::shape::ColonyMorphology;

#[derive(Debug, Clone, Copy)]
pub struct ColonySeed {
    pub x: i32,
    pub y: i32,
    pub onset_h: f32,
    pub kappa_scale: f32,
    pub temp_opt_offset_c: f32,
    pub morphology: ColonyMorphology,
}

#[derive(Debug, Clone)]
pub struct PoissonDiscDelayV1Params {
    pub min_dist_factor: f32,
    pub min_dist_floor_px: f32,
    pub attempts_per_colony: u32,
    pub onset: LognormalDelaySpec,
    pub kappa_jitter_low: f32,
    pub kappa_jitter_high: f32,
    pub opacity_scale_translucent: f32,
    pub opacity_scale_standard: f32,
    pub opacity_scale_dense: f32,
    pub morphology_jitter: f32,
    pub temp_opt_jitter_sigma_c: f32,
}

#[derive(Debug)]
pub struct SeedingInput<'a> {
    pub roi: &'a Roi,
    pub count: u32,
    pub max_radius_px: u32,
    pub opacity_class: OpacityClass,
    pub organism: &'a OrganismProfile,
}

pub fn resolve_cfu_count(cfu: &CfuSpec, seed: u64) -> u32 {
    match cfu {
        CfuSpec::Exact(v) => (*v).max(1),
        CfuSpec::Range { min, max } => {
            let lo = (*min).min(*max).max(1);
            let hi = (*min).max(*max).max(1);
            let mut rng = Lcg::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
            lo + (rng.next_u32() % (hi - lo + 1))
        }
    }
}

pub fn poisson_disc_with_delay(
    params: &PoissonDiscDelayV1Params,
    input: &SeedingInput<'_>,
    rng: &mut Lcg,
) -> Vec<ColonySeed> {
    let (x0, y0, w, h) = input.roi.bounds();
    if w == 0 || h == 0 {
        return Vec::new();
    }

    let mut out: Vec<ColonySeed> = Vec::with_capacity(input.count as usize);
    let min_dist = ((input.max_radius_px as f32 * params.min_dist_factor)
        .max(params.min_dist_floor_px)) as i32;
    let attempts_limit = input.count.saturating_mul(params.attempts_per_colony);
    let mut attempts = 0u32;

    while (out.len() as u32) < input.count && attempts < attempts_limit {
        attempts += 1;
        let x = x0 as i32 + (rng.next_u32() % w.max(1)) as i32;
        let y = y0 as i32 + (rng.next_u32() % h.max(1)) as i32;
        if !input.roi.contains(x, y) {
            continue;
        }

        let mut too_close = false;
        for c in &out {
            let dx = c.x - x;
            let dy = c.y - y;
            if dx * dx + dy * dy < min_dist * min_dist {
                too_close = true;
                break;
            }
        }
        if too_close {
            continue;
        }

        let delay_h = sample_lognormal_delay_h(rng, &params.onset);
        let opacity_multiplier = match input.opacity_class {
            OpacityClass::Translucent => params.opacity_scale_translucent,
            OpacityClass::Standard => params.opacity_scale_standard,
            OpacityClass::Dense => params.opacity_scale_dense,
        };

        let jitter_low = params.kappa_jitter_low.min(params.kappa_jitter_high);
        let jitter_high = params.kappa_jitter_low.max(params.kappa_jitter_high);
        let jitter = jitter_low + (jitter_high - jitter_low) * rng.next_f32();

        let phenotype = sample_phenotype(&input.organism.phenotypes, rng);

        out.push(ColonySeed {
            x,
            y,
            onset_h: delay_h,
            kappa_scale: opacity_multiplier * jitter,
            temp_opt_offset_c: sample_temperature_opt_offset_c(rng, params.temp_opt_jitter_sigma_c),
            morphology: ColonyMorphology {
                angle_rad: rng.next_f32() * TAU,
                anisotropy_scale: 1.0 + (rng.next_f32() * 2.0 - 1.0) * params.morphology_jitter,
                wobble_phase: rng.next_f32() * TAU,
                edge_roughness: phenotype.edge_roughness,
                spread_bias: phenotype.spread_bias,
                core_density: phenotype.core_density,
                phenotype: phenotype.id,
            },
        });
    }

    out
}

fn sample_lognormal_delay_h(rng: &mut Lcg, onset: &LognormalDelaySpec) -> f32 {
    let mean = onset.mean_min.max(0.01);
    let sigma = onset.sigma.max(0.01);
    let mu = mean.ln() - 0.5 * sigma * sigma;
    let z = rng.next_standard_normal();
    let delay_min = (mu + sigma * z).exp();
    (delay_min / 60.0).clamp(0.0, onset.max_h.max(0.0))
}

fn sample_temperature_opt_offset_c(rng: &mut Lcg, sigma_c: f32) -> f32 {
    let sigma = sigma_c.max(0.0);
    if sigma == 0.0 {
        return 0.0;
    }
    rng.next_standard_normal() * sigma
}

fn sample_phenotype<'a>(phenotypes: &'a [PhenotypeProfile], rng: &mut Lcg) -> &'a PhenotypeProfile {
    let total: f32 = phenotypes
        .iter()
        .map(|p| p.weight.max(0.0))
        .sum::<f32>()
        .max(0.001);
    let mut ticket = rng.next_f32() * total;
    for p in phenotypes {
        ticket -= p.weight.max(0.0);
        if ticket <= 0.0 {
            return p;
        }
    }
    &phenotypes[phenotypes.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::PhenotypeKind;

    #[test]
    fn resolve_cfu_count_range_is_seeded_and_bounded() {
        let spec = CfuSpec::Range { min: 10, max: 20 };
        let a = resolve_cfu_count(&spec, 42);
        let b = resolve_cfu_count(&spec, 42);
        let c = resolve_cfu_count(&spec, 43);

        assert_eq!(a, b);
        assert!((10..=20).contains(&a));
        assert!((10..=20).contains(&c));
    }

    #[test]
    fn sample_phenotype_honors_zero_weight() {
        let phenotypes = vec![
            PhenotypeProfile {
                id: PhenotypeKind::SmoothRound,
                weight: 1.0,
                edge_roughness: 0.1,
                spread_bias: 1.0,
                core_density: 1.0,
            },
            PhenotypeProfile {
                id: PhenotypeKind::RoughIrregular,
                weight: 0.0,
                edge_roughness: 0.8,
                spread_bias: 0.9,
                core_density: 0.9,
            },
        ];

        let mut rng = Lcg::new(7);
        for _ in 0..50 {
            let selected = sample_phenotype(&phenotypes, &mut rng);
            assert_eq!(selected.id, PhenotypeKind::SmoothRound);
        }
    }

    #[test]
    fn sample_temp_opt_offset_respects_sigma() {
        let mut rng = Lcg::new(11);
        assert_eq!(sample_temperature_opt_offset_c(&mut rng, 0.0), 0.0);

        let mut rng2 = Lcg::new(11);
        let v = sample_temperature_opt_offset_c(&mut rng2, 1.2);
        assert!(v.abs() > 0.0);
    }
}
