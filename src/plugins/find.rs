// SPDX-License-Identifier: GPL-3.0-only
//! File search: `find report` looks for files and folders under the home
//! folder whose name contains every word typed ("find tax 2025").
//!
//! The walk runs on a blocking thread so typing never stalls. Each new query
//! bumps a generation counter, and a walk that sees a newer generation stops
//! at once, so only the latest search keeps the disk busy. Hidden folders,
//! build output and caches are skipped, and the walk stops after enough
//! matches or a few seconds.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::{Action, Item, file_icons, tilde};

/// Matches shown.
const LIMIT: usize = 50;
/// Matches collected before ranking (so better names can win).
const COLLECT: usize = 300;
/// The walk gives up after this long, keeping what it found.
const TIME_LIMIT: Duration = Duration::from_secs(4);
/// Folders never worth descending into.
const SKIP: [&str; 9] = [
    "node_modules",
    "target",
    "__pycache__",
    "venv",
    "snap",
    "Trash",
    "site-packages",
    "vendor",
    "build",
];

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Start a new search generation; older walks stop.
pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

/// Search the home folder for `term`. Returns `None` if a newer search
/// started meanwhile (the result would be stale).
pub async fn search(term: String, generation: u64) -> Option<Vec<Item>> {
    tokio::task::spawn_blocking(move || {
        let home = dirs::home_dir()?;
        walk(&home, &term, generation)
    })
    .await
    .ok()
    .flatten()
}

fn walk(root: &Path, term: &str, generation: u64) -> Option<Vec<Item>> {
    let words: Vec<String> = term.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Some(Vec::new());
    }
    let start = Instant::now();
    let mut found: Vec<(u8, usize, String, PathBuf, bool)> = Vec::new();

    let entries = walkdir::WalkDir::new(root)
        .min_depth(1)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !(name.starts_with('.') || (e.file_type().is_dir() && SKIP.contains(&name.as_ref())))
        });
    for (n, entry) in entries.flatten().enumerate() {
        // check for a newer search every few hundred entries
        if n % 256 == 0
            && (GENERATION.load(Ordering::SeqCst) != generation || start.elapsed() > TIME_LIMIT)
        {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return None;
            }
            break;
        }
        let name = entry.file_name().to_string_lossy();
        let lc = name.to_lowercase();
        if !words.iter().all(|w| lc.contains(w.as_str())) {
            continue;
        }
        // rank: whole name, then starts with the first word, then anywhere
        let rank = if lc == words.join(" ") {
            0
        } else if lc.starts_with(words[0].as_str()) {
            1
        } else {
            2
        };
        found.push((
            rank,
            entry.depth(),
            name.into_owned(),
            entry.path().to_path_buf(),
            entry.file_type().is_dir(),
        ));
        if found.len() >= COLLECT {
            break;
        }
    }

    found.sort_by(|a, b| {
        (a.0, a.1)
            .cmp(&(b.0, b.1))
            .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
    });
    Some(
        found
            .into_iter()
            .take(LIMIT)
            .map(|(_, _, name, path, is_dir)| Item {
                icons: file_icons(&path, is_dir),
                title: if is_dir { format!("{name}/") } else { name },
                subtitle: tilde(path.parent().unwrap_or(&path)),
                source: crate::fl!("plugin-find"),
                action: Action::Open(path.to_string_lossy().into_owned()),
                complete: None,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_all_words_and_skips_hidden_and_build_folders() {
        let root = std::env::temp_dir().join(format!("oumuamua-find-{}", std::process::id()));
        let mk = |p: &str| {
            let p = root.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"").unwrap();
        };
        mk("Documents/Tax Report 2025.pdf");
        mk("Documents/old/tax-report-2024.odt");
        mk("Documents/notes.txt");
        mk(".cache/tax report hidden.txt");
        mk("code/target/tax report build.txt");

        let generation = next_generation();
        let items = walk(&root, "tax report", generation).unwrap();
        let names: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(names, ["Tax Report 2025.pdf", "tax-report-2024.odt"]);
    }

    #[test]
    fn a_newer_search_cancels_an_older_one() {
        let old = next_generation();
        next_generation();
        assert!(walk(&std::env::temp_dir(), "x", old).is_none());
    }
}
