mod api;
mod bookmark;
mod config;
mod html;
mod model;
mod select;
mod server;
mod settings;

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
#[cfg(target_os = "linux")]
use std::process::Stdio;

#[derive(Parser)]
#[command(name = "daily", version, about = "Anonymous daily AtCoder mini-contest generator")]
struct Cli {
    /// Data root. When specified, it is saved to ~/.daily-config for future runs.
    #[arg(long)]
    root: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate today's contest, or open it if it already exists.
    Start,
    /// Open a previously generated contest. Defaults to today.
    Open { date: Option<String> },
    /// Manage the default data root.
    Root {
        #[command(subcommand)]
        command: RootCommand,
    },
}

#[derive(Subcommand)]
enum RootCommand {
    /// Save the data root used when --root is omitted.
    Set { path: PathBuf },
}

fn main() -> Result<()> {
    let Cli { root, command } = Cli::parse();
    match command {
        Command::Root {
            command: RootCommand::Set { path },
        } => {
            let root = settings::set_root(&path)?;
            println!("Saved data root: {}", root.display());
            Ok(())
        }
        command => {
            let root = settings::resolve_root(root.as_deref())?;
            match command {
                Command::Start => start(&root),
                Command::Open { date } => open_existing(&root, date.as_deref()),
                Command::Root { .. } => unreachable!(),
            }
        }
    }
}

fn start(root: &Path) -> Result<()> {
    let config = Config::load(&root.join("config.toml"))?;
    let date = today_jst();
    let contest_dir = root.join("contests").join(&date);
    if contest_dir.join("index.html").exists() {
        println!("Today's contest already exists: {date}");
        return serve_contest(root, &date);
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
    serve_contest(root, &date)
}

fn open_existing(root: &Path, date: Option<&str>) -> Result<()> {
    let date = date.map(ToOwned::to_owned).unwrap_or_else(today_jst);
    validate_date(&date)?;
    let index = root.join("contests").join(&date).join("index.html");
    if !index.exists() { bail!("contest not found: {date}"); }
    serve_contest(root, &date)
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

fn serve_contest(root: &Path, date: &str) -> Result<()> {
    let server = server::BookmarkServer::bind(root, date)?;
    let url = server.url()?;
    println!("Serving: {url}");
    println!("Press Ctrl+C to stop.");
    open_url(&url)?;
    server.run()
}

fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    let status = ProcessCommand::new("explorer.exe")
        .arg(url)
        .status()
        .context("failed to run explorer.exe")?;

    #[cfg(target_os = "macos")]
    let status = ProcessCommand::new("open")
        .arg(url)
        .status()
        .context("failed to run open")?;

    #[cfg(target_os = "linux")]
    let status = if is_wsl() {
        return open_url_in_wsl(url);
    } else {
        ProcessCommand::new("xdg-open")
            .arg(url)
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

#[cfg(target_os = "linux")]
fn open_url_in_wsl(url: &str) -> Result<()> {
    const FIREFOX_LOCATIONS: [&str; 2] = [
        "/mnt/c/Program Files/Mozilla Firefox/firefox.exe",
        "/mnt/c/Program Files (x86)/Mozilla Firefox/firefox.exe",
    ];
    for firefox in FIREFOX_LOCATIONS {
        if Path::new(firefox).is_file() && spawn_wsl_program(firefox, url).is_ok() {
            return Ok(());
        }
    }

    if spawn_wsl_program("firefox.exe", url).is_ok() {
        return Ok(());
    }

    // explorer.exe may exit with status 1 even when the associated application
    // was opened successfully, so only process creation is checked here.
    spawn_wsl_program("explorer.exe", url)
        .context("failed to launch Firefox or the Windows file opener")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn spawn_wsl_program(program: &str, path: &str) -> std::io::Result<()> {
    ProcessCommand::new(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
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
