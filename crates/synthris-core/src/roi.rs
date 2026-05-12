use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Roi {
    Circle {
        x: u32,
        y: u32,
        radius: u32,
    },
    Rect {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    Polygon {
        points: Vec<(u32, u32)>,
    },
}

impl Roi {
    pub fn validate(&self) -> Result<()> {
        match self {
            Roi::Circle { radius, .. } if *radius == 0 => bail!("circle radius must be > 0"),
            Roi::Rect { width, height, .. } if *width == 0 || *height == 0 => {
                bail!("rect width/height must be > 0")
            }
            Roi::Polygon { points } if points.len() < 3 => {
                bail!("polygon roi requires at least 3 points")
            }
            _ => Ok(()),
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        match self {
            Roi::Circle {
                x: cx,
                y: cy,
                radius,
            } => {
                let dx = x - (*cx as i32);
                let dy = y - (*cy as i32);
                dx * dx + dy * dy <= (*radius as i32) * (*radius as i32)
            }
            Roi::Rect {
                x: rx,
                y: ry,
                width,
                height,
            } => {
                x >= *rx as i32
                    && y >= *ry as i32
                    && x < (*rx + *width) as i32
                    && y < (*ry + *height) as i32
            }
            Roi::Polygon { points } => point_in_polygon(x, y, points),
        }
    }

    pub fn bounds(&self) -> (u32, u32, u32, u32) {
        match self {
            Roi::Circle { x, y, radius } => {
                let min_x = x.saturating_sub(*radius);
                let min_y = y.saturating_sub(*radius);
                let size = radius.saturating_mul(2);
                (min_x, min_y, size, size)
            }
            Roi::Rect {
                x,
                y,
                width,
                height,
            } => (*x, *y, *width, *height),
            Roi::Polygon { points } => {
                if points.is_empty() {
                    return (0, 0, 0, 0);
                }

                let mut min_x = u32::MAX;
                let mut min_y = u32::MAX;
                let mut max_x = 0u32;
                let mut max_y = 0u32;

                for (x, y) in points {
                    min_x = min_x.min(*x);
                    min_y = min_y.min(*y);
                    max_x = max_x.max(*x);
                    max_y = max_y.max(*y);
                }

                (
                    min_x,
                    min_y,
                    max_x.saturating_sub(min_x),
                    max_y.saturating_sub(min_y),
                )
            }
        }
    }
}

fn point_in_polygon(x: i32, y: i32, points: &[(u32, u32)]) -> bool {
    if points.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (xi, yi) = (points[i].0 as i32, points[i].1 as i32);
        let (xj, yj) = (points[j].0 as i32, points[j].1 as i32);

        let intersects =
            ((yi > y) != (yj > y)) && (x < (xj - xi) * (y - yi) / ((yj - yi).max(1)) + xi);

        if intersects {
            inside = !inside;
        }
        j = i;
    }

    inside
}
