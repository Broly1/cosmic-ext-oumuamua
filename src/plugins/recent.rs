// SPDX-License-Identifier: GPL-3.0-only
//! Recently opened documents, from the freedesktop list that GTK, Qt/KDE and
//! COSMIC apps write to (`~/.local/share/recently-used.xbel`). Newest first,
//! filtered by name; files that no longer exist are skipped.

use std::path::PathBuf;

use super::{Action, Item, file_icons, tilde};

const LIMIT: usize = 40;

pub fn search(query: &str) -> Vec<Item> {
    let Some(path) = dirs::data_dir().map(|d| d.join("recently-used.xbel")) else {
        return Vec::new();
    };
    let Ok(xml) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();

    let mut found: Vec<(String, PathBuf)> = bookmarks(&xml)
        .filter_map(|(href, modified)| {
            let path = PathBuf::from(file_url_path(href)?);
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            (name.contains(&q) && path.exists()).then(|| (modified.to_string(), path))
        })
        .collect();
    // ISO 8601 timestamps sort as text
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.dedup_by(|a, b| a.1 == b.1);

    found
        .into_iter()
        .take(LIMIT)
        .map(|(_, path)| {
            let is_dir = path.is_dir();
            Item {
                icons: file_icons(&path, is_dir),
                title: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                subtitle: tilde(path.parent().unwrap_or(&path)),
                source: crate::fl!("plugin-recent"),
                action: Action::Open(path.to_string_lossy().into_owned()),
                complete: None,
            }
        })
        .collect()
}

/// `(href, modified)` of each `<bookmark>` element. The format is simple and
/// machine-written, so plain scanning is enough (no XML parser needed).
fn bookmarks(xml: &str) -> impl Iterator<Item = (&str, &str)> {
    xml.split("<bookmark ").skip(1).filter_map(|chunk| {
        let tag = &chunk[..chunk.find('>')?];
        Some((attr(tag, "href")?, attr(tag, "modified").unwrap_or("")))
    })
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = format!(" {tag}").find(&key)? + key.len() - 1;
    let rest = &tag[start..];
    Some(&rest[..rest.find('"')?])
}

/// The local path of a `file://` URL, percent-decoded (`%20` → space).
fn file_url_path(href: &str) -> Option<String> {
    let encoded = href.strip_prefix("file://")?;
    let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8().ok()?;
    // XML entities that can appear in paths
    Some(
        decoded
            .replace("&amp;", "&")
            .replace("&apos;", "'")
            .replace("&quot;", "\""),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bookmarks() {
        let xml = r#"<xbel>
  <bookmark href="file:///home/me/My%20Doc.odt" added="2026-01-01T00:00:00Z" modified="2026-02-01T00:00:00Z" visited="x">
  </bookmark>
  <bookmark href="https://example.com" modified="2026-03-01T00:00:00Z"></bookmark>
</xbel>"#;
        let b: Vec<_> = bookmarks(xml).collect();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].0, "file:///home/me/My%20Doc.odt");
        assert_eq!(b[0].1, "2026-02-01T00:00:00Z");
        assert_eq!(file_url_path(b[0].0).unwrap(), "/home/me/My Doc.odt");
        assert!(file_url_path(b[1].0).is_none());
    }
}
