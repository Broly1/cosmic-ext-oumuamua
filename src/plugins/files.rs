// SPDX-License-Identifier: GPL-3.0-only
//! File navigation: `~/Doc` lists the entries of `~/` starting with "Doc".
//! Folders come first; hidden entries only show once the name starts with a
//! dot. Tab completes the selected entry, Enter opens it.

use std::path::PathBuf;

use super::{Action, Item, file_icons, tilde};

/// Most entries listed, so huge folders stay quick.
const LIMIT: usize = 40;

pub fn list(query: &str) -> Vec<Item> {
    // "~/Documents/" lists the folder; "~/Doc" lists ~/ filtered by "Doc".
    // Split the text itself: Path would drop a trailing "." ("~/x/." must
    // list the hidden entries of ~/x, not ~/ filtered by "x").
    let (dir_text, prefix) = if query == "~" {
        ("~/", String::new())
    } else {
        let cut = query.rfind('/').map_or(0, |i| i + 1);
        (&query[..cut], query[cut..].to_string())
    };
    let Some(dir) = expand(dir_text) else {
        return Vec::new();
    };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let prefix_lc = prefix.to_lowercase();
    let show_hidden = prefix.starts_with('.');

    let mut entries: Vec<(bool, String, PathBuf)> = read
        .flatten()
        .take(5000)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && !show_hidden {
                return None;
            }
            if !name.to_lowercase().starts_with(&prefix_lc) {
                return None;
            }
            // follows symlinks, so a link to a folder counts as a folder
            let is_dir = e.path().is_dir();
            Some((is_dir, name, e.path()))
        })
        .collect();
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });

    entries
        .into_iter()
        .take(LIMIT)
        .map(|(is_dir, name, path)| {
            let shown = tilde(&path);
            Item {
                icons: file_icons(&path, is_dir),
                title: if is_dir { format!("{name}/") } else { name },
                subtitle: tilde(path.parent().unwrap_or(&path)),
                source: crate::fl!("plugin-files"),
                action: Action::Open(path.to_string_lossy().into_owned()),
                complete: Some(if is_dir { format!("{shown}/") } else { shown }),
            }
        })
        .collect()
}

/// `~` and `~/x` expanded to the home folder; absolute paths as they are.
fn expand(query: &str) -> Option<PathBuf> {
    if let Some(rest) = query.strip_prefix('~') {
        let home = dirs::home_dir()?;
        let rest = rest.trim_start_matches('/');
        return Some(if rest.is_empty() {
            home
        } else {
            home.join(rest)
        });
    }
    query.starts_with('/').then(|| PathBuf::from(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_filters_and_shows_hidden_only_after_a_dot() {
        let root = std::env::temp_dir().join(format!("oumuamua-files-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Projects")).unwrap();
        for f in ["notes.txt", "Notebook.md", ".secret"] {
            std::fs::write(root.join(f), b"").unwrap();
        }
        let dir = format!("{}/", root.display());
        let names = |q: &str| -> Vec<String> { list(q).into_iter().map(|i| i.title).collect() };

        assert_eq!(names(&dir), ["Projects/", "Notebook.md", "notes.txt"]);
        assert_eq!(names(&format!("{dir}not")), ["Notebook.md", "notes.txt"]);
        assert_eq!(names(&format!("{dir}.")), [".secret"]);
        let tab = list(&format!("{dir}Pro")).remove(0).complete.unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(tab.ends_with("/Projects/"), "{tab}");
    }
}
