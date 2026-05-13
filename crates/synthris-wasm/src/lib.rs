use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use synthris_core::engine::FrameIterator;
use synthris_core::{
    BackgroundMode, CfuSpec, Engine, EngineConfig, LookPreset, OpacityClass, PhasePreset, ProfileDb,
    SimulationBackground, SimulationRequest, TemperatureSpec, TimeSpec,
};
use synthris_plate_assets::{BUILTIN_PLATE_BASELINES, builtin_plate_baseline_by_id};
use wasm_bindgen::prelude::*;

thread_local! {
    static STATE: RefCell<AppState> = RefCell::new(AppState::new());
}
static PANIC_HOOK: Once = Once::new();

#[derive(Default)]
struct AppState {
    next_id: u32,
    sims: HashMap<u32, SimulationState>,
}

impl AppState {
    fn new() -> Self {
        Self {
            next_id: 1,
            sims: HashMap::new(),
        }
    }
}

struct SimulationState {
    iter: FrameIterator,
    frame_count: usize,
}

#[derive(Debug, Deserialize)]
struct UiSimulationRequest {
    organism: String,
    illumination: String,
    temperature_c: f32,
    capture_interval_minutes: u64,
    grow_time_hours: u64,
    seed: u64,
    cfu: u32,
    width: Option<u32>,
    height: Option<u32>,
    background_mode: Option<String>,
    plate_baseline_id: Option<String>,
}

#[wasm_bindgen(getter_with_clone)]
pub struct WasmFrame {
    pub frame_index: u32,
    pub elapsed_seconds: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub done: bool,
}

#[wasm_bindgen]
pub fn list_organisms() -> Result<JsValue, JsValue> {
    let db = ProfileDb::builtin().map_err(as_js_err)?;
    let mut ids: Vec<String> = db.organisms.keys().cloned().collect();
    ids.sort();
    serde_json::to_string(&ids)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| as_js_err(anyhow!(e)))
}

#[wasm_bindgen]
pub fn list_illuminations() -> Result<JsValue, JsValue> {
    let db = ProfileDb::builtin().map_err(as_js_err)?;
    let mut ids: Vec<String> = db.illuminations.keys().cloned().collect();
    ids.sort();
    serde_json::to_string(&ids)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| as_js_err(anyhow!(e)))
}

#[wasm_bindgen]
pub fn list_plate_baselines() -> Result<JsValue, JsValue> {
    let mut ids: Vec<&str> = BUILTIN_PLATE_BASELINES.iter().map(|a| a.id).collect();
    ids.sort_unstable();
    serde_json::to_string(&ids)
        .map(|s| JsValue::from_str(&s))
        .map_err(|e| as_js_err(anyhow!(e)))
}

#[wasm_bindgen]
pub fn create_simulation(request_json: &str) -> Result<u32, JsValue> {
    PANIC_HOOK.call_once(console_error_panic_hook::set_once);
    let req: UiSimulationRequest =
        serde_json::from_str(request_json).context("invalid simulation request json").map_err(as_js_err)?;
    let db = ProfileDb::builtin().map_err(as_js_err)?;
    let sim_req = map_ui_request(&req);
    let background = resolve_background(&req).map_err(as_js_err)?;
    let organism = db
        .organism(&sim_req.organism_id)
        .ok_or_else(|| as_js_err(anyhow!("unknown organism profile: {}", sim_req.organism_id)))?;
    let illumination = db
        .illumination(&sim_req.illumination_id)
        .ok_or_else(|| as_js_err(anyhow!("unknown illumination profile: {}", sim_req.illumination_id)))?;

    let engine = Engine::new(EngineConfig::default());
    let iter = engine
        .frame_iter(&sim_req, organism, illumination, &background)
        .map_err(as_js_err)?;
    let frame_count = iter.frame_count();

    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.sims.insert(id, SimulationState { iter, frame_count });
        Ok(id)
    })
}

#[wasm_bindgen]
pub fn simulation_frame_count(sim_id: u32) -> Result<u32, JsValue> {
    STATE.with(|state| {
        let state = state.borrow();
        let sim = state
            .sims
            .get(&sim_id)
            .ok_or_else(|| JsValue::from_str("unknown simulation id"))?;
        Ok(sim.frame_count as u32)
    })
}

#[wasm_bindgen]
pub fn next_frame_rgba(sim_id: u32, frame_index: u32) -> Result<WasmFrame, JsValue> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let sim = state
            .sims
            .get_mut(&sim_id)
            .ok_or_else(|| JsValue::from_str("unknown simulation id"))?;

        let next = sim.iter.next_raw();
        let Some(raw) = next else {
            return Ok(WasmFrame {
                frame_index,
                elapsed_seconds: 0,
                width: 0,
                height: 0,
                rgba: Vec::new(),
                done: true,
            });
        };
        let raw = raw.map_err(as_js_err)?;
        let rgba = rgb_to_rgba(raw.rgb_bytes);
        Ok(WasmFrame {
            frame_index,
            elapsed_seconds: raw.elapsed_seconds.min(u32::MAX as u64) as u32,
            width: raw.width,
            height: raw.height,
            rgba,
            done: false,
        })
    })
}

#[wasm_bindgen]
pub fn render_frame_at_rgba(request_json: &str, frame_index: u32) -> Result<WasmFrame, JsValue> {
    PANIC_HOOK.call_once(console_error_panic_hook::set_once);
    let req: UiSimulationRequest =
        serde_json::from_str(request_json).context("invalid simulation request json").map_err(as_js_err)?;
    let db = ProfileDb::builtin().map_err(as_js_err)?;
    let mut sim_req = map_ui_request(&req);
    let background = resolve_background(&req).map_err(as_js_err)?;
    let organism = db
        .organism(&sim_req.organism_id)
        .ok_or_else(|| as_js_err(anyhow!("unknown organism profile: {}", sim_req.organism_id)))?;
    let illumination = db
        .illumination(&sim_req.illumination_id)
        .ok_or_else(|| as_js_err(anyhow!("unknown illumination profile: {}", sim_req.illumination_id)))?;

    let step_seconds = sim_req.time.step_seconds.max(1);
    let offset = step_seconds.saturating_mul(frame_index as u64);
    sim_req.time.start_after_seconds = sim_req.time.start_after_seconds.saturating_add(offset);
    sim_req.time.duration_seconds = 0;
    sim_req.time.step_seconds = 1;

    let engine = Engine::new(EngineConfig::default());
    let mut iter = engine
        .frame_iter(&sim_req, organism, illumination, &background)
        .map_err(as_js_err)?;
    let raw = iter
        .next_raw()
        .ok_or_else(|| JsValue::from_str("no frame produced"))?
        .map_err(as_js_err)?;
    let rgba = rgb_to_rgba(raw.rgb_bytes);
    Ok(WasmFrame {
        frame_index,
        elapsed_seconds: raw.elapsed_seconds.min(u32::MAX as u64) as u32,
        width: raw.width,
        height: raw.height,
        rgba,
        done: false,
    })
}

#[wasm_bindgen]
pub fn drop_simulation(sim_id: u32) {
    STATE.with(|state| {
        state.borrow_mut().sims.remove(&sim_id);
    });
}

#[wasm_bindgen]
pub fn reset_all_simulations() {
    STATE.with(|state| {
        state.borrow_mut().sims.clear();
    });
}

fn map_ui_request(req: &UiSimulationRequest) -> SimulationRequest {
    let background_mode = match req.background_mode.as_deref() {
        Some(raw) if raw.eq_ignore_ascii_case("blankfield") => BackgroundMode::Blankfield,
        _ => BackgroundMode::PlateImage,
    };
    SimulationRequest {
        organism_id: req.organism.clone(),
        illumination_id: req.illumination.clone(),
        background_mode,
        cfu: CfuSpec::Exact(req.cfu.max(1)),
        time: TimeSpec {
            start_after_seconds: 0,
            duration_seconds: req.grow_time_hours.saturating_mul(3600),
            step_seconds: req.capture_interval_minutes.max(1).saturating_mul(60),
        },
        temperature: TemperatureSpec {
            constant_c: req.temperature_c,
        },
        phase: PhasePreset::Mid,
        look: LookPreset::Realistic,
        opacity_class: OpacityClass::Standard,
        seed: req.seed,
        width: req.width.unwrap_or(512),
        height: req.height.unwrap_or(512),
        show_colony_ids: false,
        render_scale: 1.0,
    }
}

fn resolve_background(req: &UiSimulationRequest) -> Result<SimulationBackground> {
    let is_blankfield = matches!(
        req.background_mode.as_deref(),
        Some(raw) if raw.eq_ignore_ascii_case("blankfield")
    );
    if is_blankfield {
        return Ok(SimulationBackground::Blankfield);
    }

    let id = req
        .plate_baseline_id
        .as_deref()
        .unwrap_or("petridish-top-1");
    let asset = builtin_plate_baseline_by_id(id)
        .ok_or_else(|| anyhow!("unknown plate baseline id: {id}"))?;
    let baseline = asset.decode()?;
    Ok(SimulationBackground::PlateBaseline(baseline))
}

fn rgb_to_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    let mut i = 0usize;
    while i + 2 < rgb.len() {
        rgba.push(rgb[i]);
        rgba.push(rgb[i + 1]);
        rgba.push(rgb[i + 2]);
        rgba.push(255);
        i += 3;
    }
    rgba
}

fn as_js_err(err: anyhow::Error) -> JsValue {
    JsValue::from_str(&err.to_string())
}
