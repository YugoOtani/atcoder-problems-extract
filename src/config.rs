use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub username: String,
    pub problem_count: usize,
    #[serde(default = "default_cache_ttl_hours")]
    pub cache_ttl_hours: u64,
    pub selection: SelectionConfig,
    pub contest: ContestConfig,
    pub difficulty: DifficultyConfig,
    pub age: AgeConfig,
}

fn default_cache_ttl_hours() -> u64 { 24 }

#[derive(Debug, Clone, Deserialize)]
pub struct SelectionConfig {
    pub exclude_accepted: bool,
    pub exclude_previously_selected: bool,
    pub shuffle_final_order: bool,
    #[serde(default = "default_max_generation_attempts")]
    pub max_generation_attempts: usize,
}

fn default_max_generation_attempts() -> usize { 5000 }

#[derive(Debug, Clone, Deserialize)]
pub struct ContestConfig {
    pub types: Vec<ContestTypeConfig>,
    pub max_same_contest: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContestTypeConfig {
    pub name: String,
    pub weight: f64,
    #[serde(default)]
    pub min_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DifficultyConfig {
    pub bands: Vec<DifficultyBand>,
    #[serde(default)]
    pub constraints: Vec<DifficultyConstraint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DifficultyBand {
    pub name: String,
    pub min: i64,
    pub max: i64,
    pub weight: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DifficultyConstraint {
    pub min: Option<i64>,
    pub max: Option<i64>,
    pub min_count: Option<usize>,
    pub max_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgeConfig {
    pub bands: Vec<AgeBand>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgeBand {
    pub max_years: Option<f64>,
    pub weight: f64,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.username.trim().is_empty() { bail!("username must not be empty"); }
        if self.problem_count == 0 { bail!("problem_count must be > 0"); }
        if self.selection.max_generation_attempts == 0 { bail!("max_generation_attempts must be > 0"); }
        if self.contest.max_same_contest == 0 { bail!("contest.max_same_contest must be > 0"); }
        if self.contest.types.is_empty() { bail!("contest.types must not be empty"); }

        let mut min_type_sum = 0usize;
        for t in &self.contest.types {
            if t.name.trim().is_empty() { bail!("contest type name must not be empty"); }
            if t.weight <= 0.0 { bail!("contest type weight must be > 0: {}", t.name); }
            min_type_sum += t.min_count;
        }
        if min_type_sum > self.problem_count {
            bail!("sum of contest type min_count ({min_type_sum}) exceeds problem_count ({})", self.problem_count);
        }

        if self.difficulty.bands.is_empty() { bail!("difficulty.bands must not be empty"); }
        for b in &self.difficulty.bands {
            if b.min > b.max { bail!("difficulty band {} has min > max", b.name); }
            if b.weight <= 0.0 { bail!("difficulty band {} has non-positive weight", b.name); }
        }
        let mut sorted = self.difficulty.bands.clone();
        sorted.sort_by_key(|b| b.min);
        for pair in sorted.windows(2) {
            if pair[0].max >= pair[1].min {
                bail!("difficulty bands overlap: {} and {}", pair[0].name, pair[1].name);
            }
        }

        for c in &self.difficulty.constraints {
            if let (Some(min), Some(max)) = (c.min, c.max) {
                if min > max { bail!("difficulty constraint has min > max"); }
            }
            if let Some(n) = c.min_count {
                if n > self.problem_count { bail!("difficulty min_count exceeds problem_count"); }
            }
            if let Some(n) = c.max_count {
                if n > self.problem_count { bail!("difficulty max_count exceeds problem_count"); }
            }
            if let (Some(min_count), Some(max_count)) = (c.min_count, c.max_count) {
                if min_count > max_count { bail!("difficulty constraint min_count > max_count"); }
            }
        }

        if self.age.bands.is_empty() { bail!("age.bands must not be empty"); }
        let mut seen_fallback = false;
        let mut last_max = -1.0_f64;
        for (i, b) in self.age.bands.iter().enumerate() {
            if b.weight <= 0.0 { bail!("age band weight must be > 0"); }
            match b.max_years {
                Some(max) => {
                    if seen_fallback { bail!("age band with max_years cannot follow fallback band"); }
                    if max < 0.0 || max <= last_max { bail!("age max_years must be strictly increasing"); }
                    last_max = max;
                }
                None => {
                    if i + 1 != self.age.bands.len() { bail!("age fallback band must be last"); }
                    seen_fallback = true;
                }
            }
        }
        if !seen_fallback { bail!("age.bands must end with a fallback band without max_years"); }
        Ok(())
    }
}
