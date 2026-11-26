// SPDX-License-Identifier: GPL-3.0-only
//! Built-in search plugins: calculator, files, recent documents, commands,
//! web search and session actions.
//!
//! Everything here is self-contained: Oumuamua never calls `cosmic-launcher`
//! or `pop-launcher`, so it works the same whether they are installed or not.
//!
//! [`search`] runs on every keystroke, so each plugin only does work when its
//! prefix matches, and the filesystem ones cap how much they read.

pub mod calc;
mod files;
pub mod find;
mod recent;
pub mod windows;

use std::process::Command;

use crate::grid::Power;

/// One search result shown above the app grid.
#[derive(Debug, Clone)]
pub struct Item {
    /// Icon name, with fallbacks tried in order when the theme lacks it.
    pub icons: Vec<String>,
    pub title: String,
    pub subtitle: String,
    /// Plugin name shown as a pill on the row ("Calculator"); empty for none.
    pub source: String,
    pub action: Action,
    /// What Tab puts in the search field (file paths).
    pub complete: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Action {
    /// Copy text to the clipboard (calculator results).
    Copy(String),
    /// Open a file, folder or URL with its default app.
    Open(String),
    /// Run a shell command, in a terminal window or in the background.
    Run { cmd: String, terminal: bool },
    /// Replace the search text (the `?` help list).
    Fill(String),
    /// Bring an open window to the front ([`windows::focus`]).
    Focus(u64),
    Power(Power),
}

/// Results for one query.
#[derive(Debug, Default)]
pub struct Results {
    pub items: Vec<Item>,
    /// The query used a plugin prefix (`=`, `~/`, `run `…): hide the apps.
    pub isolate: bool,
    /// `find …`: a file search to run in the background ([`find::search`]).
    pub find: Option<String>,
}

/// `icon` followed by stand-ins for themes that lack it: Pop's Universe
/// theme has no `system-search` or `system-run`, and its Adwaita fallback
/// only has symbolic versions.
fn with_fallbacks(icon: &str) -> Vec<String> {
    let extra: &[&str] = match icon {
        "system-search" => &["edit-find", "system-search-symbolic"],
        "system-run" => &["application-x-executable", "system-run-symbolic"],
        "document-open-recent" => &["folder-recent", "document-open-recent-symbolic", "text-x-generic"],
        _ => &[],
    };
    std::iter::once(icon).chain(extra.iter().copied()).map(String::from).collect()
}

impl Item {
    fn new(
        icon: &str,
        title: impl Into<String>,
        subtitle: impl Into<String>,
        source: String,
        action: Action,
    ) -> Self {
        Self {
            icons: with_fallbacks(icon),
            title: title.into(),
            subtitle: subtitle.into(),
            source,
            action,
            complete: None,
        }
    }
}

/// Web search engines: prefix, name, URL the encoded query is appended to.
const ENGINES: [(&str, &str, &str); 7] = [
    ("ddg", "DuckDuckGo", "https://duckduckgo.com/?q="),
    ("g", "Google", "https://www.google.com/search?q="),
    ("google", "Google", "https://www.google.com/search?q="),
    ("w", "Wikipedia", "https://www.wikipedia.org/search-redirect.php?search="),
    ("wiki", "Wikipedia", "https://www.wikipedia.org/search-redirect.php?search="),
    ("yt", "YouTube", "https://www.youtube.com/results?search_query="),
    ("gh", "GitHub", "https://github.com/search?q="),
];

/// Run every plugin that matches `query`.
#[must_use]
pub fn search(query: &str) -> Results {
    let q = query.trim_start();
    if q.is_empty() {
        return Results::default();
    }

    if q == "?" {
        return Results {
            items: help(),
            isolate: true,
            find: None,
        };
    }

    // ---- prefixed plugins: only their results are shown
    if let Some(expr) = q.strip_prefix('=') {
        let items = calc_item(expr).into_iter().collect();
        return Results {
            items,
            isolate: true,
            find: None,
        };
    }
    if q.starts_with('/') || q.starts_with('~') {
        return Results {
            items: files::list(q),
            isolate: true,
            find: None,
        };
    }
    if let Some(term) = strip_word(q, "find") {
        let term = term.trim();
        return Results {
            items: Vec::new(),
            isolate: true,
            // one letter would match nearly everything
            find: (term.chars().count() >= 2).then(|| term.to_string()),
        };
    }
    // "recent" alone lists them all
    if let Some(rest) = strip_word(q, "recent").or_else(|| q.eq_ignore_ascii_case("recent").then_some("")) {
        return Results {
            items: recent::search(rest),
            isolate: true,
            find: None,
        };
    }
    // "run cmd" and "t:cmd" open a terminal so the output can be read;
    // ":cmd" runs silently in the background (as in Plasma's AppGrid)
    if let Some(cmd) = q.strip_prefix("t:").or_else(|| strip_word(q, "run")) {
        return Results {
            items: run_item(cmd, true).into_iter().collect(),
            isolate: true,
            find: None,
        };
    }
    if let Some(cmd) = q.strip_prefix(':') {
        return Results {
            items: run_item(cmd, false).into_iter().collect(),
            isolate: true,
            find: None,
        };
    }
    for (prefix, name, url) in ENGINES {
        if let Some(terms) = strip_word(q, prefix) {
            let terms = terms.trim();
            let items = (!terms.is_empty())
                .then(|| {
                    Item::new(
                        "web-browser",
                        crate::fl!("search-web", engine = name, query = terms),
                        url.to_string() + &encode(terms),
                        crate::fl!("plugin-web"),
                        Action::Open(url.to_string() + &encode(terms)),
                    )
                })
                .into_iter()
                .collect();
            return Results {
                items,
                isolate: true,
                find: None,
            };
        }
    }

    // ---- plain text: extras listed after the matching apps
    let mut items = windows::search(q);
    if calc::looks_like_maths(q) {
        items.extend(calc_item(q));
    }
    items.extend(session_items(q));
    Results {
        items,
        isolate: false,
        find: None,
    }
}

/// `word` followed by a space at the start of `q`, case-insensitively.
fn strip_word<'a>(q: &'a str, word: &str) -> Option<&'a str> {
    let head = q.get(..word.len())?;
    (head.eq_ignore_ascii_case(word) && q[word.len()..].starts_with(' '))
        .then(|| &q[word.len() + 1..])
}

fn encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn calc_item(expr: &str) -> Option<Item> {
    let v = calc::eval(expr)?;
    let result = calc::format(v);
    Some(Item::new(
        "accessories-calculator",
        format!("= {result}"),
        format!("{} · {}", expr.trim(), crate::fl!("copy-to-clipboard")),
        crate::fl!("plugin-calculator"),
        Action::Copy(result),
    ))
}

fn run_item(cmd: &str, terminal: bool) -> Option<Item> {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return None;
    }
    let subtitle = if terminal {
        crate::fl!("run-in-terminal")
    } else {
        crate::fl!("run-command")
    };
    Some(Item::new(
        if terminal { "utilities-terminal" } else { "system-run" },
        cmd,
        subtitle,
        crate::fl!("plugin-command"),
        Action::Run {
            cmd: cmd.to_string(),
            terminal,
        },
    ))
}

/// Power actions whose name (in English or the current language) starts
/// with the query, so "shut", "reboot" or "bloquear" find them.
fn session_items(q: &str) -> Vec<Item> {
    let q = q.trim().to_lowercase();
    if q.chars().count() < 3 {
        return Vec::new();
    }
    let keywords = |p: Power| -> &'static [&'static str] {
        match p {
            Power::Lock => &["lock", "lock screen"],
            Power::Suspend => &["suspend", "sleep"],
            Power::LogOut => &["log out", "logout", "sign out"],
            Power::Restart => &["restart", "reboot"],
            Power::Shutdown => &["shut down", "shutdown", "power off", "poweroff"],
        }
    };
    Power::ALL
        .into_iter()
        .filter(|&p| {
            p.label().to_lowercase().starts_with(&q)
                || keywords(p).iter().any(|k| k.starts_with(&q))
        })
        .map(|p| Item::new(p.icon(), p.label(), "", crate::fl!("plugin-session"), Action::Power(p)))
        .collect()
}

/// The `?` list: one entry per prefix, Enter types it in.
fn help() -> Vec<Item> {
    [
        ("accessories-calculator", "= ", crate::fl!("help-calc")),
        ("folder", "~/", crate::fl!("help-files")),
        ("system-search", "find ", crate::fl!("help-find")),
        ("document-open-recent", "recent ", crate::fl!("help-recent")),
        ("utilities-terminal", "run ", crate::fl!("help-run")),
        ("system-run", ":", crate::fl!("help-background")),
        ("web-browser", "ddg ", crate::fl!("help-web")),
    ]
    .into_iter()
    .map(|(icon, prefix, desc)| {
        Item::new(icon, prefix.trim_end(), desc, String::new(), Action::Fill(prefix.into()))
    })
    .collect()
}

/// Do what a result asks, except `Copy` and `Fill`, which need the UI.
pub fn perform(action: &Action) {
    match action {
        Action::Open(target) => crate::open_url(target),
        Action::Run { cmd, terminal } => run(cmd, *terminal),
        Action::Power(p) => p.run(),
        Action::Focus(key) => windows::focus(*key),
        Action::Copy(_) | Action::Fill(_) => {}
    }
}

fn run(cmd: &str, terminal: bool) {
    let mut c = if terminal {
        // keep the window open after the command so its output can be read
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".into());
        let script = format!("{cmd}; exec {shell}");
        let term = ["cosmic-term", "x-terminal-emulator", "kgx", "gnome-terminal", "konsole", "xterm"]
            .into_iter()
            .find(|t| in_path(t))
            .unwrap_or("xterm");
        let mut c = Command::new(term);
        if term == "gnome-terminal" {
            c.arg("--");
        } else {
            c.arg("-e");
        }
        c.args(["sh", "-c", &script]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    };
    if let Some(home) = dirs::home_dir() {
        c.current_dir(home);
    }
    match c.spawn() {
        // reap it in the background so it doesn't linger as a zombie
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => log::error!("failed to run {cmd}: {e}"),
    }
}

fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// `/home/me/Documents` as `~/Documents`.
fn tilde(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

/// Icon names for a file: its exact MIME icon, then the generic one.
fn file_icons(path: &std::path::Path, is_dir: bool) -> Vec<String> {
    if is_dir {
        return vec!["folder".into()];
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let generic = match mime.type_().as_str() {
        "image" => "image-x-generic",
        "audio" => "audio-x-generic",
        "video" => "video-x-generic",
        _ => "text-x-generic",
    };
    vec![
        mime.essence_str().replace('/', "-"),
        generic.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes() {
        assert!(search("= 2+2").isolate);
        assert_eq!(search("= 2+2").items[0].title, "= 4");
        assert!(!search("2+2").isolate);
        assert_eq!(search("2+2").items.len(), 1);
        assert!(search("firefox").items.is_empty());
        assert!(search("ddg rust").isolate);
        assert!(search("run ls").isolate);
        assert!(search(":ls").isolate);
        assert!(search("t:htop").isolate);
        assert!(search("recent x").isolate);
        // an app called "Runner" is not a command
        assert!(!search("runner").isolate);
    }

    #[test]
    fn which_commands_open_a_terminal() {
        let terminal = |q: &str| match &search(q).items[0].action {
            Action::Run { terminal, .. } => *terminal,
            a => panic!("{a:?}"),
        };
        assert!(terminal("run htop"));
        assert!(terminal("t:htop"));
        assert!(!terminal(":notify-send hi"));
    }

    #[test]
    fn web_url_is_encoded() {
        let r = search("ddg a&b c");
        match &r.items[0].action {
            Action::Open(u) => assert_eq!(u, "https://duckduckgo.com/?q=a%26b%20c"),
            a => panic!("{a:?}"),
        }
    }
}
