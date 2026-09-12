use crate::model::{ApiContest, ApiProblem, ContestProblem, ProblemModel, Submission};
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime},
};

const RESOURCES_BASE: &str = "https://kenkoooo.com/atcoder/resources";
const API_BASE: &str = "https://kenkoooo.com/atcoder/atcoder-api/v3";
const KENKOOOO_INTERVAL: Duration = Duration::from_millis(1100);
const ATCODER_INTERVAL: Duration = Duration::from_millis(350);

pub struct ApiClient {
    client: Client,
    cache_dir: PathBuf,
    cache_ttl: Duration,
    last_kenkoooo_request: Mutex<Option<Instant>>,
}

pub struct ResourceData {
    pub problems: Vec<ApiProblem>,
    pub contests: Vec<ApiContest>,
    pub contest_problems: Vec<ContestProblem>,
    pub models: HashMap<String, ProblemModel>,
}

impl ApiClient {
    pub fn new(cache_dir: PathBuf, cache_ttl_hours: u64) -> Result<Self> {
        fs::create_dir_all(&cache_dir)?;
        let client = Client::builder()
            .user_agent("daily-atcoder/0.1 (+personal study tool)")
            .gzip(true)
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            cache_dir,
            cache_ttl: Duration::from_secs(cache_ttl_hours.saturating_mul(3600)),
            last_kenkoooo_request: Mutex::new(None),
        })
    }

    pub fn load_resources(&self) -> Result<ResourceData> {
        let problems = self.cached_json("problems.json", &format!("{RESOURCES_BASE}/problems.json"))?;
        let contests = self.cached_json("contests.json", &format!("{RESOURCES_BASE}/contests.json"))?;
        let contest_problems = self.cached_json("contest-problem.json", &format!("{RESOURCES_BASE}/contest-problem.json"))?;
        let models = self.cached_json("problem-models.json", &format!("{RESOURCES_BASE}/problem-models.json"))?;
        Ok(ResourceData { problems, contests, contest_problems, models })
    }

    pub fn refresh_submissions(&self, username: &str) -> Result<Vec<Submission>> {
        let safe_user: String = username.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        let path = self.cache_dir.join(format!("submissions-{safe_user}.json"));
        let mut all: Vec<Submission> = if path.exists() {
            let bytes = fs::read(&path)?;
            serde_json::from_slice(&bytes).context("failed to parse cached submissions")?
        } else {
            Vec::new()
        };

        let mut from_second = all.iter().map(|s| s.epoch_second).max().map(|x| x + 1).unwrap_or(0);
        loop {
            let url = format!("{API_BASE}/user/submissions?user={}&from_second={from_second}", urlencoding(username));
            self.wait_for_kenkoooo();
            let page: Vec<Submission> = self.client.get(&url).send()?.error_for_status()?.json()?;
            if page.is_empty() { break; }
            let next = page.iter().map(|s| s.epoch_second).max().unwrap_or(from_second) + 1;
            all.extend(page);
            all.sort_by_key(|s| (s.epoch_second, s.problem_id.clone()));
            all.dedup_by(|a, b| a.epoch_second == b.epoch_second && a.problem_id == b.problem_id && a.result == b.result);
            fs::write(&path, serde_json::to_vec_pretty(&all)?)?;
            from_second = next;
        }
        if !path.exists() {
            fs::write(&path, serde_json::to_vec_pretty(&all)?)?;
        }
        Ok(all)
    }

    pub fn fetch_statement(&self, contest_id: &str, problem_id: &str) -> Result<String> {
        let url = format!("https://atcoder.jp/contests/{contest_id}/tasks/{problem_id}?lang=ja");
        let text = self.client.get(&url).send()?.error_for_status()?.text()?;
        thread::sleep(ATCODER_INTERVAL);
        Ok(text)
    }

    fn cached_json<T: DeserializeOwned>(&self, filename: &str, url: &str) -> Result<T> {
        let path = self.cache_dir.join(filename);
        if self.is_fresh(&path) {
            return read_json(&path);
        }

        self.wait_for_kenkoooo();
        match self.client.get(url).send().and_then(|r| r.error_for_status()) {
            Ok(response) => {
                let bytes = response.bytes()?;
                fs::write(&path, &bytes)?;
                serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {url}"))
            }
            Err(err) if path.exists() => {
                eprintln!("warning: failed to refresh {filename}: {err}; using cached copy");
                read_json(&path)
            }
            Err(err) => Err(err).with_context(|| format!("failed to download {url}")),
        }
    }

    fn is_fresh(&self, path: &Path) -> bool {
        if self.cache_ttl.is_zero() || !path.exists() { return false; }
        let Ok(metadata) = fs::metadata(path) else { return false; };
        let Ok(modified) = metadata.modified() else { return false; };
        SystemTime::now().duration_since(modified).map(|age| age <= self.cache_ttl).unwrap_or(false)
    }

    fn wait_for_kenkoooo(&self) {
        let mut last = self.last_kenkoooo_request.lock().expect("rate limiter mutex poisoned");
        if let Some(previous) = *last {
            let elapsed = previous.elapsed();
            if elapsed < KENKOOOO_INTERVAL {
                thread::sleep(KENKOOOO_INTERVAL - elapsed);
            }
        }
        *last = Some(Instant::now());
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).with_context(|| format!("failed to parse {}", path.display()))
}

fn urlencoding(input: &str) -> String {
    url::form_urlencoded::byte_serialize(input.as_bytes()).collect()
}
