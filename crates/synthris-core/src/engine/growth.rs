use crate::profiles::TemperatureCardinalProfile;
use crate::request::PhasePreset;

#[derive(Debug, Clone, Copy)]
pub struct GrowthInput<'a> {
    pub temperature_c: f32,
    pub phase: PhasePreset,
    pub age_h: f32,
    pub temp_cardinal: &'a TemperatureCardinalProfile,
    pub temp_opt_offset_c: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GrowthState {
    pub radius_px: f32,
    pub biomass_norm: f32,
    pub viability_norm: f32,
    pub roughness_norm: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GompertzRadiusV2Params {
    pub mu_max_ref_h: f32,
    pub lag_ref_h: f32,
    pub n0_log10: f32,
    pub nmax_log10: f32,
    pub r0_px: f32,
    pub rmax_ref_px: f32,
    pub phase_early_scale: f32,
    pub phase_mid_scale: f32,
    pub phase_late_scale: f32,
    pub rmax_temp_floor: f32,
}

pub fn gompertz_state_v1(params: &GompertzRadiusV2Params, input: &GrowthInput<'_>) -> GrowthState {
    let phi = cardinal_phi_with_opt_offset(
        input.temperature_c,
        input.temp_cardinal,
        input.temp_opt_offset_c,
    );

    let age_scale = match input.phase {
        PhasePreset::Early => params.phase_early_scale,
        PhasePreset::Mid => params.phase_mid_scale,
        PhasePreset::Late => params.phase_late_scale,
    }
    .max(0.01);

    let mu = params.mu_max_ref_h.max(0.0001) * phi;
    let lag_h = params.lag_ref_h.max(0.01) / phi.max(0.05);
    let r0 = params.r0_px.max(0.1);
    let rmax = params.rmax_ref_px.max(r0 + 1.0)
        * (params.rmax_temp_floor + (1.0 - params.rmax_temp_floor) * phi);

    let t = (input.age_h * age_scale - lag_h).max(0.0);
    let n0 = 10f32.powf(params.n0_log10);
    let nmax = 10f32.powf(params.nmax_log10).max(n0 + 1.0);

    let exp_term = (-mu * t).exp();
    let nt = nmax / (1.0 + ((nmax - n0) / n0) * exp_term);
    let progress = ((nt - n0) / (nmax - n0)).clamp(0.0, 1.0);
    let radius_px = r0 + (rmax - r0) * progress;

    // Heuristic state channels derived from growth progress and age.
    let biomass_norm = progress;
    let viability_norm = (1.0 - 0.35 * progress.powf(1.7)).clamp(0.25, 1.0);
    let roughness_norm = (0.2 + 0.8 * progress.powf(0.9)).clamp(0.0, 1.0);

    GrowthState {
        radius_px,
        biomass_norm,
        viability_norm,
        roughness_norm,
    }
}

pub fn inferred_rmax(
    params: &GompertzRadiusV2Params,
    card: &TemperatureCardinalProfile,
    temp_c: f32,
) -> f32 {
    let phi = cardinal_phi(temp_c, card);
    (params.rmax_ref_px * (params.rmax_temp_floor + (1.0 - params.rmax_temp_floor) * phi)).max(1.0)
}

pub fn cardinal_phi(temp_c: f32, card: &TemperatureCardinalProfile) -> f32 {
    cardinal_phi_with_opt_offset(temp_c, card, 0.0)
}

pub fn cardinal_phi_with_opt_offset(
    temp_c: f32,
    card: &TemperatureCardinalProfile,
    opt_offset_c: f32,
) -> f32 {
    if temp_c <= card.t_min_c || temp_c >= card.t_max_c {
        return 0.02;
    }

    let eps = 0.01_f32;
    let t_opt = (card.t_opt_c + opt_offset_c).clamp(card.t_min_c + eps, card.t_max_c - eps);

    if temp_c <= t_opt {
        let denom = (t_opt - card.t_min_c).max(0.01);
        ((temp_c - card.t_min_c) / denom)
            .clamp(0.0, 1.0)
            .powf(card.alpha.max(0.1))
            .clamp(0.02, 1.0)
    } else {
        let denom = (card.t_max_c - t_opt).max(0.01);
        ((card.t_max_c - temp_c) / denom)
            .clamp(0.0, 1.0)
            .powf(card.beta.max(0.1))
            .clamp(0.02, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cardinal() -> TemperatureCardinalProfile {
        TemperatureCardinalProfile {
            t_min_c: 4.0,
            t_opt_c: 30.0,
            t_max_c: 44.0,
            alpha: 1.2,
            beta: 1.5,
        }
    }

    fn test_params() -> GompertzRadiusV2Params {
        GompertzRadiusV2Params {
            mu_max_ref_h: 0.8,
            lag_ref_h: 2.5,
            n0_log10: 1.0,
            nmax_log10: 8.0,
            r0_px: 2.0,
            rmax_ref_px: 40.0,
            phase_early_scale: 0.8,
            phase_mid_scale: 1.0,
            phase_late_scale: 1.2,
            rmax_temp_floor: 0.6,
        }
    }

    #[test]
    fn gompertz_state_grows_with_age() {
        let card = test_cardinal();
        let params = test_params();
        let s0 = gompertz_state_v1(
            &params,
            &GrowthInput {
                temperature_c: 30.0,
                phase: PhasePreset::Mid,
                age_h: 0.0,
                temp_cardinal: &card,
                temp_opt_offset_c: 0.0,
            },
        );
        let s1 = gompertz_state_v1(
            &params,
            &GrowthInput {
                temperature_c: 30.0,
                phase: PhasePreset::Mid,
                age_h: 24.0,
                temp_cardinal: &card,
                temp_opt_offset_c: 0.0,
            },
        );

        assert!(s1.radius_px > s0.radius_px);
        assert!(s1.biomass_norm >= s0.biomass_norm);
        assert!((0.0..=1.0).contains(&s1.roughness_norm));
    }

    #[test]
    fn cardinal_phi_peaks_at_optimum() {
        let card = test_cardinal();
        let phi_cold = cardinal_phi(8.0, &card);
        let phi_opt = cardinal_phi(30.0, &card);
        let phi_hot = cardinal_phi(42.0, &card);

        assert!(phi_opt >= phi_cold);
        assert!(phi_opt >= phi_hot);
        assert!((0.02..=1.0).contains(&phi_opt));
    }

    #[test]
    fn phi_shifted_by_colony_temp_opt_offset() {
        let card = test_cardinal();
        let at_28_no_shift = cardinal_phi_with_opt_offset(28.0, &card, 0.0);
        let at_28_shifted = cardinal_phi_with_opt_offset(28.0, &card, -2.0);
        assert!(at_28_shifted > at_28_no_shift);
    }
}
