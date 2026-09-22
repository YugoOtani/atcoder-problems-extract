use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

const SETTINGS_FILENAME: &str = ".daily-config";

#[derive(Debug, Deserialize, Serialize)]
struct Settings {
    root: PathBuf,
}

pub fn resolve_root(explicit_root: Option<&Path>) -> Result<PathBuf> {
    let current_dir = env::current_dir().context("failed to determine the current directory")?;
    let settings_path = home_dir()?.join(SETTINGS_FILENAME);
    resolve_root_at(explicit_root, &settings_path, &current_dir)
}

pub fn set_root(root: &Path) -> Result<PathBuf> {
    resolve_root(Some(root))
}

fn resolve_root_at(
    explicit_root: Option<&Path>,
    settings_path: &Path,
    current_dir: &Path,
) -> Result<PathBuf> {
    if let Some(root) = explicit_root {
        let root = absolute_path(root, current_dir);
        save(settings_path, &root)?;
        return Ok(root);
    }

    match fs::read_to_string(settings_path) {
        Ok(text) => {
            let settings: Settings = toml::from_str(&text)
                .with_context(|| format!("failed to parse {}", settings_path.display()))?;
            Ok(absolute_path(&settings.root, current_dir))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(absolute_path(current_dir, current_dir))
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", settings_path.display())),
    }
}

fn save(settings_path: &Path, root: &Path) -> Result<()> {
    let text = toml::to_string(&Settings {
        root: root.to_owned(),
    })
    .context("failed to serialize root setting")?;
    fs::write(settings_path, text)
        .with_context(|| format!("failed to write {}", settings_path.display()))
}

fn absolute_path(path: &Path, current_dir: &Path) -> PathBuf {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        current_dir.join(path)
    };
    path.canonicalize().unwrap_or(path)
}

fn home_dir() -> Result<PathBuf> {
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    #[cfg(target_os = "windows")]
    if let Some(home) = env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(home));
    }

    bail!("could not determine the home directory for {SETTINGS_FILENAME}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "daily-atcoder-settings-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn explicit_root_is_saved_and_reused() {
        let base = test_dir("saved-root");
        let working_dir = base.join("working");
        let data_root = base.join("data");
        let settings_path = base.join(SETTINGS_FILENAME);
        fs::create_dir_all(&working_dir).unwrap();
        fs::create_dir_all(&data_root).unwrap();

        let resolved =
            resolve_root_at(Some(Path::new("../data")), &settings_path, &working_dir).unwrap();
        assert_eq!(resolved, data_root.canonicalize().unwrap());

        let reused = resolve_root_at(None, &settings_path, &working_dir).unwrap();
        assert_eq!(reused, resolved);
        let saved: Settings = toml::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(saved.root, resolved);

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn current_directory_is_used_without_saved_settings() {
        let base = test_dir("default-root");
        let working_dir = base.join("working");
        fs::create_dir_all(&working_dir).unwrap();

        let resolved = resolve_root_at(None, &base.join(SETTINGS_FILENAME), &working_dir).unwrap();
        assert_eq!(resolved, working_dir.canonicalize().unwrap());

        fs::remove_dir_all(base).unwrap();
    }
}
