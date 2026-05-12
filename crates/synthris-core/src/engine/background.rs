use anyhow::Result;
use image::{Rgb, RgbImage, imageops::FilterType};

use super::codec::resize_rgb;
use crate::plate::{FitTransform, SimulationBackground};
use crate::profiles::IlluminationProfile;
use crate::roi::Roi;

fn compute_fit(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> FitTransform {
    FitTransform::compute(src_w, src_h, dst_w, dst_h)
}

pub fn render_background(
    width: u32,
    height: u32,
    background: &SimulationBackground,
    illumination: &IlluminationProfile,
) -> Result<(RgbImage, Roi)> {
    let mut img = RgbImage::new(width, height);

    for (x, y, px) in img.enumerate_pixels_mut() {
        if matches!(background, SimulationBackground::Blankfield) {
            *px = Rgb([0, 0, 0]);
            continue;
        }

        let grain = (((x + 3 * y) % 19) as i16) - 9;
        let base = illumination.background_rgb;

        let apply = |c: u8| -> u8 {
            let v = c as i16 + grain;
            v.clamp(0, 255) as u8
        };

        *px = Rgb([apply(base[0]), apply(base[1]), apply(base[2])]);
    }

    if let SimulationBackground::PlateBaseline(plate) = background {
        let src = &plate.image;
        let fit = compute_fit(src.width(), src.height(), width, height);
        let fit_w = ((src.width() as f32) * fit.scale).max(1.0).round() as u32;
        let fit_h = ((src.height() as f32) * fit.scale).max(1.0).round() as u32;
        let fitted = resize_rgb(src, fit_w, fit_h, FilterType::CatmullRom);
        image::imageops::replace(&mut img, &fitted, fit.offset_x as i64, fit.offset_y as i64);
    }

    Ok((img, background.growth_area_for_canvas(width, height)))
}

#[cfg(test)]
mod tests {
    use super::compute_fit;

    #[test]
    fn compute_fit_letterboxes_square_target() {
        let fit = compute_fit(4000, 3000, 1024, 1024);
        let fit_w = (4000.0 * fit.scale).round() as u32;
        let fit_h = (3000.0 * fit.scale).round() as u32;

        assert_eq!(fit_w, 1024);
        assert_eq!(fit_h, 768);
        assert_eq!(fit.offset_x, 0);
        assert_eq!(fit.offset_y, 128);
    }
}
