use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use pprof::{ProfilerGuard, protos::Message};
use synthris_core::{
    BackgroundMode, CfuSpec, Engine, EngineConfig, IlluminationProfile, LookPreset, OpacityClass,
    OrganismProfile, PhasePreset, PlateBaseline, ProfileDb, ProfileDbConfig,
    SimulationBackground, SimulationRequest, TemperatureSpec, TimeSpec,
};
use synthris_plate_assets::{BUILTIN_PLATE_BASELINES, builtin_plate_baseline_by_id};
use tokio::fs;
use tracing::info;
use tracing_subscriber::EnvFilter;
mod video;
use video::{VideoCodec, VideoEncodeOptions, encode_raw_stream};

#[derive(Debug, Parser)]
#[command(name = "synthris")]
#[command(about = "Synthris Rust CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Generate {
        #[arg(short = 'o', long)]
        out: PathBuf,
        #[arg(long, default_value = "jpeg-seq", value_enum)]
        output_format: OutputFormatArg,
        #[arg(long, default_value_t = 12)]
        fps: u32,
        #[arg(short = 'g', long)]
        organism: Option<String>,
        #[arg(short = 'c', long)]
        cfu: Option<String>,
        #[arg(short = 'i', long)]
        illumination: Option<String>,
        #[arg(short = 'p', long)]
        plate_profile: Option<String>,
        #[arg(
            short = 'b',
            long = "background-mode",
            visible_alias = "bg",
            value_enum
        )]
        background_mode: Option<BackgroundModeArg>,
        #[arg(short = 'd', long)]
        duration: String,
        #[arg(short = 's', long)]
        step: String,
        #[arg(long = "start-after", default_value = "0s")]
        start_after: String,
        #[arg(short = 't', long, default_value_t = 30.0)]
        temperature_c: f32,
        #[arg(long, value_enum)]
        phase: Option<PhaseArg>,
        #[arg(long, value_enum)]
        look: Option<LookArg>,
        #[arg(long, value_enum)]
        opacity_class: Option<OpacityClassArg>,
        #[arg(short = 'S', long, default_value_t = 42)]
        seed: u64,
        #[arg(short = 'w', long)]
        width: Option<u32>,
        #[arg(short = 'H', long)]
        height: Option<u32>,
        #[arg(long, default_value_t = 1.0)]
        render_scale: f32,
        #[arg(
            short = 'I',
            long = "show-colony-ids",
            visible_alias = "ids",
            default_value_t = false
        )]
        show_colony_ids: bool,
        #[arg(short = 'P', long = "profile-dir")]
        profile_dirs: Vec<PathBuf>,
    },
    Trace {
        #[arg(short = 'o', long)]
        out: Option<PathBuf>,
        #[arg(short = 'g', long)]
        organism: Option<String>,
        #[arg(short = 'c', long)]
        cfu: Option<String>,
        #[arg(short = 'i', long)]
        illumination: Option<String>,
        #[arg(short = 'p', long)]
        plate_profile: Option<String>,
        #[arg(
            short = 'b',
            long = "background-mode",
            visible_alias = "bg",
            value_enum
        )]
        background_mode: Option<BackgroundModeArg>,
        #[arg(short = 'd', long)]
        duration: String,
        #[arg(short = 's', long)]
        step: String,
        #[arg(long = "start-after", default_value = "0s")]
        start_after: String,
        #[arg(short = 't', long, default_value_t = 30.0)]
        temperature_c: f32,
        #[arg(long, value_enum)]
        phase: Option<PhaseArg>,
        #[arg(long, value_enum)]
        look: Option<LookArg>,
        #[arg(long, value_enum)]
        opacity_class: Option<OpacityClassArg>,
        #[arg(short = 'S', long, default_value_t = 42)]
        seed: u64,
        #[arg(short = 'w', long)]
        width: Option<u32>,
        #[arg(short = 'H', long)]
        height: Option<u32>,
        #[arg(long, default_value_t = 1.0)]
        render_scale: f32,
        #[arg(short = 'P', long = "profile-dir")]
        profile_dirs: Vec<PathBuf>,
    },
    Plate {
        #[command(subcommand)]
        command: PlateCommand,
    },
    Perf {
        #[command(subcommand)]
        command: PerfCommand,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BackgroundModeArg {
    Plate,
    Blankfield,
}

impl From<BackgroundModeArg> for BackgroundMode {
    fn from(value: BackgroundModeArg) -> Self {
        match value {
            BackgroundModeArg::Plate => BackgroundMode::PlateImage,
            BackgroundModeArg::Blankfield => BackgroundMode::Blankfield,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormatArg {
    JpegSeq,
    Mp4H264,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PhaseArg {
    Early,
    Mid,
    Late,
}

impl From<PhaseArg> for PhasePreset {
    fn from(value: PhaseArg) -> Self {
        match value {
            PhaseArg::Early => PhasePreset::Early,
            PhaseArg::Mid => PhasePreset::Mid,
            PhaseArg::Late => PhasePreset::Late,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LookArg {
    Clean,
    Realistic,
    Gritty,
}

impl From<LookArg> for LookPreset {
    fn from(value: LookArg) -> Self {
        match value {
            LookArg::Clean => LookPreset::Clean,
            LookArg::Realistic => LookPreset::Realistic,
            LookArg::Gritty => LookPreset::Gritty,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OpacityClassArg {
    Translucent,
    Standard,
    Dense,
}

impl From<OpacityClassArg> for OpacityClass {
    fn from(value: OpacityClassArg) -> Self {
        match value {
            OpacityClassArg::Translucent => OpacityClass::Translucent,
            OpacityClassArg::Standard => OpacityClass::Standard,
            OpacityClassArg::Dense => OpacityClass::Dense,
        }
    }
}

#[derive(Debug, Subcommand)]
enum PlateCommand {
    List {
        #[arg(short = 'P', long = "profile-dir")]
        profile_dirs: Vec<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum PerfCommand {
    Baseline {
        #[arg(short = 'o', long)]
        out: PathBuf,
        #[arg(short = 'g', long)]
        organism: String,
        #[arg(short = 'i', long)]
        illumination: String,
        #[arg(short = 'p', long)]
        plate_profile: String,
        #[arg(short = 'd', long, default_value = "2d")]
        duration: String,
        #[arg(short = 's', long, default_value = "1h")]
        step: String,
        #[arg(long = "start-after", default_value = "0s")]
        start_after: String,
        #[arg(long, default_value_t = 30.0)]
        temperature_c: f32,
        #[arg(short = 'c', long, default_value = "50")]
        cfu: String,
        #[arg(short = 'n', long, default_value_t = 3)]
        runs: u32,
        #[arg(short = 'P', long = "profile-dir")]
        profile_dirs: Vec<PathBuf>,
    },
    Profile {
        #[arg(short = 'o', long)]
        out: PathBuf,
        #[arg(short = 'g', long)]
        organism: String,
        #[arg(short = 'i', long)]
        illumination: String,
        #[arg(short = 'p', long)]
        plate_profile: String,
        #[arg(short = 'd', long, default_value = "2d")]
        duration: String,
        #[arg(short = 's', long, default_value = "1h")]
        step: String,
        #[arg(long = "start-after", default_value = "0s")]
        start_after: String,
        #[arg(long, default_value_t = 30.0)]
        temperature_c: f32,
        #[arg(short = 'c', long, default_value = "50")]
        cfu: String,
        #[arg(long, default_value_t = 100)]
        sample_hz: i32,
        #[arg(long, default_value = "target/perf")]
        profile_out: PathBuf,
        #[arg(short = 'P', long = "profile-dir")]
        profile_dirs: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Generate {
            out,
            output_format,
            fps,
            organism,
            cfu,
            illumination,
            plate_profile,
            background_mode,
            duration,
            step,
            start_after,
            temperature_c,
            phase,
            look,
            opacity_class,
            seed,
            width,
            height,
            render_scale,
            show_colony_ids,
            profile_dirs,
        } => {
            run_generate(
                &out,
                output_format,
                fps,
                organism,
                cfu,
                illumination,
                plate_profile,
                background_mode,
                duration,
                step,
                start_after,
                temperature_c,
                phase,
                look,
                opacity_class,
                seed,
                width,
                height,
                render_scale,
                show_colony_ids,
                &profile_dirs,
            )
            .await
        }
        Command::Trace {
            out,
            organism,
            cfu,
            illumination,
            plate_profile,
            background_mode,
            duration,
            step,
            start_after,
            temperature_c,
            phase,
            look,
            opacity_class,
            seed,
            width,
            height,
            render_scale,
            profile_dirs,
        } => {
            run_trace(
                out.as_deref(),
                organism,
                cfu,
                illumination,
                plate_profile,
                background_mode,
                duration,
                step,
                start_after,
                temperature_c,
                phase,
                look,
                opacity_class,
                seed,
                width,
                height,
                render_scale,
                &profile_dirs,
            )
            .await
        }
        Command::Plate { command } => run_plate(command).await,
        Command::Perf { command } => run_perf(command).await,
    }
}

struct RequestBuildOptions {
    organism: Option<String>,
    cfu: Option<String>,
    illumination: Option<String>,
    plate_baseline: Option<String>,
    background_mode: Option<BackgroundModeArg>,
    duration: String,
    step: String,
    start_after: String,
    temperature_c: f32,
    phase: Option<PhaseArg>,
    look: Option<LookArg>,
    opacity_class: Option<OpacityClassArg>,
    seed: u64,
    width: Option<u32>,
    height: Option<u32>,
    render_scale: f32,
    show_colony_ids: bool,
}

#[allow(clippy::too_many_arguments)]
async fn run_generate(
    out: &Path,
    output_format: OutputFormatArg,
    fps: u32,
    organism: Option<String>,
    cfu: Option<String>,
    illumination: Option<String>,
    plate_profile: Option<String>,
    background_mode: Option<BackgroundModeArg>,
    duration: String,
    step: String,
    start_after: String,
    temperature_c: f32,
    phase: Option<PhaseArg>,
    look: Option<LookArg>,
    opacity_class: Option<OpacityClassArg>,
    seed: u64,
    width: Option<u32>,
    height: Option<u32>,
    render_scale: f32,
    show_colony_ids: bool,
    profile_dirs: &[PathBuf],
) -> Result<()> {
    let db = load_profile_db(profile_dirs)?;
    let (config, req, plate_baseline_id) = build_simulation_request(
        RequestBuildOptions {
            organism,
            cfu,
            illumination,
            plate_baseline: plate_profile,
            background_mode,
            duration,
            step,
            start_after,
            temperature_c,
            phase,
            look,
            opacity_class,
            seed,
            width,
            height,
            render_scale,
            show_colony_ids,
        },
        true,
    )?;

    let engine = Engine::new(config);
    let (organism, illumination_profile, background) =
        resolve_render_inputs(&db, &req, plate_baseline_id.as_deref())?;
    let mut frame_iter = engine.frame_iter(&req, &organism, &illumination_profile, &background)?;
    let manifest = frame_iter.manifest();
    let frame_count = frame_iter.frame_count();

    let manifest_path = match output_format {
        OutputFormatArg::JpegSeq => {
            fs::create_dir_all(out).await?;
            for frame in &mut frame_iter {
                let frame = frame?;
                let filename = elapsed_filename(frame.elapsed_seconds);
                fs::write(out.join(filename), frame.image_jpeg_bytes).await?;
            }
            out.join("manifest.json")
        }
        OutputFormatArg::Mp4H264 => {
            if frame_count == 0 {
                bail!("cannot encode empty frame sequence");
            }
            let is_mp4_file = out
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("mp4"))
                .unwrap_or(false);

            let (video_path, manifest_path) = if is_mp4_file {
                let manifest = out.with_extension("manifest.json");
                (out.to_path_buf(), manifest)
            } else {
                (out.join("video.mp4"), out.join("manifest.json"))
            };

            if let Some(parent) = video_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            let (width, height) = frame_iter.frame_dimensions();
            let mut frames_written = 0usize;
            encode_raw_stream(
                &video_path,
                VideoEncodeOptions {
                    fps,
                    codec: VideoCodec::Mp4H264,
                },
                width,
                height,
                |stdin| {
                    let expected_len = width as usize * height as usize * 3;
                    while let Some(frame) = frame_iter.next_raw() {
                        let frame = frame?;
                        if frame.rgb_bytes.len() != expected_len {
                            bail!(
                                "raw frame byte length mismatch: got {}, expected {}",
                                frame.rgb_bytes.len(),
                                expected_len
                            );
                        }
                        stdin.write_all(frame.rgb_bytes)?;
                        frames_written += 1;
                    }
                    Ok(())
                },
            )?;
            println!(
                "generated {} frames as mp4-h264: {}",
                frames_written,
                video_path.display()
            );
            manifest_path
        }
    };

    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?).await?;

    match output_format {
        OutputFormatArg::JpegSeq => {
            println!("generated {} snapshots in {}", frame_count, out.display());
        }
        OutputFormatArg::Mp4H264 => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn run_trace(
    out: Option<&Path>,
    organism: Option<String>,
    cfu: Option<String>,
    illumination: Option<String>,
    plate_profile: Option<String>,
    background_mode: Option<BackgroundModeArg>,
    duration: String,
    step: String,
    start_after: String,
    temperature_c: f32,
    phase: Option<PhaseArg>,
    look: Option<LookArg>,
    opacity_class: Option<OpacityClassArg>,
    seed: u64,
    width: Option<u32>,
    height: Option<u32>,
    render_scale: f32,
    profile_dirs: &[PathBuf],
) -> Result<()> {
    let db = load_profile_db(profile_dirs)?;
    let (config, req, plate_baseline_id) = build_simulation_request(
        RequestBuildOptions {
            organism,
            cfu,
            illumination,
            plate_baseline: plate_profile,
            background_mode,
            duration,
            step,
            start_after,
            temperature_c,
            phase,
            look,
            opacity_class,
            seed,
            width,
            height,
            render_scale,
            show_colony_ids: false,
        },
        false,
    )?;

    let engine = Engine::new(config);
    let (organism, illumination_profile, background) =
        resolve_render_inputs(&db, &req, plate_baseline_id.as_deref())?;
    let mut jsonl = Vec::new();
    for sample in engine.trace_iter(&req, &organism, &illumination_profile, &background)? {
        serde_json::to_writer(&mut jsonl, &sample)?;
        jsonl.push(b'\n');
    }

    if let Some(out) = out {
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::write(out, jsonl).await?;
    } else {
        io::stdout().write_all(&jsonl)?;
    }

    Ok(())
}

fn build_simulation_request(
    options: RequestBuildOptions,
    warn_missing_source_dims: bool,
) -> Result<(EngineConfig, SimulationRequest, Option<String>)> {
    let phase_preset = options.phase.map(Into::into);
    let look_preset = options.look.map(Into::into);
    let opacity = options.opacity_class.map(Into::into);
    let background_mode_resolved = options
        .background_mode
        .map(Into::into)
        .unwrap_or(BackgroundMode::PlateImage);

    let organism_id = options.organism.context("--organism is required")?;
    let illumination_id = options.illumination.context("--illumination is required")?;
    let cfu_spec_raw = options.cfu.context("--cfu is required")?;

    let cfu_spec = parse_cfu_spec(&cfu_spec_raw)?;
    let duration_seconds = parse_time_span(&options.duration)?;
    let step_seconds = parse_time_span(&options.step)?;
    let start_after_seconds = parse_time_span_allow_zero(&options.start_after)?;

    let plate_baseline_id = match background_mode_resolved {
        BackgroundMode::Blankfield => None,
        BackgroundMode::PlateImage => Some(
            options
                .plate_baseline
                .context("--plate-profile is required for plate mode")?,
        ),
    };

    let config = EngineConfig::default();
    let source_dims = source_dims_for_request(background_mode_resolved, plate_baseline_id.as_deref());
    if warn_missing_source_dims
        && background_mode_resolved == BackgroundMode::PlateImage
        && source_dims.is_none()
        && (options.width.is_none() || options.height.is_none())
    {
        eprintln!(
            "warning: plate source dimensions unavailable; using fallback output size defaults"
        );
    }
    let (width, height) = resolve_output_size(
        options.width,
        options.height,
        source_dims,
        config.default_width,
        config.default_height,
    );

    Ok((
        config,
        SimulationRequest {
            organism_id,
            illumination_id,
            background_mode: background_mode_resolved,
            cfu: cfu_spec,
            time: TimeSpec {
                start_after_seconds,
                duration_seconds,
                step_seconds,
            },
            temperature: TemperatureSpec {
                constant_c: options.temperature_c,
            },
            phase: phase_preset.unwrap_or(PhasePreset::Mid),
            look: look_preset.unwrap_or(LookPreset::Realistic),
            opacity_class: opacity.unwrap_or(OpacityClass::Standard),
            seed: options.seed,
            width,
            height,
            show_colony_ids: options.show_colony_ids,
            render_scale: options.render_scale.clamp(0.05, 4.0),
        },
        plate_baseline_id,
    ))
}

fn resolve_render_inputs(
    db: &ProfileDb,
    req: &SimulationRequest,
    plate_baseline_id: Option<&str>,
) -> Result<(OrganismProfile, IlluminationProfile, SimulationBackground)> {
    let organism = db
        .organism(&req.organism_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown organism profile: {}", req.organism_id))?;
    let illumination = db
        .illumination(&req.illumination_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown illumination profile: {}", req.illumination_id))?;

    let background = match req.background_mode {
        BackgroundMode::Blankfield => SimulationBackground::Blankfield,
        BackgroundMode::PlateImage => {
            let id = plate_baseline_id.context("--plate-profile is required for plate mode")?;
            let asset = builtin_plate_baseline_by_id(id)
                .ok_or_else(|| anyhow::anyhow!("unknown plate baseline: {id}"))?;
            let plate: PlateBaseline = asset.decode()?;
            SimulationBackground::PlateBaseline(plate)
        }
    };

    Ok((organism, illumination, background))
}

async fn run_plate(command: PlateCommand) -> Result<()> {
    match command {
        PlateCommand::List { profile_dirs: _ } => {
            let mut items: Vec<_> = BUILTIN_PLATE_BASELINES.iter().collect();
            items.sort_by(|a, b| a.id.cmp(b.id));
            for item in items {
                println!(
                    "{}\t{:?}\t{:?}\t{}x{}",
                    item.id,
                    item.plate_type,
                    item.view,
                    item.source_size.width,
                    item.source_size.height
                );
            }
            Ok(())
        }
    }
}

async fn run_perf(command: PerfCommand) -> Result<()> {
    init_tracing();
    match command {
        PerfCommand::Baseline {
            out,
            organism,
            illumination,
            plate_profile,
            duration,
            step,
            start_after,
            temperature_c,
            cfu,
            runs,
            profile_dirs,
        } => {
            let runs = runs.max(1);
            let mut times_ms = Vec::with_capacity(runs as usize);
            for idx in 0..runs {
                let run_out = out.join(format!("run_{:03}", idx + 1));
                let start = Instant::now();
                run_generate(
                    &run_out,
                    OutputFormatArg::JpegSeq,
                    12,
                    Some(organism.clone()),
                    Some(cfu.clone()),
                    Some(illumination.clone()),
                    Some(plate_profile.clone()),
                    Some(BackgroundModeArg::Plate),
                    duration.clone(),
                    step.clone(),
                    start_after.clone(),
                    temperature_c,
                    None,
                    None,
                    None,
                    42 + idx as u64,
                    None,
                    None,
                    1.0,
                    false,
                    &profile_dirs,
                )
                .await?;
                let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
                times_ms.push(elapsed_ms);
                info!(run = idx + 1, elapsed_ms, "perf baseline run complete");
            }

            let min_ms = times_ms.iter().copied().fold(f64::INFINITY, f64::min);
            let max_ms = times_ms.iter().copied().fold(0.0, f64::max);
            let avg_ms = times_ms.iter().sum::<f64>() / times_ms.len() as f64;
            println!(
                "baseline summary runs={} avg_ms={:.2} min_ms={:.2} max_ms={:.2}",
                runs, avg_ms, min_ms, max_ms
            );
            Ok(())
        }
        PerfCommand::Profile {
            out,
            organism,
            illumination,
            plate_profile,
            duration,
            step,
            start_after,
            temperature_c,
            cfu,
            sample_hz,
            profile_out,
            profile_dirs,
        } => {
            let guard = ProfilerGuard::new(sample_hz.max(10))?;
            let start = Instant::now();
            run_generate(
                &out,
                OutputFormatArg::JpegSeq,
                12,
                Some(organism.clone()),
                Some(cfu),
                Some(illumination),
                Some(plate_profile),
                Some(BackgroundModeArg::Plate),
                duration,
                step,
                start_after,
                temperature_c,
                None,
                None,
                None,
                42,
                None,
                None,
                1.0,
                false,
                &profile_dirs,
            )
            .await?;
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

            std::fs::create_dir_all(&profile_out)?;
            let stamp = unix_timestamp_secs();
            let flame_path = profile_out.join(format!("synthris-{stamp}.svg"));
            let pb_path = profile_out.join(format!("synthris-{stamp}.pb"));

            let report = guard.report().build()?;
            let mut flame_file = std::fs::File::create(&flame_path)?;
            report.flamegraph(&mut flame_file)?;

            let profile = report.pprof()?;
            let mut pb_bytes = Vec::new();
            profile.encode(&mut pb_bytes)?;
            std::fs::write(&pb_path, pb_bytes)?;

            println!(
                "profile complete elapsed_ms={:.2} flamegraph={} pprof={}",
                elapsed_ms,
                flame_path.display(),
                pb_path.display()
            );
            Ok(())
        }
    }
}

fn load_profile_db(profile_dirs: &[PathBuf]) -> Result<ProfileDb> {
    let db = load_profile_db_any(profile_dirs)?;
    if db.organisms.is_empty() {
        bail!(
            "no organism profiles found; add profiles/organisms/*.(toml|json) or pass --profile-dir"
        );
    }
    if db.illuminations.is_empty() {
        bail!("no illumination profiles found; add profiles/illumination/*.(toml|json)");
    }

    Ok(db)
}

fn load_profile_db_any(profile_dirs: &[PathBuf]) -> Result<ProfileDb> {
    let mut cfg = ProfileDbConfig::default();
    if !profile_dirs.is_empty() {
        cfg.search_paths = profile_dirs.to_vec();
    }

    ProfileDb::load(&cfg)
}

fn parse_cfu_spec(input: &str) -> Result<CfuSpec> {
    if let Some((min_raw, max_raw)) = input.split_once('-') {
        let min = min_raw.trim().parse::<u32>()?;
        let max = max_raw.trim().parse::<u32>()?;
        if min == 0 && max == 0 {
            bail!("cfu range cannot be 0-0");
        }
        return Ok(CfuSpec::Range { min, max });
    }

    let v = input.trim().parse::<u32>()?;
    if v == 0 {
        bail!("cfu must be > 0");
    }
    Ok(CfuSpec::Exact(v))
}

fn parse_time_span(input: &str) -> Result<u64> {
    parse_time_span_impl(input, false)
}

fn parse_time_span_allow_zero(input: &str) -> Result<u64> {
    parse_time_span_impl(input, true)
}

fn parse_time_span_impl(input: &str, allow_zero: bool) -> Result<u64> {
    let raw = input.trim();
    if raw.is_empty() {
        bail!("empty time span");
    }

    let mut total = 0u64;
    let mut num_buf = String::new();

    for ch in raw.chars() {
        if ch.is_ascii_digit() {
            num_buf.push(ch);
            continue;
        }

        if num_buf.is_empty() {
            bail!("invalid time span segment in '{raw}'");
        }

        let n = num_buf.parse::<u64>()?;
        num_buf.clear();
        total = total.saturating_add(match ch {
            'd' | 'D' => n.saturating_mul(86_400),
            'h' | 'H' => n.saturating_mul(3_600),
            'm' | 'M' => n.saturating_mul(60),
            's' | 'S' => n,
            _ => bail!("unknown time unit '{ch}' in '{raw}'"),
        });
    }

    if !num_buf.is_empty() {
        let n = num_buf.parse::<u64>()?;
        total = total.saturating_add(n);
    }

    if total == 0 && !allow_zero {
        bail!("time span must be > 0");
    }

    Ok(total)
}

fn elapsed_filename(elapsed_seconds: u64) -> String {
    let h = elapsed_seconds / 3600;
    let m = (elapsed_seconds % 3600) / 60;
    format!("t_{h:03}h{m:02}m.jpg")
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("synthris_core=info,synthris_cli=info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn source_dims_for_request(
    mode: BackgroundMode,
    plate_baseline_id: Option<&str>,
) -> Option<(u32, u32)> {
    if mode != BackgroundMode::PlateImage {
        return None;
    }
    let asset = builtin_plate_baseline_by_id(plate_baseline_id?)?;
    Some((asset.source_size.width, asset.source_size.height))
}

fn resolve_output_size(
    width: Option<u32>,
    height: Option<u32>,
    source_dims: Option<(u32, u32)>,
    default_width: u32,
    default_height: u32,
) -> (u32, u32) {
    if let Some((src_w, src_h)) = source_dims {
        if src_w == 0 || src_h == 0 {
            return match (width, height) {
                (Some(w), Some(h)) => (w, h),
                (Some(v), None) | (None, Some(v)) => (v, v),
                (None, None) => (default_width, default_height),
            };
        }
        return match (width, height) {
            (None, None) => (src_w, src_h),
            (Some(w), None) => {
                let h = ((w as f32) * (src_h as f32 / src_w as f32))
                    .round()
                    .max(1.0) as u32;
                (w, h)
            }
            (None, Some(h)) => {
                let w = ((h as f32) * (src_w as f32 / src_h as f32))
                    .round()
                    .max(1.0) as u32;
                (w, h)
            }
            (Some(w), Some(h)) => (w, h),
        };
    }

    match (width, height) {
        (Some(w), Some(h)) => (w, h),
        (Some(v), None) | (None, Some(v)) => (v, v),
        (None, None) => (default_width, default_height),
    }
}


#[cfg(test)]
mod tests {
    use super::{parse_cfu_spec, parse_time_span, parse_time_span_allow_zero, resolve_output_size};
    use synthris_core::CfuSpec;

    #[test]
    fn parse_cfu_supports_exact_and_range() {
        let exact = parse_cfu_spec("50").expect("exact should parse");
        assert_eq!(exact, CfuSpec::Exact(50));

        let range = parse_cfu_spec("40-60").expect("range should parse");
        assert_eq!(range, CfuSpec::Range { min: 40, max: 60 });
    }

    #[test]
    fn parse_time_span_supports_compound_units() {
        assert_eq!(parse_time_span("2d").expect("2d"), 172_800);
        assert_eq!(parse_time_span("12h30m").expect("12h30m"), 45_000);
        assert_eq!(parse_time_span("90m").expect("90m"), 5_400);
    }

    #[test]
    fn parse_time_span_allow_zero_for_start_after() {
        assert_eq!(parse_time_span_allow_zero("0s").expect("0s"), 0);
        assert!(parse_time_span("0s").is_err());
    }

    #[test]
    fn output_size_uses_source_dimensions_by_default() {
        let (w, h) = resolve_output_size(None, None, Some((4000, 3000)), 1024, 1024);
        assert_eq!((w, h), (4000, 3000));
    }

    #[test]
    fn output_size_derives_missing_dimension_from_source_aspect() {
        let (w, h) = resolve_output_size(Some(1000), None, Some((4000, 3000)), 1024, 1024);
        assert_eq!((w, h), (1000, 750));
    }

    #[test]
    fn output_size_fallbacks_to_square_when_no_source_and_one_dimension_given() {
        let (w, h) = resolve_output_size(Some(1200), None, None, 1024, 1024);
        assert_eq!((w, h), (1200, 1200));
    }
}
