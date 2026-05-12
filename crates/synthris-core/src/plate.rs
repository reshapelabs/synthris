use anyhow::{Result, bail};
use image::RgbImage;
use serde::{Deserialize, Serialize};

use crate::roi::Roi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}

impl ImageSize {
    pub fn validate(&self) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            bail!("image size width/height must be > 0");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct PlateBaseline {
    pub image: RgbImage,
    pub growth_area: Roi,
}

impl PlateBaseline {
    pub fn new(image: RgbImage, growth_area: Roi) -> Result<Self> {
        if image.width() == 0 || image.height() == 0 {
            bail!("plate baseline image width/height must be > 0");
        }
        growth_area.validate()?;
        Ok(Self { image, growth_area })
    }

    pub fn source_size(&self) -> ImageSize {
        ImageSize {
            width: self.image.width(),
            height: self.image.height(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SimulationBackground {
    Blankfield,
    PlateBaseline(PlateBaseline),
}

impl SimulationBackground {
    pub fn growth_area_for_canvas(&self, width: u32, height: u32) -> Roi {
        match self {
            SimulationBackground::Blankfield => Roi::Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            SimulationBackground::PlateBaseline(plate) => {
                let fit = FitTransform::compute(plate.image.width(), plate.image.height(), width, height);
                scale_roi_with_fit(&plate.growth_area, fit)
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FitTransform {
    pub scale: f32,
    pub offset_x: u32,
    pub offset_y: u32,
}

impl FitTransform {
    pub(crate) fn compute(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Self {
        if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
            return Self {
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

        Self {
            scale,
            offset_x,
            offset_y,
        }
    }
}

pub(crate) fn scale_roi_with_fit(roi: &Roi, fit: FitTransform) -> Roi {
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

#[cfg(test)]
mod tests {
    use image::RgbImage;

    use super::{FitTransform, PlateBaseline, SimulationBackground, scale_roi_with_fit};
    use crate::roi::Roi;

    #[test]
    fn fit_letterboxes_square_target() {
        let fit = FitTransform::compute(4000, 3000, 1024, 1024);
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

    #[test]
    fn blankfield_growth_area_is_full_canvas() {
        assert_eq!(
            SimulationBackground::Blankfield.growth_area_for_canvas(640, 480),
            Roi::Rect {
                x: 0,
                y: 0,
                width: 640,
                height: 480
            }
        );
    }

    #[test]
    fn plate_baseline_growth_area_is_scaled_to_canvas() {
        let plate = PlateBaseline::new(
            RgbImage::new(4000, 3000),
            Roi::Circle {
                x: 2000,
                y: 1500,
                radius: 1000,
            },
        )
        .expect("plate baseline");
        assert_eq!(
            SimulationBackground::PlateBaseline(plate).growth_area_for_canvas(1024, 1024),
            Roi::Circle {
                x: 512,
                y: 512,
                radius: 256
            }
        );
    }
}
