use crate::model::{ContestFile, ContestProblemFile};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const BOOKMARKS_FILENAME: &str = "bookmarks.json";
const BOOKMARKS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub problem_id: String,
    pub contest_id: String,
    pub problem_index: String,
    pub title: String,
    pub difficulty: i64,
    pub url: String,
    pub submit_url: String,
    pub bookmarked_at: String,
    pub source_date: String,
    pub source_slot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BookmarkStore {
    #[serde(default = "bookmark_version")]
    pub version: u32,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl Default for BookmarkStore {
    fn default() -> Self {
        Self {
            version: BOOKMARKS_VERSION,
            bookmarks: Vec::new(),
        }
    }
}

pub fn load(root: &Path) -> Result<BookmarkStore> {
    let path = root.join(BOOKMARKS_FILENAME);
    if !path.exists() {
        return Ok(BookmarkStore::default());
    }

    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let store: BookmarkStore = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if store.version != BOOKMARKS_VERSION {
        bail!(
            "unsupported bookmarks version {} in {}",
            store.version,
            path.display()
        );
    }
    Ok(store)
}

pub fn is_bookmarked(root: &Path, date: &str, slot: &str) -> Result<bool> {
    let problem = resolve_problem(root, date, slot)?;
    let store = load(root)?;
    Ok(store
        .bookmarks
        .iter()
        .any(|bookmark| bookmark.problem_id == problem.problem_id))
}

pub fn add(root: &Path, date: &str, slot: &str) -> Result<bool> {
    let problem = resolve_problem(root, date, slot)?;
    let mut store = load(root)?;
    if store
        .bookmarks
        .iter()
        .any(|bookmark| bookmark.problem_id == problem.problem_id)
    {
        return Ok(true);
    }

    store.bookmarks.insert(
        0,
        Bookmark {
            problem_id: problem.problem_id,
            contest_id: problem.contest_id,
            problem_index: problem.problem_index,
            title: problem.title,
            difficulty: problem.difficulty,
            url: problem.url,
            submit_url: problem.submit_url,
            bookmarked_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            source_date: date.to_owned(),
            source_slot: slot.to_owned(),
        },
    );
    save(root, &store)?;
    Ok(true)
}

pub fn remove(root: &Path, date: &str, slot: &str) -> Result<bool> {
    let problem = resolve_problem(root, date, slot)?;
    remove_by_problem_id(root, &problem.problem_id)
}

pub fn remove_by_problem_id(root: &Path, problem_id: &str) -> Result<bool> {
    let mut store = load(root)?;
    let old_len = store.bookmarks.len();
    store
        .bookmarks
        .retain(|bookmark| bookmark.problem_id != problem_id);
    if store.bookmarks.len() != old_len {
        save(root, &store)?;
    }
    Ok(false)
}

fn resolve_problem(root: &Path, date: &str, slot: &str) -> Result<ContestProblemFile> {
    validate_date_and_slot(date, slot)?;
    let path = root.join("contests").join(date).join("contest.json");
    let bytes = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let contest: ContestFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    contest
        .problems
        .into_iter()
        .find(|problem| problem.slot == slot)
        .with_context(|| format!("problem slot {slot} not found in contest {date}"))
}

fn validate_date_and_slot(date: &str, slot: &str) -> Result<()> {
    let parsed = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("invalid contest date '{date}'"))?;
    if parsed.format("%Y-%m-%d").to_string() != date {
        bail!("invalid contest date '{date}'");
    }
    let Some(number) = slot.strip_prefix('q') else {
        bail!("invalid problem slot '{slot}'");
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("invalid problem slot '{slot}'");
    }
    Ok(())
}

fn save(root: &Path, store: &BookmarkStore) -> Result<()> {
    let path = root.join(BOOKMARKS_FILENAME);
    let temp_path = root.join(format!(".{BOOKMARKS_FILENAME}.tmp"));
    let bytes = serde_json::to_vec_pretty(store)?;
    fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn bookmark_version() -> u32 {
    BOOKMARKS_VERSION
}

#[cfg(test)]
mod tests {
    use super::{add, is_bookmarked, load, remove, remove_by_problem_id};
    use crate::model::{ContestFile, ContestProblemFile};
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "daily-atcoder-bookmark-{name}-{}",
            std::process::id()
        ))
    }

    fn create_contest(root: &Path) {
        let dir = root.join("contests/2026-09-20");
        fs::create_dir_all(&dir).unwrap();
        let contest = ContestFile {
            date: "2026-09-20".into(),
            problems: vec![ContestProblemFile {
                slot: "q1".into(),
                problem_id: "abc123_d".into(),
                contest_id: "abc123".into(),
                problem_index: "D".into(),
                title: "Example".into(),
                difficulty: 1200,
                raw_difficulty: 1200.0,
                url: "https://example.com/problem".into(),
                submit_url: "https://example.com/submit".into(),
            }],
        };
        fs::write(
            dir.join("contest.json"),
            serde_json::to_vec(&contest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn bookmark_round_trip_is_persistent_and_deduplicated() {
        let root = test_root("round-trip");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        create_contest(&root);

        assert!(!is_bookmarked(&root, "2026-09-20", "q1").unwrap());
        assert!(add(&root, "2026-09-20", "q1").unwrap());
        assert!(add(&root, "2026-09-20", "q1").unwrap());
        assert!(is_bookmarked(&root, "2026-09-20", "q1").unwrap());
        assert_eq!(load(&root).unwrap().bookmarks.len(), 1);
        assert!(!remove(&root, "2026-09-20", "q1").unwrap());
        assert!(!is_bookmarked(&root, "2026-09-20", "q1").unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bookmark_can_be_removed_by_problem_id() {
        let root = test_root("remove-id");
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        create_contest(&root);
        add(&root, "2026-09-20", "q1").unwrap();

        assert!(!remove_by_problem_id(&root, "abc123_d").unwrap());
        assert!(load(&root).unwrap().bookmarks.is_empty());

        fs::remove_dir_all(root).unwrap();
    }
}
