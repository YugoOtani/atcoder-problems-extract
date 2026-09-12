use crate::{
    api::ResourceData,
    config::{AgeConfig, Config, DifficultyBand},
    model::{Candidate, ContestFile, Submission},
};
use anyhow::{bail, Result};
use chrono::Utc;
use rand::{distributions::WeightedIndex, prelude::Distribution, seq::SliceRandom, Rng};
use std::collections::{HashMap, HashSet};

pub fn build_candidates(
    config: &Config,
    resources: &ResourceData,
    submissions: &[Submission],
    prior_contests: &[ContestFile],
) -> Vec<Candidate> {
    let solved: HashSet<&str> = submissions.iter()
        .filter(|s| s.result == "AC")
        .map(|s| s.problem_id.as_str())
        .collect();
    let previously_selected: HashSet<&str> = prior_contests.iter()
        .flat_map(|c| c.problems.iter().map(|p| p.problem_id.as_str()))
        .collect();

    let contest_map: HashMap<&str, _> = resources.contests.iter().map(|c| (c.id.as_str(), c)).collect();
    let index_map: HashMap<(&str, &str), &str> = resources.contest_problems.iter()
        .map(|x| ((x.contest_id.as_str(), x.problem_id.as_str()), x.problem_index.as_str()))
        .collect();

    let allowed_types: HashSet<String> = config.contest.types.iter().map(|t| t.name.to_ascii_uppercase()).collect();

    resources.problems.iter().filter_map(|p| {
        if config.selection.exclude_accepted && solved.contains(p.id.as_str()) { return None; }
        if config.selection.exclude_previously_selected && previously_selected.contains(p.id.as_str()) { return None; }

        let contest_type = classify_contest(&p.contest_id)?;
        if !allowed_types.contains(&contest_type) { return None; }

        let model = resources.models.get(&p.id);
        let raw = model.and_then(|m| m.difficulty)?;
        let difficulty = clip_difficulty(raw);
        if !config.difficulty.bands.iter().any(|b| in_band(difficulty, b)) { return None; }

        let contest = contest_map.get(p.contest_id.as_str())?;
        if contest.start_epoch_second > Utc::now().timestamp() { return None; }
        let index = index_map.get(&(p.contest_id.as_str(), p.id.as_str()))?.to_string();
        Some(Candidate {
            id: p.id.clone(),
            contest_id: p.contest_id.clone(),
            problem_index: index,
            title: p.title.clone(),
            difficulty,
            raw_difficulty: raw,
            contest_start_epoch_second: contest.start_epoch_second,
            contest_type,
        })
    }).collect()
}

pub fn select_problems(config: &Config, candidates: &[Candidate]) -> Result<Vec<Candidate>> {
    if candidates.len() < config.problem_count {
        bail!("only {} eligible problems remain, but problem_count is {}", candidates.len(), config.problem_count);
    }

    let mut rng = rand::thread_rng();
    for _ in 0..config.selection.max_generation_attempts {
        let mut selected: Vec<Candidate> = Vec::with_capacity(config.problem_count);
        let mut failed = false;

        for _slot in 0..config.problem_count {
            let mut picked = None;
            for _ in 0..200 {
                let band = weighted_choice(&config.difficulty.bands, |x| x.weight, &mut rng)?;
                let contest_type = weighted_choice(&config.contest.types, |x| x.weight, &mut rng)?;

                let pool: Vec<&Candidate> = candidates.iter().filter(|p| {
                    in_band(p.difficulty, band)
                        && p.contest_type.eq_ignore_ascii_case(&contest_type.name)
                        && !selected.iter().any(|s| s.id == p.id)
                        && selected.iter().filter(|s| s.contest_id == p.contest_id).count() < config.contest.max_same_contest
                }).collect();
                if pool.is_empty() { continue; }

                let weights: Vec<f64> = pool.iter().map(|p| age_weight(&config.age, p.contest_start_epoch_second)).collect();
                let dist = WeightedIndex::new(&weights)?;
                picked = Some((*pool[dist.sample(&mut rng)]).clone());
                break;
            }
            if let Some(p) = picked { selected.push(p); } else { failed = true; break; }
        }

        if failed { continue; }
        if !satisfies_constraints(config, &selected) { continue; }
        if config.selection.shuffle_final_order { selected.shuffle(&mut rng); }
        return Ok(selected);
    }

    bail!("could not generate a contest satisfying the current configuration after {} attempts; relax constraints or broaden candidate ranges", config.selection.max_generation_attempts)
}

fn satisfies_constraints(config: &Config, selected: &[Candidate]) -> bool {
    for t in &config.contest.types {
        let count = selected.iter().filter(|p| p.contest_type.eq_ignore_ascii_case(&t.name)).count();
        if count < t.min_count { return false; }
    }
    for c in &config.difficulty.constraints {
        let count = selected.iter().filter(|p| {
            c.min.map(|x| p.difficulty >= x).unwrap_or(true)
                && c.max.map(|x| p.difficulty <= x).unwrap_or(true)
        }).count();
        if c.min_count.map(|x| count < x).unwrap_or(false) { return false; }
        if c.max_count.map(|x| count > x).unwrap_or(false) { return false; }
    }
    true
}

fn weighted_choice<'a, T, F, R>(items: &'a [T], weight: F, rng: &mut R) -> Result<&'a T>
where
    F: Fn(&T) -> f64,
    R: Rng + ?Sized,
{
    let weights: Vec<f64> = items.iter().map(weight).collect();
    let dist = WeightedIndex::new(&weights)?;
    Ok(&items[dist.sample(rng)])
}

fn age_weight(config: &AgeConfig, contest_start: i64) -> f64 {
    let age_seconds = (Utc::now().timestamp() - contest_start).max(0) as f64;
    let years = age_seconds / (365.2425 * 24.0 * 60.0 * 60.0);
    config.age_weight(years)
}

impl AgeConfig {
    fn age_weight(&self, years: f64) -> f64 {
        for band in &self.bands {
            if band.max_years.map(|m| years <= m).unwrap_or(true) { return band.weight; }
        }
        1.0
    }
}

fn classify_contest(contest_id: &str) -> Option<String> {
    let id = contest_id.to_ascii_lowercase();
    if id.starts_with("abc") { Some("ABC".into()) }
    else if id.starts_with("arc") { Some("ARC".into()) }
    else { None }
}

fn in_band(difficulty: i64, band: &DifficultyBand) -> bool {
    band.min <= difficulty && difficulty <= band.max
}

pub fn clip_difficulty(raw: f64) -> i64 {
    let clipped = if raw >= 400.0 { raw } else { 400.0 / (1.0_f64 - raw / 400.0).exp() };
    clipped.round() as i64
}

#[cfg(test)]
mod tests {
    use super::clip_difficulty;

    #[test]
    fn difficulty_clip_matches_atcoder_problems_rule() {
        assert_eq!(clip_difficulty(400.0), 400);
        assert_eq!(clip_difficulty(0.0), 147);
        assert!(clip_difficulty(200.0) > 200);
    }
}
