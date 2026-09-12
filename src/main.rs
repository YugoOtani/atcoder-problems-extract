mod api;
mod config;
mod html;
mod model;
mod select;

use anyhow::{bail, Context, Result};
use api::ApiClient;
use chrono::{FixedOffset, Utc};
use clap::{Parser, Subcommand};
use config::Config;
use model::ContestFile;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, ExitStatus},
};

#[derive(Parser)]
#[command(name = "daily", version, about = "Anonymous daily AtCoder mini-contest generator")]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate today's contest, or open it if it already exists.
    Start,
    /// Open a previously generated contest. Defaults to today.
    Open { date: Option<String> },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.canonicalize().unwrap_or(cli.root.clone());
    match cli.command {
        Command::Start => start(&root),
        Command::Open { date } => open_existing(&root, date.as_deref()),
    }
}

fn start(root: &Path) -> Result<()> {
    let config = Config::load(&root.join("config.toml"))?;
    let date = today_jst();
    let contest_dir = root.join("contests").join(&date);
    if contest_dir.join("index.html").exists() {
        println!("Today's contest already exists: {date}");
        return open_html(&contest_dir.join("index.html"));
    }

    fs::create_dir_all(root.join("contests"))?;
    let api = ApiClient::new(root.join("cache"), config.cache_ttl_hours)?;

    println!("Loading AtCoder Problems data...");
    let resources = api.load_resources()?;
    println!("Refreshing submissions for {}...", config.username);
    let submissions = api.refresh_submissions(&config.username)?;
    let prior = load_prior_contests(&root.join("contests"))?;

    let candidates = select::build_candidates(&config, &resources, &submissions, &prior);
    println!("Eligible problems: {}", candidates.len());
    let selected = select::select_problems(&config, &candidates)?;

    // Fetch statements before creating the final directory. If one fetch fails,
    // there is no half-generated contest that blocks a later retry.
    let mut statements = Vec::with_capacity(selected.len());
    println!("Fetching {} statements...", selected.len());
    for (i, p) in selected.iter().enumerate() {
        print!("  Q{}... ", i + 1);
        let raw = api.fetch_statement(&p.contest_id, &p.id)
            .with_context(|| format!("failed to fetch statement for selected Q{}", i + 1))?;
        let statement = html::extract_statement(&raw)
            .with_context(|| format!("failed to parse statement for selected Q{}", i + 1))?;
        statements.push(statement);
        println!("ok");
    }

    let temp_dir = root.join("contests").join(format!(".{date}.tmp"));
    if temp_dir.exists() { fs::remove_dir_all(&temp_dir)?; }
    html::write_contest(&temp_dir, &date, &selected, &statements)?;
    fs::rename(&temp_dir, &contest_dir)
        .with_context(|| format!("failed to finalize {}", contest_dir.display()))?;
    println!("Generated: {}", contest_dir.display());
    open_html(&contest_dir.join("index.html"))
}

fn open_existing(root: &Path, date: Option<&str>) -> Result<()> {
    let date = date.map(ToOwned::to_owned).unwrap_or_else(today_jst);
    validate_date(&date)?;
    let index = root.join("contests").join(&date).join("index.html");
    if !index.exists() { bail!("contest not found: {date}"); }
    open_html(&index)
}

fn today_jst() -> String {
    let jst = FixedOffset::east_opt(9 * 3600).expect("valid JST offset");
    Utc::now().with_timezone(&jst).format("%Y-%m-%d").to_string()
}

fn validate_date(s: &str) -> Result<()> {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date '{s}', expected YYYY-MM-DD"))?;
    Ok(())
}

fn open_html(path: &Path) -> Result<()> {
    let absolute = path.canonicalize()?;
    open_file(&absolute)?;
    println!("Opened: {}", absolute.display());
    Ok(())
}

fn open_file(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let status = ProcessCommand::new("explorer.exe")
        .arg(path)
        .status()
        .context("failed to run explorer.exe")?;

    #[cfg(target_os = "macos")]
    let status = ProcessCommand::new("open")
        .arg(path)
        .status()
        .context("failed to run open")?;

    #[cfg(target_os = "linux")]
    let status = if is_wsl() {
        let output = ProcessCommand::new("wslpath")
            .arg("-w")
            .arg(path)
            .output()
            .context("failed to run wslpath")?;
        ensure_success("wslpath", output.status)?;
        let windows_path =
            String::from_utf8(output.stdout).context("wslpath returned a non-UTF-8 path")?;

        ProcessCommand::new("explorer.exe")
            .arg(windows_path.trim_end_matches(['\r', '\n']))
            .status()
            .context("failed to run explorer.exe")?
    } else {
        ProcessCommand::new("xdg-open")
            .arg(path)
            .status()
            .context("failed to run xdg-open")?
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    bail!("opening files is not supported on this OS");

    ensure_success("OS file opener", status)
}

#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    std::env::var_os("WSL_INTEROP").is_some()
        || std::env::var_os("WSL_DISTRO_NAME").is_some()
        || fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"))
}

fn ensure_success(command: &str, status: ExitStatus) -> Result<()> {
    if !status.success() {
        bail!("{command} exited with {status}");
    }
    Ok(())
}

fn load_prior_contests(contests_dir: &Path) -> Result<Vec<ContestFile>> {
    if !contests_dir.exists() { return Ok(Vec::new()); }
    let mut result = Vec::new();
    for entry in fs::read_dir(contests_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() { continue; }
        let path = entry.path().join("contest.json");
        if !path.exists() { continue; }
        let bytes = fs::read(&path)?;
        match serde_json::from_slice::<ContestFile>(&bytes) {
            Ok(contest) => result.push(contest),
            Err(err) => eprintln!("warning: skipping invalid {}: {err}", path.display()),
        }
    }
    Ok(result)
}
