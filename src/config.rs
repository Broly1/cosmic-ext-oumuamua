// SPDX-License-Identifier: GPL-3.0-only
//! Settings shared by the centered launcher and the panel applet.
//!
//! Stored with cosmic-config under `~/.config/cosmic/<APP_ID>/v1/`, so both
//! processes see changes live (the applet watches it to switch modes).

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APP_ID: &str = "io.github.broly1.CosmicAppGrid";

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
    /// Grid size of one page in centered mode.
    pub columns: u16,
    pub rows: u16,
    /// Grid size of one page in panel mode (smaller popup).
    pub panel_columns: u16,
    pub panel_rows: u16,
    pub icon_size: u16,
    /// Desktop-entry IDs pinned to the start of the grid.
    pub favorites: Vec<String>,
}

impl Default for AppGridConfig {
    fn default() -> Self {
        Self {
            position: Position::Centered,
            columns: 7,
            rows: 4,
            panel_columns: 6,
            panel_rows: 4,
            icon_size: 64,
            favorites: Vec::new(),
        }
    }
}

impl AppGridConfig {
    pub fn helper() -> Option<cosmic_config::Config> {
        cosmic_config::Config::new(APP_ID, Self::VERSION).ok()
    }

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

    pub fn toggle_favorite(&mut self, helper: Option<&cosmic_config::Config>, id: &str) {
        let mut favs = self.favorites.clone();
        if let Some(i) = favs.iter().position(|f| f == id) {
            favs.remove(i);
        } else {
            favs.push(id.to_string());
        }
        if let Some(h) = helper {
            if let Err(e) = self.set_favorites(h, favs.clone()) {
                log::error!("failed to save favorites: {e:?}");
            }
        }
        self.favorites = favs;
    }

    pub fn save_position(&mut self, helper: Option<&cosmic_config::Config>, position: Position) {
        if let Some(h) = helper {
            if let Err(e) = self.set_position(h, position) {
                log::error!("failed to save position: {e:?}");
            }
        }
        self.position = position;
    }
}
