// SPDX-License-Identifier: GPL-3.0-only
//! The application grid itself: search, paging, keyboard selection.
//!
//! Used unchanged by both the centered launcher and the panel applet popup;
//! each wraps [`Grid::view`] in its own surface and maps [`Msg`] into its own
//! message type.

use std::sync::{Arc, LazyLock};

use cosmic::iced::alignment::Horizontal;
use cosmic::iced::widget::{column, row, text};
use cosmic::iced::{Alignment, Length};
use cosmic::theme;
use cosmic::widget::{self, button, container, icon, mouse_area, search_input, tooltip};
use cosmic::Element;

use crate::apps::App;
use crate::config::{AppGridConfig, Position};

pub static SEARCH_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("appgrid-search"));

/// Extra width/height around the icon inside one cell (label + padding).
const CELL_PAD_W: f32 = 52.0;
const CELL_PAD_H: f32 = 56.0;
const SPACING: f32 = 6.0;
const OUTER_PAD: f32 = 24.0;
const HEADER_H: f32 = 64.0;
const FOOTER_H: f32 = 40.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Search(String),
    Submit,
    Launch(usize),
    ToggleFavorite(usize),
    NextPage,
    PrevPage,
    SetPage(usize),
    /// Mouse wheel: positive = scroll up.
    Scroll(f32),
    Move(i32, i32),
    TogglePosition,
}

/// What the owner of the grid needs to do after an update.
#[derive(Debug, Clone)]
pub enum Event {
    Launch(Arc<App>),
    ToggleFavorite(String),
    TogglePosition,
}

pub struct Grid {
    apps: Vec<Arc<App>>,
    query: String,
    /// Indices into `apps` that match the query, best first.
    filtered: Vec<usize>,
    page: usize,
    /// Index into `filtered`.
    selected: Option<usize>,
    columns: usize,
    rows: usize,
    icon_size: u16,
    favorites: Vec<String>,
    /// Which mode this grid is shown in (decides the "move" button icon).
    mode: Position,
    scroll_accum: f32,
}

impl Grid {
    pub fn new(mode: Position) -> Self {
        Self {
            apps: Vec::new(),
            query: String::new(),
            filtered: Vec::new(),
            page: 0,
            selected: None,
            columns: 7,
            rows: 4,
            icon_size: 64,
            favorites: Vec::new(),
            mode,
            scroll_accum: 0.0,
        }
    }

    pub fn apply_config(&mut self, config: &AppGridConfig) {
        let (c, r) = match self.mode {
            Position::Centered => (config.columns, config.rows),
            Position::Panel => (config.panel_columns, config.panel_rows),
        };
        self.columns = c.clamp(3, 12) as usize;
        self.rows = r.clamp(2, 8) as usize;
        self.icon_size = config.icon_size.clamp(32, 128);
        if self.favorites != config.favorites {
            self.favorites = config.favorites.clone();
            crate::apps::sort(&mut self.apps, &self.favorites);
            self.refilter();
        }
    }

    /// Reload the application list and reset search/page (called on open).
    pub fn reset(&mut self) {
        self.apps = crate::apps::load(&self.favorites);
        self.query.clear();
        self.page = 0;
        self.refilter();
    }

    /// Logical size of the grid panel, used to size/center the surface.
    pub fn panel_size(&self) -> (f32, f32) {
        let (cw, ch) = self.cell_size();
        let w = self.columns as f32 * (cw + SPACING) - SPACING + 2.0 * OUTER_PAD;
        let h = HEADER_H + self.rows as f32 * (ch + SPACING) - SPACING + FOOTER_H + 2.0 * OUTER_PAD;
        (w, h)
    }

    fn cell_size(&self) -> (f32, f32) {
        let i = f32::from(self.icon_size);
        (i + CELL_PAD_W, i + CELL_PAD_H)
    }

    fn per_page(&self) -> usize {
        self.columns * self.rows
    }

    fn pages(&self) -> usize {
        self.filtered.len().div_ceil(self.per_page()).max(1)
    }

    fn refilter(&mut self) {
        let q = self.query.trim().to_lowercase();
        let mut scored: Vec<(u32, usize)> = self
            .apps
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.score(&q).map(|s| (s, i)))
            .collect();
        // stable sort keeps favorites-then-A–Z order within the same score
        scored.sort_by_key(|(s, _)| *s);
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        self.page = self.page.min(self.pages() - 1);
        self.selected = if q.is_empty() || self.filtered.is_empty() { None } else { Some(0) };
    }

    fn app_at(&self, filtered_idx: usize) -> Option<&Arc<App>> {
        self.filtered.get(filtered_idx).and_then(|&i| self.apps.get(i))
    }

    fn set_page(&mut self, page: usize) {
        self.page = page.min(self.pages() - 1);
        if let Some(s) = self.selected {
            let pp = self.per_page();
            if s / pp != self.page {
                self.selected = Some(self.page * pp);
            }
        }
    }

    pub fn update(&mut self, msg: Msg) -> Option<Event> {
        match msg {
            Msg::Search(q) => {
                self.query = q;
                self.page = 0;
                self.refilter();
            }
            Msg::Submit => {
                let idx = self.selected.or(if self.filtered.is_empty() { None } else { Some(0) });
                if let Some(app) = idx.and_then(|i| self.app_at(i)) {
                    return Some(Event::Launch(app.clone()));
                }
            }
            Msg::Launch(i) => {
                if let Some(app) = self.app_at(i) {
                    return Some(Event::Launch(app.clone()));
                }
            }
            Msg::ToggleFavorite(i) => {
                if let Some(app) = self.app_at(i) {
                    return Some(Event::ToggleFavorite(app.id().to_string()));
                }
            }
            Msg::NextPage => self.set_page(self.page + 1),
            Msg::PrevPage => self.set_page(self.page.saturating_sub(1)),
            Msg::SetPage(p) => self.set_page(p),
            Msg::Scroll(dy) => {
                // accumulate so touchpads (many small deltas) flip one page at a time
                self.scroll_accum += dy;
                if self.scroll_accum <= -1.0 {
                    self.scroll_accum = 0.0;
                    self.set_page(self.page + 1);
                } else if self.scroll_accum >= 1.0 {
                    self.scroll_accum = 0.0;
                    self.set_page(self.page.saturating_sub(1));
                }
            }
            Msg::Move(dx, dy) => {
                if self.filtered.is_empty() {
                    return None;
                }
                let n = self.filtered.len() as i32;
                let cols = self.columns as i32;
                let cur = self.selected.map_or(-1, |s| s as i32);
                let next = if cur < 0 {
                    (self.page * self.per_page()) as i32
                } else {
                    (cur + dx + dy * cols).clamp(0, n - 1)
                };
                self.selected = Some(next as usize);
                self.page = next as usize / self.per_page();
            }
            Msg::TogglePosition => return Some(Event::TogglePosition),
        }
        None
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let (cw, ch) = self.cell_size();
        let icon_px = f32::from(self.icon_size);

        // ---- header: search + "move to panel / center" button
        let (move_icon, move_tip) = match self.mode {
            Position::Centered => ("view-dual-symbolic", "Show in the panel"),
            Position::Panel => ("view-fullscreen-symbolic", "Show centered on screen"),
        };
        let move_btn = tooltip(
            button::icon(icon::from_name(move_icon).size(16))
                .padding(8)
                .on_press(Msg::TogglePosition),
            text(move_tip),
            tooltip::Position::Bottom,
        );
        let header = row![
            search_input("Search applications…", &self.query)
                .on_input(Msg::Search)
                .on_paste(Msg::Search)
                .on_submit(|_| Msg::Submit)
                .id(SEARCH_ID.clone())
                .width(Length::Fill),
            move_btn,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(HEADER_H));

        // ---- grid page
        let start = self.page * self.per_page();
        let mut rows_col = Vec::with_capacity(self.rows);
        for r in 0..self.rows {
            let mut cells: Vec<Element<'_, Msg>> = Vec::with_capacity(self.columns);
            for c in 0..self.columns {
                let fi = start + r * self.columns + c;
                let cell: Element<'_, Msg> = match self.app_at(fi) {
                    Some(app) => self.cell(app, fi, cw, ch, icon_px),
                    None => widget::Space::new().width(Length::Fixed(cw)).height(Length::Fixed(ch)).into(),
                };
                cells.push(cell);
            }
            rows_col.push(row(cells).spacing(SPACING).into());
        }
        let page: Element<'_, Msg> = if self.filtered.is_empty() {
            container(text("No applications found").size(15))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into()
        } else {
            column(rows_col).spacing(SPACING).into()
        };
        let page = mouse_area(
            container(page)
                .width(Length::Fill)
                .height(Length::Fixed(self.rows as f32 * (ch + SPACING) - SPACING)),
        )
        .on_scroll(|delta| {
            use cosmic::iced::mouse::ScrollDelta;
            match delta {
                ScrollDelta::Lines { y, .. } => Msg::Scroll(y),
                ScrollDelta::Pixels { y, .. } => Msg::Scroll(y / 60.0),
            }
        });

        // ---- footer: page dots
        let pages = self.pages();
        let dots: Vec<Element<'_, Msg>> = (0..pages)
            .map(|p| {
                let active = p == self.page;
                button::custom(widget::Space::new().width(Length::Fixed(8.0)).height(Length::Fixed(8.0)))
                    .class(if active { theme::Button::Suggested } else { theme::Button::Standard })
                    .padding(0)
                    .on_press(Msg::SetPage(p))
                    .into()
            })
            .collect();
        let footer = container(row(dots).spacing(10).align_y(Alignment::Center))
            .center_x(Length::Fill)
            .center_y(Length::Fixed(FOOTER_H));

        column![header, page, footer]
            .padding(OUTER_PAD)
            .width(Length::Fill)
            .into()
    }

    fn cell<'a>(&'a self, app: &'a Arc<App>, fi: usize, cw: f32, ch: f32, icon_px: f32) -> Element<'a, Msg> {
        let fav = self.favorites.iter().any(|f| f == app.id());
        let label = if fav { format!("★ {}", app.name()) } else { app.name().to_string() };
        let content = column![
            app.icon.clone().icon().width(Length::Fixed(icon_px)).height(Length::Fixed(icon_px)),
            container(
                text(label)
                    .size(13)
                    .align_x(text::Alignment::Center)
                    .wrapping(text::Wrapping::WordOrGlyph)
            )
            .align_x(Horizontal::Center)
            .width(Length::Fill)
            .height(Length::Fixed(CELL_PAD_H - 16.0)),
        ]
        .spacing(4)
        .align_x(Alignment::Center);
        let btn = button::custom(content)
            .selected(self.selected == Some(fi))
            .class(theme::Button::IconVertical)
            .width(Length::Fixed(cw))
            .height(Length::Fixed(ch))
            .padding(6)
            .on_press(Msg::Launch(fi));
        mouse_area(btn).on_right_press(Msg::ToggleFavorite(fi)).into()
    }
}
