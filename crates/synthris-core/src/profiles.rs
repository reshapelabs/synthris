use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::request::IlluminationMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureCardinalProfile {
    pub t_min_c: f32,
    pub t_opt_c: f32,
    pub t_max_c: f32,
    pub alpha: f32,
    pub beta: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpticalMaterialProfile {
    pub kappa_ref: f32,
    pub thickness_exp: f32,
    pub translucency: f32,
    pub pigment_rgb: [u8; 3],
    pub pigment_strength: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhenotypeKind {
    SmoothRound,
    RoughIrregular,
    MucoidSpread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhenotypeProfile {
    pub id: PhenotypeKind,
    pub weight: f32,
    #[serde(default = "default_phenotype_edge_roughness")]
    pub edge_roughness: f32,
    #[serde(default = "default_phenotype_spread_bias")]
    pub spread_bias: f32,
    #[serde(default = "default_phenotype_core_density")]
    pub core_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GrowthModelSpec {
    GompertzRadiusV2 {
        mu_max_ref_h: f32,
        lag_ref_h: f32,
        n0_log10: f32,
        nmax_log10: f32,
        r0_px: f32,
        rmax_ref_px: f32,
        #[serde(default = "default_phase_early_scale")]
        phase_early_scale: f32,
        #[serde(default = "default_phase_mid_scale")]
        phase_mid_scale: f32,
        #[serde(default = "default_phase_late_scale")]
        phase_late_scale: f32,
        #[serde(default = "default_rmax_temp_floor")]
        rmax_temp_floor: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LognormalDelaySpec {
    pub mean_min: f32,
    pub sigma: f32,
    pub max_h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SeedingModelSpec {
    PoissonDiscDelayV1 {
        #[serde(default = "default_min_dist_factor")]
        min_dist_factor: f32,
        #[serde(default = "default_min_dist_floor_px")]
        min_dist_floor_px: f32,
        #[serde(default = "default_attempts_per_colony")]
        attempts_per_colony: u32,
        onset: LognormalDelaySpec,
        #[serde(default = "default_kappa_jitter_low")]
        kappa_jitter_low: f32,
        #[serde(default = "default_kappa_jitter_high")]
        kappa_jitter_high: f32,
        #[serde(default = "default_opacity_scale_translucent")]
        opacity_scale_translucent: f32,
        #[serde(default = "default_opacity_scale_standard")]
        opacity_scale_standard: f32,
        #[serde(default = "default_opacity_scale_dense")]
        opacity_scale_dense: f32,
        #[serde(default = "default_morphology_jitter")]
        morphology_jitter: f32,
        #[serde(default = "default_temp_opt_jitter_sigma_c")]
        temp_opt_jitter_sigma_c: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GeometryModelSpec {
    RadialDomeV2 {
        #[serde(default = "default_edge_hardness")]
        edge_hardness: f32,
        #[serde(default = "default_thickness_power")]
        thickness_power: f32,
    },
    AnisotropicBlobV1 {
        #[serde(default = "default_edge_hardness")]
        edge_hardness: f32,
        #[serde(default = "default_thickness_power")]
        thickness_power: f32,
        #[serde(default = "default_anisotropy")]
        anisotropy: f32,
        #[serde(default = "default_angular_wobble")]
        angular_wobble: f32,
        #[serde(default = "default_wobble_frequency")]
        wobble_frequency: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacklitOpticsParams {
    #[serde(default = "default_min_absorbance")]
    pub min_absorbance: f32,
    #[serde(default = "default_backlit_edge_base")]
    pub attenuation_edge_base: f32,
    #[serde(default = "default_backlit_edge_gain")]
    pub attenuation_edge_gain: f32,
    #[serde(default = "default_tint_strength")]
    pub tint_strength: f32,
    #[serde(default = "default_translucency_min")]
    pub translucency_min: f32,
    #[serde(default = "default_translucency_max")]
    pub translucency_max: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontlitOpticsParams {
    #[serde(default = "default_min_contrast")]
    pub min_contrast: f32,
    #[serde(default = "default_frontlit_target_edge_base")]
    pub target_edge_base: f32,
    #[serde(default = "default_frontlit_target_edge_gain")]
    pub target_edge_gain: f32,
    #[serde(default = "default_frontlit_blend_alpha")]
    pub blend_alpha: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookScaleParams {
    #[serde(default = "default_look_clean")]
    pub clean: f32,
    #[serde(default = "default_look_realistic")]
    pub realistic: f32,
    #[serde(default = "default_look_gritty")]
    pub gritty: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpacityScaleParams {
    #[serde(default = "default_opacity_scale_translucent")]
    pub translucent: f32,
    #[serde(default = "default_opacity_scale_standard")]
    pub standard: f32,
    #[serde(default = "default_opacity_scale_dense")]
    pub dense: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpticsModelSpec {
    AttenuationBlendV2 {
        backlit: BacklitOpticsParams,
        frontlit: FrontlitOpticsParams,
        look: LookScaleParams,
        opacity: OpacityScaleParams,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganismProfile {
    pub id: String,
    pub temperature_cardinal: TemperatureCardinalProfile,
    pub optical_material: OpticalMaterialProfile,
    pub growth_model: GrowthModelSpec,
    pub seeding_model: SeedingModelSpec,
    pub geometry_model: GeometryModelSpec,
    #[serde(default = "default_phenotypes")]
    pub phenotypes: Vec<PhenotypeProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IlluminationProfile {
    pub id: String,
    pub mode: IlluminationMode,
    pub background_rgb: [u8; 3],
    pub colony_rgb: [u8; 3],
    #[serde(default = "default_backlit_absorbance")]
    pub backlit_absorbance: f32,
    #[serde(default = "default_frontlit_contrast")]
    pub frontlit_contrast: f32,
    pub optics_model: OpticsModelSpec,
}

#[derive(Debug, Clone)]
pub struct ProfileDbConfig {
    pub include_builtin: bool,
    pub search_paths: Vec<PathBuf>,
}

impl Default for ProfileDbConfig {
    fn default() -> Self {
        Self {
            include_builtin: true,
            search_paths: vec![PathBuf::from("profiles")],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProfileDb {
    pub organisms: HashMap<String, OrganismProfile>,
    pub illuminations: HashMap<String, IlluminationProfile>,
}

impl ProfileDb {
    pub fn load(config: &ProfileDbConfig) -> Result<Self> {
        let mut db = if config.include_builtin {
            Self::builtin()?
        } else {
            Self::default()
        };

        for root in &config.search_paths {
            if !root.exists() {
                continue;
            }

            load_directory_profiles(root.join("organisms"), |p: OrganismProfile| {
                db.organisms.insert(p.id.clone(), p)
            })?;
            load_directory_profiles(root.join("illumination"), |p: IlluminationProfile| {
                db.illuminations.insert(p.id.clone(), p)
            })?;
        }

        for o in db.organisms.values() {
            validate_organism(o)?;
        }
        for i in db.illuminations.values() {
            validate_illumination(i)?;
        }
        Ok(db)
    }

    pub fn builtin() -> Result<Self> {
        let mut db = Self::default();

        const ORGANISMS: &[&str] = &[
            include_str!("../../../profiles/organisms/morrow.toml"),
            include_str!("../../../profiles/organisms/quill.toml"),
            include_str!("../../../profiles/organisms/solen.toml"),
            include_str!("../../../profiles/organisms/zenth.toml"),
        ];
        const ILLUMINATIONS: &[&str] = &[
            include_str!("../../../profiles/illumination/frontlit.toml"),
            include_str!("../../../profiles/illumination/backlit.toml"),
        ];

        for raw in ORGANISMS {
            let p: OrganismProfile =
                toml::from_str(raw).context("invalid built-in organism profile")?;
            db.organisms.insert(p.id.clone(), p);
        }
        for raw in ILLUMINATIONS {
            let p: IlluminationProfile =
                toml::from_str(raw).context("invalid built-in illumination profile")?;
            db.illuminations.insert(p.id.clone(), p);
        }

        for o in db.organisms.values() {
            validate_organism(o)?;
        }
        for i in db.illuminations.values() {
            validate_illumination(i)?;
        }

        Ok(db)
    }

    pub fn organism(&self, id: &str) -> Option<&OrganismProfile> {
        self.organisms.get(id)
    }

    pub fn illumination(&self, id: &str) -> Option<&IlluminationProfile> {
        self.illuminations.get(id)
    }

}

fn validate_organism(organism: &OrganismProfile) -> Result<()> {
    if organism.id.trim().is_empty() {
        bail!("organism profile id must not be empty");
    }
    if organism.optical_material.kappa_ref <= 0.0 {
        bail!(
            "organism '{}' has non-positive optical_material.kappa_ref",
            organism.id
        );
    }
    if organism.optical_material.thickness_exp <= 0.0 {
        bail!(
            "organism '{}' has non-positive optical_material.thickness_exp",
            organism.id
        );
    }
    if !(0.0..=1.0).contains(&organism.optical_material.pigment_strength) {
        bail!(
            "organism '{}' has optical_material.pigment_strength outside 0..=1",
            organism.id
        );
    }
    if organism.phenotypes.is_empty() {
        bail!(
            "organism '{}' must define at least one phenotype",
            organism.id
        );
    }
    for p in &organism.phenotypes {
        if p.weight <= 0.0 {
            bail!(
                "organism '{}' has non-positive phenotype weight",
                organism.id
            );
        }
    }

    match &organism.growth_model {
        GrowthModelSpec::GompertzRadiusV2 {
            mu_max_ref_h,
            lag_ref_h,
            nmax_log10,
            r0_px,
            rmax_ref_px,
            ..
        } => {
            if *mu_max_ref_h <= 0.0 {
                bail!(
                    "organism '{}' growth_model.mu_max_ref_h must be > 0",
                    organism.id
                );
            }
            if *lag_ref_h < 0.0 {
                bail!(
                    "organism '{}' growth_model.lag_ref_h must be >= 0",
                    organism.id
                );
            }
            if *nmax_log10 <= 0.0 {
                bail!(
                    "organism '{}' growth_model.nmax_log10 must be > 0",
                    organism.id
                );
            }
            if *r0_px <= 0.0 || *rmax_ref_px <= *r0_px {
                bail!(
                    "organism '{}' growth_model requires rmax_ref_px > r0_px > 0",
                    organism.id
                );
            }
        }
    }

    match &organism.seeding_model {
        SeedingModelSpec::PoissonDiscDelayV1 {
            min_dist_factor,
            min_dist_floor_px,
            attempts_per_colony,
            onset,
            kappa_jitter_low,
            kappa_jitter_high,
            temp_opt_jitter_sigma_c,
            ..
        } => {
            if *min_dist_factor <= 0.0 || *min_dist_floor_px < 0.0 {
                bail!(
                    "organism '{}' has invalid seeding min distance params",
                    organism.id
                );
            }
            if *attempts_per_colony == 0 {
                bail!("organism '{}' attempts_per_colony must be > 0", organism.id);
            }
            if onset.mean_min <= 0.0 || onset.sigma <= 0.0 || onset.max_h < 0.0 {
                bail!(
                    "organism '{}' has invalid seeding onset params",
                    organism.id
                );
            }
            if *kappa_jitter_high < *kappa_jitter_low {
                bail!(
                    "organism '{}' has kappa_jitter_high < kappa_jitter_low",
                    organism.id
                );
            }
            if *temp_opt_jitter_sigma_c < 0.0 {
                bail!(
                    "organism '{}' temp_opt_jitter_sigma_c must be >= 0",
                    organism.id
                );
            }
        }
    }

    Ok(())
}

fn validate_illumination(illumination: &IlluminationProfile) -> Result<()> {
    if illumination.id.trim().is_empty() {
        bail!("illumination profile id must not be empty");
    }
    if illumination.backlit_absorbance <= 0.0 {
        bail!(
            "illumination '{}' backlit_absorbance must be > 0",
            illumination.id
        );
    }
    if illumination.frontlit_contrast <= 0.0 {
        bail!(
            "illumination '{}' frontlit_contrast must be > 0",
            illumination.id
        );
    }
    Ok(())
}

fn load_directory_profiles<T, F, V>(dir: PathBuf, mut insert: F) -> Result<()>
where
    T: for<'de> Deserialize<'de>,
    F: FnMut(T) -> V,
{
    if !dir.exists() {
        return Ok(());
    }

    for path in collect_profile_files(&dir)? {
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed reading {}", path.display()))?;
        let obj: T = parse_profile_str(&path, &raw)?;
        insert(obj);
    }

    Ok(())
}

fn collect_profile_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                stack.push(path);
                continue;
            }

            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("toml")
                })
            {
                out.push(path);
            }
        }
    }

    Ok(out)
}

fn parse_profile_str<T>(path: &Path, raw: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "json" => {
            serde_json::from_str(raw).with_context(|| format!("invalid json {}", path.display()))
        }
        "toml" => toml::from_str(raw).with_context(|| format!("invalid toml {}", path.display())),
        _ => bail!("unsupported profile extension for {}", path.display()),
    }
}

fn default_phase_early_scale() -> f32 {
    0.8
}
fn default_phase_mid_scale() -> f32 {
    1.0
}
fn default_phase_late_scale() -> f32 {
    1.25
}
fn default_rmax_temp_floor() -> f32 {
    0.6
}
fn default_min_dist_factor() -> f32 {
    0.9
}
fn default_min_dist_floor_px() -> f32 {
    8.0
}
fn default_attempts_per_colony() -> u32 {
    600
}
fn default_kappa_jitter_low() -> f32 {
    0.9
}
fn default_kappa_jitter_high() -> f32 {
    1.1
}
fn default_morphology_jitter() -> f32 {
    0.25
}
fn default_temp_opt_jitter_sigma_c() -> f32 {
    0.0
}
fn default_opacity_scale_translucent() -> f32 {
    0.8
}
fn default_opacity_scale_standard() -> f32 {
    1.0
}
fn default_opacity_scale_dense() -> f32 {
    1.3
}
fn default_edge_hardness() -> f32 {
    1.0
}
fn default_thickness_power() -> f32 {
    1.0
}
fn default_anisotropy() -> f32 {
    0.2
}
fn default_angular_wobble() -> f32 {
    0.08
}
fn default_wobble_frequency() -> u32 {
    6
}
fn default_min_absorbance() -> f32 {
    0.2
}
fn default_backlit_edge_base() -> f32 {
    0.5
}
fn default_backlit_edge_gain() -> f32 {
    0.5
}
fn default_tint_strength() -> f32 {
    0.25
}
fn default_translucency_min() -> f32 {
    0.2
}
fn default_translucency_max() -> f32 {
    1.2
}
fn default_min_contrast() -> f32 {
    0.2
}
fn default_frontlit_target_edge_base() -> f32 {
    0.7
}
fn default_frontlit_target_edge_gain() -> f32 {
    0.3
}
fn default_frontlit_blend_alpha() -> f32 {
    0.68
}
fn default_look_clean() -> f32 {
    0.9
}
fn default_look_realistic() -> f32 {
    1.0
}
fn default_look_gritty() -> f32 {
    1.15
}
fn default_backlit_absorbance() -> f32 {
    1.0
}
fn default_frontlit_contrast() -> f32 {
    1.0
}
fn default_phenotype_edge_roughness() -> f32 {
    0.2
}
fn default_phenotype_spread_bias() -> f32 {
    1.0
}
fn default_phenotype_core_density() -> f32 {
    1.0
}
fn default_phenotypes() -> Vec<PhenotypeProfile> {
    vec![
        PhenotypeProfile {
            id: PhenotypeKind::SmoothRound,
            weight: 0.45,
            edge_roughness: 0.15,
            spread_bias: 1.0,
            core_density: 1.0,
        },
        PhenotypeProfile {
            id: PhenotypeKind::RoughIrregular,
            weight: 0.35,
            edge_roughness: 0.45,
            spread_bias: 0.95,
            core_density: 1.05,
        },
        PhenotypeProfile {
            id: PhenotypeKind::MucoidSpread,
            weight: 0.20,
            edge_roughness: 0.25,
            spread_bias: 1.12,
            core_density: 0.9,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        BacklitOpticsParams, FrontlitOpticsParams, GeometryModelSpec, GrowthModelSpec,
        IlluminationProfile, LognormalDelaySpec, LookScaleParams, OpacityScaleParams,
        OpticalMaterialProfile, OpticsModelSpec, OrganismProfile, PhenotypeKind, PhenotypeProfile,
        ProfileDb, SeedingModelSpec, TemperatureCardinalProfile,
    };
    use crate::request::IlluminationMode;

    fn test_optics_model() -> OpticsModelSpec {
        OpticsModelSpec::AttenuationBlendV2 {
            backlit: BacklitOpticsParams {
                min_absorbance: 0.2,
                attenuation_edge_base: 0.5,
                attenuation_edge_gain: 0.5,
                tint_strength: 0.25,
                translucency_min: 0.2,
                translucency_max: 1.2,
            },
            frontlit: FrontlitOpticsParams {
                min_contrast: 0.2,
                target_edge_base: 0.7,
                target_edge_gain: 0.3,
                blend_alpha: 0.68,
            },
            look: LookScaleParams {
                clean: 0.9,
                realistic: 1.0,
                gritty: 1.15,
            },
            opacity: OpacityScaleParams {
                translucent: 0.8,
                standard: 1.0,
                dense: 1.3,
            },
        }
    }

    fn minimal_db() -> ProfileDb {
        let mut db = ProfileDb::default();
        db.organisms.insert(
            "morrow".into(),
            OrganismProfile {
                id: "morrow".into(),
                temperature_cardinal: TemperatureCardinalProfile {
                    t_min_c: 2.0,
                    t_opt_c: 30.0,
                    t_max_c: 45.0,
                    alpha: 1.2,
                    beta: 1.5,
                },
                optical_material: OpticalMaterialProfile {
                    kappa_ref: 1.2,
                    thickness_exp: 1.4,
                    translucency: 0.9,
                    pigment_rgb: [212, 190, 145],
                    pigment_strength: 0.35,
                },
                growth_model: GrowthModelSpec::GompertzRadiusV2 {
                    mu_max_ref_h: 0.8,
                    lag_ref_h: 2.0,
                    n0_log10: 1.0,
                    nmax_log10: 8.0,
                    r0_px: 2.0,
                    rmax_ref_px: 30.0,
                    phase_early_scale: 0.8,
                    phase_mid_scale: 1.0,
                    phase_late_scale: 1.25,
                    rmax_temp_floor: 0.6,
                },
                seeding_model: SeedingModelSpec::PoissonDiscDelayV1 {
                    min_dist_factor: 0.9,
                    min_dist_floor_px: 8.0,
                    attempts_per_colony: 300,
                    onset: LognormalDelaySpec {
                        mean_min: 20.0,
                        sigma: 0.5,
                        max_h: 2.0,
                    },
                    kappa_jitter_low: 0.9,
                    kappa_jitter_high: 1.1,
                    opacity_scale_translucent: 0.8,
                    opacity_scale_standard: 1.0,
                    opacity_scale_dense: 1.3,
                    morphology_jitter: 0.2,
                    temp_opt_jitter_sigma_c: 0.0,
                },
                geometry_model: GeometryModelSpec::RadialDomeV2 {
                    edge_hardness: 1.0,
                    thickness_power: 1.0,
                },
                phenotypes: vec![PhenotypeProfile {
                    id: PhenotypeKind::SmoothRound,
                    weight: 1.0,
                    edge_roughness: 0.1,
                    spread_bias: 1.0,
                    core_density: 1.0,
                }],
            },
        );
        db.illuminations.insert(
            "backlit".into(),
            IlluminationProfile {
                id: "backlit".into(),
                mode: IlluminationMode::Backlit,
                background_rgb: [170, 165, 160],
                colony_rgb: [120, 110, 105],
                backlit_absorbance: 1.8,
                frontlit_contrast: 1.0,
                optics_model: test_optics_model(),
            },
        );
        db
    }

    #[test]
    fn minimal_db_contains_expected_profiles() {
        let db = minimal_db();
        assert!(db.organism("morrow").is_some());
        assert!(db.illumination("backlit").is_some());
    }

    #[test]
    fn builtin_profiles_load() {
        let db = ProfileDb::builtin().expect("builtin profiles");
        assert!(db.organism("morrow").is_some());
        assert!(db.illumination("backlit").is_some());
    }
}
