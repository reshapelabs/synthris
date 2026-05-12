use anyhow::{Context, Result, bail};
use synthris_core::{ImageSize, PlateBaseline, Roi};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateType {
    PetriDish,
    OmniTray,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlateView {
    Top,
    Bottom,
}

#[derive(Debug, Clone)]
pub struct PlateBaselineAsset {
    pub id: &'static str,
    pub plate_type: PlateType,
    pub view: PlateView,
    pub source_size: ImageSize,
    pub image_jpeg: &'static [u8],
    pub growth_area: Roi,
}

impl PlateBaselineAsset {
    pub fn decode(self) -> Result<PlateBaseline> {
        self.source_size.validate()?;
        let image = image::load_from_memory(self.image_jpeg)
            .with_context(|| format!("failed decoding plate baseline asset '{}'", self.id))?
            .to_rgb8();
        if image.width() != self.source_size.width || image.height() != self.source_size.height {
            bail!(
                "plate baseline asset '{}' declared {}x{} but decoded {}x{}",
                self.id,
                self.source_size.width,
                self.source_size.height,
                image.width(),
                image.height()
            );
        }
        PlateBaseline::new(image, self.growth_area)
    }
}

pub const PETRIDISH_TOP_1: PlateBaselineAsset = PlateBaselineAsset {
    id: "petridish-top-1",
    plate_type: PlateType::PetriDish,
    view: PlateView::Top,
    source_size: ImageSize {
        width: 3434,
        height: 3434,
    },
    image_jpeg: include_bytes!("../assets/top/petridish-1.jpeg"),
    growth_area: Roi::Circle {
        x: 1750,
        y: 1750,
        radius: 1264,
    },
};

pub const PETRIDISH_BOTTOM_1: PlateBaselineAsset = PlateBaselineAsset {
    id: "petridish-bottom-1",
    plate_type: PlateType::PetriDish,
    view: PlateView::Bottom,
    source_size: ImageSize {
        width: 3434,
        height: 3434,
    },
    image_jpeg: include_bytes!("../assets/bottom/petridish-1.jpeg"),
    growth_area: Roi::Circle {
        x: 1750,
        y: 1750,
        radius: 1264,
    },
};

pub const OMNITRAY_TOP_1: PlateBaselineAsset = PlateBaselineAsset {
    id: "omnitray-top-1",
    plate_type: PlateType::OmniTray,
    view: PlateView::Top,
    source_size: ImageSize {
        width: 3270,
        height: 4362,
    },
    image_jpeg: include_bytes!("../assets/top/omnitray-1.jpeg"),
    growth_area: Roi::Rect {
        x: 582,
        y: 444,
        width: 2170,
        height: 3426,
    },
};

pub const OMNITRAY_BOTTOM_1: PlateBaselineAsset = PlateBaselineAsset {
    id: "omnitray-bottom-1",
    plate_type: PlateType::OmniTray,
    view: PlateView::Bottom,
    source_size: ImageSize {
        width: 3270,
        height: 4362,
    },
    image_jpeg: include_bytes!("../assets/bottom/omnitray-1.jpeg"),
    growth_area: Roi::Rect {
        x: 582,
        y: 444,
        width: 2170,
        height: 3426,
    },
};

pub const BUILTIN_PLATE_BASELINES: &[PlateBaselineAsset] = &[
    PETRIDISH_TOP_1,
    PETRIDISH_BOTTOM_1,
    OMNITRAY_TOP_1,
    OMNITRAY_BOTTOM_1,
];

pub fn builtin_plate_baseline_by_id(id: &str) -> Option<PlateBaselineAsset> {
    BUILTIN_PLATE_BASELINES
        .iter()
        .cloned()
        .find(|asset| asset.id == id)
}

pub fn builtin_plate_baseline(plate_type: PlateType, view: PlateView) -> Option<PlateBaselineAsset> {
    BUILTIN_PLATE_BASELINES
        .iter()
        .cloned()
        .find(|asset| asset.plate_type == plate_type && asset.view == view)
}

pub fn parse_plate_type(raw: &str) -> Result<PlateType> {
    if raw.eq_ignore_ascii_case("Petri Dish") {
        return Ok(PlateType::PetriDish);
    }
    if raw.eq_ignore_ascii_case("OmniTray") {
        return Ok(PlateType::OmniTray);
    }
    bail!("unknown plate type id: {raw}")
}

#[cfg(test)]
mod tests {
    use super::{BUILTIN_PLATE_BASELINES, PlateType, PlateView, builtin_plate_baseline};

    #[test]
    fn builtin_assets_decode() {
        for asset in BUILTIN_PLATE_BASELINES {
            let plate = asset.clone().decode().expect("asset decodes");
            assert_eq!(plate.source_size(), asset.source_size);
        }
    }

    #[test]
    fn resolves_by_plate_type_and_view() {
        let asset = builtin_plate_baseline(PlateType::PetriDish, PlateView::Top).expect("asset");
        assert_eq!(asset.id, "petridish-top-1");
    }
}
