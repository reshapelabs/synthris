use anyhow::{Result, bail};
use regex::Regex;
use synthris_core::CfuSpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobNameOverrides {
    pub cfu: Option<CfuSpec>,
    pub seed: Option<u64>,
    pub organism_id: Option<String>,
    pub respect_capture_interval: Option<bool>,
}

impl JobNameOverrides {
    pub fn empty() -> Self {
        Self {
            cfu: None,
            seed: None,
            organism_id: None,
            respect_capture_interval: None,
        }
    }
}

pub fn parse_job_name_overrides(job_name: &str) -> Result<JobNameOverrides> {
    let mut out = JobNameOverrides::empty();

    let marker_re = Regex::new(r"(?i)\b(cfu|col|seed|organism|org|pace)\s*=")?;
    let pair_re =
        Regex::new(r"(?i)\b(?P<key>cfu|col|seed|organism|org|pace)\s*=\s*(?P<value>[^\s]+)")?;

    let marker_count = marker_re.find_iter(job_name).count();
    let pairs: Vec<_> = pair_re.captures_iter(job_name).collect();

    if marker_count != pairs.len() {
        bail!("malformed job parameter syntax for supported keys");
    }

    for cap in pairs {
        let key = cap["key"].to_ascii_lowercase();
        let value = &cap["value"];
        match key.as_str() {
            "col" => {
                out.cfu = Some(parse_cfu(value)?);
            }
            "cfu" => {
                out.cfu = Some(parse_cfu(value)?);
            }
            "seed" => {
                out.seed = Some(value.parse::<u64>().map_err(|_| {
                    anyhow::anyhow!("invalid seed value '{value}', expected unsigned integer")
                })?);
            }
            "organism" | "org" => {
                out.organism_id = Some(parse_organism_id(value)?);
            }
            "pace" => {
                out.respect_capture_interval = Some(parse_bool_switch(value, "pace")?);
            }
            _ => unreachable!("regex limits keys"),
        }
    }

    Ok(out)
}

fn parse_bool_switch(raw: &str, field: &str) -> Result<bool> {
    let v = raw.trim().to_ascii_lowercase();
    match v.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{field} must be one of 1/0/true/false/yes/no/on/off"),
    }
}

fn parse_organism_id(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("organism must not be empty");
    }
    let valid = raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.');
    if !valid {
        bail!("organism contains invalid characters");
    }
    Ok(raw.to_string())
}

fn parse_cfu(raw: &str) -> Result<CfuSpec> {
    if let Some((min_raw, max_raw)) = raw.split_once('-') {
        let min = parse_positive_u32(min_raw, "col range min")?;
        let max = parse_positive_u32(max_raw, "col range max")?;
        if min == 0 && max == 0 {
            bail!("col range cannot be 0-0");
        }
        if min > max {
            bail!("col range min cannot be greater than max");
        }
        return Ok(CfuSpec::Range { min, max });
    }

    let v = parse_positive_u32(raw, "col")?;
    if v == 0 {
        bail!("col must be > 0");
    }
    Ok(CfuSpec::Exact(v))
}

fn parse_positive_u32(raw: &str, field: &str) -> Result<u32> {
    raw.trim()
        .parse::<u32>()
        .map_err(|_| anyhow::anyhow!("invalid {field} value '{raw}'"))
}

#[cfg(test)]
mod tests {
    use super::parse_job_name_overrides;
    use synthris_core::CfuSpec;

    #[test]
    fn parses_exact_colony_count() {
        let parsed = parse_job_name_overrides("job cfu=75").expect("parse");
        assert_eq!(parsed.cfu, Some(CfuSpec::Exact(75)));
    }

    #[test]
    fn parses_range_colony_count() {
        let parsed = parse_job_name_overrides("job cfu=50-100").expect("parse");
        assert_eq!(parsed.cfu, Some(CfuSpec::Range { min: 50, max: 100 }));
    }

    #[test]
    fn parses_seed_and_organism() {
        let parsed =
            parse_job_name_overrides("job seed=123 organism=morrow pace=1").expect("parse");
        assert_eq!(parsed.seed, Some(123));
        assert_eq!(parsed.organism_id.as_deref(), Some("morrow"));
        assert_eq!(parsed.respect_capture_interval, Some(true));
    }

    #[test]
    fn is_case_insensitive() {
        let parsed =
            parse_job_name_overrides("job CFU=25 SeEd=44 ORG=ecoli PACE=off").expect("parse");
        assert_eq!(parsed.cfu, Some(CfuSpec::Exact(25)));
        assert_eq!(parsed.seed, Some(44));
        assert_eq!(parsed.organism_id.as_deref(), Some("ecoli"));
        assert_eq!(parsed.respect_capture_interval, Some(false));
    }

    #[test]
    fn duplicate_keys_last_wins() {
        let parsed = parse_job_name_overrides("job cfu=10-20 col=30 organism=ecoli org=morrow")
            .expect("parse");
        assert_eq!(parsed.cfu, Some(CfuSpec::Exact(30)));
        assert_eq!(parsed.organism_id.as_deref(), Some("morrow"));
    }

    #[test]
    fn malformed_value_fails() {
        assert!(parse_job_name_overrides("job cfu=abc").is_err());
        assert!(parse_job_name_overrides("job seed=nope").is_err());
        assert!(parse_job_name_overrides("job organism=morrow!").is_err());
        assert!(parse_job_name_overrides("job pace=maybe").is_err());
    }

    #[test]
    fn malformed_syntax_fails() {
        assert!(parse_job_name_overrides("job cfu=").is_err());
        assert!(parse_job_name_overrides("job seed= ").is_err());
    }

    #[test]
    fn legacy_id_token_is_ignored() {
        let parsed = parse_job_name_overrides("job id=1 cfu=55").expect("parse");
        assert_eq!(parsed.cfu, Some(CfuSpec::Exact(55)));
    }
}
