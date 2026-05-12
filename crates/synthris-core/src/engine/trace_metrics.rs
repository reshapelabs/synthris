use image::RgbImage;

use super::models::ModelBundle;
use super::seeding::ColonySeed;
use super::shape::GeometryInput;
use super::{GrowthTraceSample, SimulationRequest, paint_colony, percentile_sorted};
use crate::roi::Roi;

pub(super) fn compute_trace_sample(
    request: &SimulationRequest,
    elapsed_seconds: u64,
    t_h: f32,
    seeded_colonies: u32,
    width: u32,
    height: u32,
    roi_pixels: u32,
    roi: &Roi,
    colony_seeds: &[ColonySeed],
    models: &ModelBundle,
    background_template: &RgbImage,
    frame_img: &mut RgbImage,
) -> GrowthTraceSample {
    frame_img
        .as_mut()
        .copy_from_slice(background_template.as_raw());

    let mut radii = Vec::with_capacity(colony_seeds.len());
    let mut occupied_mask = vec![false; (width as usize) * (height as usize)];
    let mut occupied_pixels = 0u32;
    for colony in colony_seeds {
        let age_h = (t_h - colony.onset_h).max(0.0);
        let growth_state = models.growth.eval_state(
            age_h,
            request.temperature.constant_c,
            request.phase,
            colony.temp_opt_offset_c,
        );
        let radius = growth_state.radius_px;
        if radius <= 0.05 {
            continue;
        }
        radii.push(radius);

        let radius_i = radius.max(1.0) as i32;
        let min_x = (colony.x - radius_i).max(0);
        let max_x = (colony.x + radius_i).min((width.saturating_sub(1)) as i32);
        let min_y = (colony.y - radius_i).max(0);
        let max_y = (colony.y + radius_i).min((height.saturating_sub(1)) as i32);
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

                let idx = y as usize * width as usize + x as usize;
                if !occupied_mask[idx] {
                    occupied_mask[idx] = true;
                    occupied_pixels += 1;
                }
            }
        }

        paint_colony(
            frame_img,
            colony,
            age_h,
            growth_state,
            request.temperature.constant_c,
            request.opacity_class,
            request.look,
            roi,
            models,
        );
    }

    radii.sort_by(f32::total_cmp);
    let visible_colonies = radii.len() as u32;
    let radius_sum: f32 = radii.iter().sum();
    let mean_radius_px = if visible_colonies == 0 {
        0.0
    } else {
        radius_sum / visible_colonies as f32
    };
    let variance_radius_px = if visible_colonies == 0 {
        0.0
    } else {
        radii
            .iter()
            .map(|radius| {
                let delta = radius - mean_radius_px;
                delta * delta
            })
            .sum::<f32>()
            / visible_colonies as f32
    };
    let (mean_colony_r, mean_colony_g, mean_colony_b) = if occupied_pixels == 0 {
        (0.0, 0.0, 0.0)
    } else {
        let rgb = frame_img.as_raw();
        let mut sum_r = 0.0f32;
        let mut sum_g = 0.0f32;
        let mut sum_b = 0.0f32;
        for (idx, occupied) in occupied_mask.iter().enumerate() {
            if !occupied {
                continue;
            }
            let off = idx * 3;
            sum_r += rgb[off] as f32;
            sum_g += rgb[off + 1] as f32;
            sum_b += rgb[off + 2] as f32;
        }
        let denom = occupied_pixels as f32;
        (sum_r / denom, sum_g / denom, sum_b / denom)
    };

    GrowthTraceSample {
        elapsed_seconds,
        seeded_colonies,
        visible_colonies,
        occupied_pixels,
        roi_pixels,
        occupied_area_pct_of_roi: if roi_pixels == 0 {
            0.0
        } else {
            occupied_pixels as f32 * 100.0 / roi_pixels as f32
        },
        mean_colony_r,
        mean_colony_g,
        mean_colony_b,
        mean_radius_px,
        median_radius_px: percentile_sorted(&radii, 0.5),
        p90_radius_px: percentile_sorted(&radii, 0.9),
        stddev_radius_px: variance_radius_px.sqrt(),
        max_radius_px: radii.last().copied().unwrap_or(0.0),
    }
}
