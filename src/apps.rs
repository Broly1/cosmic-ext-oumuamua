// SPDX-License-Identifier: GPL-3.0-only
//! Installed applications: loading, searching and launching.

use std::sync::Arc;

use cosmic::desktop::{DesktopEntryData, IconSourceExt};
use cosmic::widget::icon;

/// macOS-style app categories, mapped from freedesktop.org main/additional
/// categories in the desktop entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Productivity,
    Utilities,
    Social,
    Creativity,
    Entertainment,
    Other,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Productivity,
        Category::Utilities,
        Category::Social,
        Category::Creativity,
        Category::Entertainment,
        Category::Other,
    ];

    pub fn label(self) -> String {
        match self {
            Category::Productivity => crate::fl!("category-productivity"),
            Category::Utilities => crate::fl!("category-utilities"),
            Category::Social => crate::fl!("category-social"),
            Category::Creativity => crate::fl!("category-creativity"),
            Category::Entertainment => crate::fl!("category-entertainment"),
            Category::Other => crate::fl!("category-other"),
        }
    }

    /// Checked in this order, so e.g. an `Office;Graphics` viewer counts as
    /// Productivity and `AudioVideoEditing` as Creativity, not Entertainment.
    fn from_freedesktop(cats: &[String]) -> Self {
        const RULES: &[(Category, &[&str])] = &[
            (
                Category::Productivity,
                &[
                    "Office",
                    "Finance",
                    "Calendar",
                    "ContactManagement",
                    "ProjectManagement",
                    "WordProcessor",
                    "Spreadsheet",
                    "Presentation",
                    "Dictionary",
                    "Chart",
                    "Database",
                ],
            ),
            (
                Category::Creativity,
                &[
                    "Graphics",
                    "2DGraphics",
                    "3DGraphics",
                    "RasterGraphics",
                    "VectorGraphics",
                    "Photography",
                    "Publishing",
                    "AudioVideoEditing",
                    "Art",
                    "Development",
                    "IDE",
                    "WebDevelopment",
                    "Building",
                    "Debugger",
                ],
            ),
            (
                Category::Social,
                &[
                    "Chat",
                    "InstantMessaging",
                    "IRCClient",
                    "Email",
                    "Telephony",
                    "VideoConference",
                    "News",
                    "Feed",
                    "WebBrowser",
                    "Network",
                ],
            ),
            (
                Category::Entertainment,
                &[
                    "Game",
                    "AudioVideo",
                    "Audio",
                    "Video",
                    "Player",
                    "Music",
                    "TV",
                    "Recorder",
                ],
            ),
            (
                Category::Utilities,
                &[
                    "System",
                    "Settings",
                    "Utility",
                    "Accessibility",
                    "FileManager",
                    "TerminalEmulator",
                    "Monitor",
                    "Archiving",
                    "Compression",
                    "FileTools",
                    "Security",
                    "PackageManager",
                    "HardwareSettings",
                    "Filesystem",
                    "Core",
                    "TextEditor",
                    "Calculator",
                    "Clock",
                ],
            ),
        ];
        for (cat, names) in RULES {
            if cats.iter().any(|c| names.contains(&c.as_str())) {
                return *cat;
            }
        }
        Category::Other
    }
}

#[derive(Debug, Clone)]
pub struct App {
    pub entry: DesktopEntryData,
    pub icon: icon::Handle,
    pub category: Category,
    /// Lower-cased name, used for matching.
    name_lc: String,
    /// Lower-cased id + categories, used as a weaker match.
    extra_lc: String,
}

impl App {
    fn new(entry: DesktopEntryData) -> Self {
        let icon = entry.icon.as_cosmic_icon();
        let category = Category::from_freedesktop(&entry.categories);
        let name_lc = entry.name.to_lowercase();
        let extra_lc = format!("{} {}", entry.id, entry.categories.join(" ")).to_lowercase();
        Self {
            entry,
            icon,
            category,
            name_lc,
            extra_lc,
        }
    }

    pub fn id(&self) -> &str {
        &self.entry.id
    }

    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// Lower score = better match; `None` = no match.
    pub fn score(&self, query_lc: &str) -> Option<u32> {
        if query_lc.is_empty() {
            return Some(0);
        }
        let name = self.name_lc.as_str();
        if name.starts_with(query_lc) {
            return Some(0);
        }
        if name
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(query_lc))
        {
            return Some(1);
        }
        if name.contains(query_lc) {
            return Some(2);
        }
        if is_subsequence(query_lc, name) {
            return Some(3);
        }
        if self.extra_lc.contains(query_lc) {
            return Some(4);
        }
        None
    }
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn locales() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(lang) = std::env::var("LANG") {
        let base = lang.split('.').next().unwrap_or_default().to_string();
        if let Some((short, _)) = base.split_once('_') {
            v.push(short.to_string());
        }
        v.insert(0, base);
    }
    v
}

/// Load visible applications, sorted A–Z.
#[must_use]
pub fn load() -> Vec<Arc<App>> {
    let locales = locales();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let mut apps: Vec<Arc<App>> =
        cosmic::desktop::load_applications(&locales, false, desktop.as_deref())
            .filter(|d| d.exec.is_some())
            .map(|d| Arc::new(App::new(d)))
            .collect();
    apps.sort_by(|a, b| a.name_lc.cmp(&b.name_lc));
    apps.dedup_by(|a, b| a.entry.id == b.entry.id);
    apps
}

/// Launch an application detached from this process (double fork + setsid),
/// passing the Wayland activation token so the new window gets focus.
pub async fn launch(app: Arc<App>, token: Option<String>) {
    let mut env = Vec::new();
    if let Some(t) = token {
        env.push(("XDG_ACTIVATION_TOKEN".to_string(), t.clone()));
        env.push(("DESKTOP_STARTUP_ID".to_string(), t));
    }
    let result = cosmic_launch::Launcher::new(&app.entry)
        .env_vars(env)
        .working_directory(app.entry.working_directory.as_deref())
        .spawn_detached(&[] as &[&str])
        .await;
    if let Err(e) = result {
        log::error!("failed to launch {}: {e:?}", app.id());
    }
}
