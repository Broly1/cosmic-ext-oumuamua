// SPDX-License-Identifier: GPL-3.0-only
//! Settings shared by the centered launcher and the panel applet.
//!
//! Stored with cosmic-config under `~/.config/cosmic/<APP_ID>/v1/`, so both
//! processes see changes live (the applet watches it to switch modes).

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.broly1.AppGrid";

/// Where the grid opens.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Position {
    /// Always in the middle of the screen, wherever the panel is (default).
    #[default]
    Centered,
    /// As a popup attached to the panel button, like the Windows 11 menu.
    Panel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CosmicConfigEntry)]
#[version = 1]
pub struct AppGridConfig {
    pub position: Position,
    /// Icons per row.
    pub columns: u16,
    /// Rows visible at once (the rest scrolls).
    pub rows: u16,
    pub icon_size: u16,
    /// Use the monochrome (theme-tinted) panel icon instead of the colored one.
    pub symbolic_icon: bool,
    /// Desktop-entry IDs of recently launched apps, most recent first.
    pub recent: Vec<String>,
}

const MAX_RECENT: usize = 24;

impl Default for AppGridConfig {
    fn default() -> Self {
        Self {
            position: Position::Centered,
            columns: 5,
            rows: 4,
            icon_size: 52,
            symbolic_icon: false,
            recent: Vec::new(),
        }
    }
}

impl AppGridConfig {
    #[must_use]
    pub fn helper() -> Option<cosmic_config::Config> {
        cosmic_config::Config::new(APP_ID, Self::VERSION).ok()
    }

    #[must_use]
    pub fn load() -> (Option<cosmic_config::Config>, Self) {
        let helper = Self::helper();
        let config = helper
            .as_ref()
            .map(|h| {
                Self::get_entry(h).unwrap_or_else(|(errs, cfg)| {
                    for e in errs {
                        log::warn!("config: {e:?}");
                    }
                    cfg
                })
            })
            .unwrap_or_default();
        (helper, config)
    }

    /// Move `id` to the front of the recently-used list and save it.
    pub fn record_launch(&mut self, helper: Option<&cosmic_config::Config>, id: &str) {
        let mut recent = self.recent.clone();
        recent.retain(|r| r != id);
        recent.insert(0, id.to_string());
        recent.truncate(MAX_RECENT);
        self.save_recent(helper, recent);
    }

    pub fn clear_recent(&mut self, helper: Option<&cosmic_config::Config>) {
        self.save_recent(helper, Vec::new());
    }

    fn save_recent(&mut self, helper: Option<&cosmic_config::Config>, recent: Vec<String>) {
        if let Some(h) = helper
            && let Err(e) = self.set_recent(h, recent.clone())
        {
            log::error!("failed to save recent apps: {e:?}");
        }
        self.recent = recent;
    }

    pub fn toggle_symbolic_icon(&mut self, helper: Option<&cosmic_config::Config>) {
        let v = !self.symbolic_icon;
        if let Some(h) = helper
            && let Err(e) = self.set_symbolic_icon(h, v)
        {
            log::error!("failed to save icon style: {e:?}");
        }
        self.symbolic_icon = v;
    }

    pub fn save_position(&mut self, helper: Option<&cosmic_config::Config>, position: Position) {
        if let Some(h) = helper
            && let Err(e) = self.set_position(h, position)
        {
            log::error!("failed to save position: {e:?}");
        }
        self.position = position;
    }
}
