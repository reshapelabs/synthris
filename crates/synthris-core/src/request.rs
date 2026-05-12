use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IlluminationMode {
    Frontlit,
    Backlit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMode {
    PlateImage,
    Blankfield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhasePreset {
    Early,
    Mid,
    Late,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookPreset {
    Clean,
    Realistic,
    Gritty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpacityClass {
    Translucent,
    Standard,
    Dense,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CfuSpec {
    Exact(u32),
    Range { min: u32, max: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSpec {
    pub start_after_seconds: u64,
    pub duration_seconds: u64,
    pub step_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemperatureSpec {
    pub constant_c: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationRequest {
    pub organism_id: String,
    pub illumination_id: String,
    pub background_mode: BackgroundMode,
    pub cfu: CfuSpec,
    pub time: TimeSpec,
    pub temperature: TemperatureSpec,
    pub phase: PhasePreset,
    pub look: LookPreset,
    pub opacity_class: OpacityClass,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub show_colony_ids: bool,
    #[serde(default = "default_render_scale")]
    pub render_scale: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColonyAnnotation {
    pub x: i32,
    pub y: i32,
    pub radius_px: f32,
    pub phenotype_idx: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedFrame {
    pub elapsed_seconds: u64,
    pub image_jpeg_bytes: Vec<u8>,
    pub annotations: Vec<ColonyAnnotation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulationManifest {
    pub organism_id: String,
    pub illumination_id: String,
    pub background_mode: BackgroundMode,
    pub cfu_count: u32,
    pub temperature_c: f32,
    pub start_after_seconds: u64,
    pub duration_seconds: u64,
    pub step_seconds: u64,
    pub elapsed_seconds: Vec<u64>,
    pub render_scale: f32,
}

fn default_render_scale() -> f32 {
    1.0
}
