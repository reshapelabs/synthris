use anyhow::Result;
use image::{Rgb, RgbImage, imageops::FilterType};

use super::codec::{decode_rgb_image, resize_rgb};
use crate::profiles::{IlluminationProfile, PlateProfile};
use crate::request::BackgroundMode;
use crate::roi::Roi;

pub fn full_canvas_roi(width: u32, height: u32) -> Roi {
    Roi::Rect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

pub fn resolve_render_roi(plate: Option<&PlateProfile>, width: u32, height: u32) -> Roi {
    let full = full_canvas_roi(width, height);

    let Some(plate) = plate else {
        return full;
    };

    let Some(image_path) = &plate.image_path else {
        return plate.roi.clone();
    };

    if !image_path.exists() {
        return plate.roi.clone();
    }

    let Ok((src_w, src_h)) = image::image_dimensions(image_path) else {
        return plate.roi.clone();
    };

    let fit = compute_fit(src_w, src_h, width, height);
    scale_roi_with_fit(&plate.roi, fit)
}

#[derive(Debug, Clone, Copy)]
struct FitTransform {
    scale: f32,
    offset_x: u32,
    offset_y: u32,
}

fn scale_roi_with_fit(roi: &Roi, fit: FitTransform) -> Roi {
    match roi {
        Roi::Circle { x, y, radius } => Roi::Circle {
            x: (fit.offset_x as f32 + ((*x as f32) * fit.scale)).round() as u32,
            y: (fit.offset_y as f32 + ((*y as f32) * fit.scale)).round() as u32,
            radius: ((*radius as f32) * fit.scale).max(1.0).round() as u32,
        },
        Roi::Rect {
            x,
            y,
            width,
            height,
        } => Roi::Rect {
            x: (fit.offset_x as f32 + ((*x as f32) * fit.scale)).round() as u32,
            y: (fit.offset_y as f32 + ((*y as f32) * fit.scale)).round() as u32,
            width: ((*width as f32) * fit.scale).max(1.0).round() as u32,
            height: ((*height as f32) * fit.scale).max(1.0).round() as u32,
        },
        Roi::Polygon { points } => Roi::Polygon {
            points: points
                .iter()
                .map(|(x, y)| {
                    (
                        (fit.offset_x as f32 + ((*x as f32) * fit.scale)).round() as u32,
                        (fit.offset_y as f32 + ((*y as f32) * fit.scale)).round() as u32,
                    )
                })
                .collect(),
        },
    }
}

fn compute_fit(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> FitTransform {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return FitTransform {
            scale: 1.0,
            offset_x: 0,
            offset_y: 0,
        };
    }

    let scale = (dst_w as f32 / src_w as f32).min(dst_h as f32 / src_h as f32);
    let fit_w = ((src_w as f32) * scale).max(1.0).round() as u32;
    let fit_h = ((src_h as f32) * scale).max(1.0).round() as u32;
    let offset_x = dst_w.saturating_sub(fit_w) / 2;
    let offset_y = dst_h.saturating_sub(fit_h) / 2;

    FitTransform {
        scale,
        offset_x,
        offset_y,
    }
}

pub fn render_background(
    width: u32,
    height: u32,
    mode: BackgroundMode,
    plate: Option<&PlateProfile>,
    illumination: &IlluminationProfile,
) -> Result<RgbImage> {
    let mut img = RgbImage::new(width, height);

    for (x, y, px) in img.enumerate_pixels_mut() {
        if mode == BackgroundMode::Blankfield {
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

    if mode == BackgroundMode::PlateImage {
        if let Some(path) = plate.and_then(|p| p.image_path.as_ref()) {
            if path.exists() {
                let src = decode_rgb_image(path)?;
                let fit = compute_fit(src.width(), src.height(), width, height);
                let fit_w = ((src.width() as f32) * fit.scale).max(1.0).round() as u32;
                let fit_h = ((src.height() as f32) * fit.scale).max(1.0).round() as u32;
                let fitted = resize_rgb(&src, fit_w, fit_h, FilterType::CatmullRom);
                image::imageops::replace(
                    &mut img,
                    &fitted,
                    fit.offset_x as i64,
                    fit.offset_y as i64,
                );
            }
        }
    }

    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::{FitTransform, compute_fit, scale_roi_with_fit};
    use crate::roi::Roi;

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

    #[test]
    fn roi_circle_is_scaled_with_offset() {
        let roi = Roi::Circle {
            x: 100,
            y: 120,
            radius: 50,
        };
        let mapped = scale_roi_with_fit(
            &roi,
            FitTransform {
                scale: 0.5,
                offset_x: 10,
                offset_y: 20,
            },
        );

        assert_eq!(
            mapped,
            Roi::Circle {
                x: 60,
                y: 80,
                radius: 25
            }
        );
    }
}
