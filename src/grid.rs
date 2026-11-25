// SPDX-License-Identifier: GPL-3.0-only
//! The application grid: search, category chips, recently used apps and a
//! scrolling grid of all apps, with keyboard selection.
//!
//! Used unchanged by both the centered launcher and the panel applet popup;
//! each wraps [`Grid::view`] in its own surface and maps [`Msg`] into its own
//! message type.

// Pixel geometry and grid indices are small (screen sizes, a few hundred
// apps), so the numeric casts in this module cannot overflow in practice.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss
)]

use std::sync::{Arc, LazyLock};

use cosmic::iced::widget::scrollable::AbsoluteOffset;
use cosmic::iced::widget::{column, row, text};
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{self, button, container, divider, icon, popover, search_input};
use cosmic::{Element, Task, theme};

use crate::apps::{App, Category};
use crate::config::{AppGridConfig, Position};

pub static SEARCH_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("appgrid-search"));
static SCROLL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("appgrid-scroll"));

/// Extra width/height around the icon inside one cell (label + padding).
const CELL_PAD_W: f32 = 52.0;
const CELL_PAD_H: f32 = 44.0;
const SPACING: f32 = 4.0;
const OUTER_PAD: f32 = 16.0;
const HEADER_H: f32 = 40.0;
const CHIPS_H: f32 = 36.0;
const GAP: f32 = 8.0;
/// Height of the divider between the recent row and all apps.
const DIVIDER_H: f32 = 17.0;
/// Room on the right of the grid for the scrollbar.
const GUTTER: f32 = 14.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Search(String),
    Submit,
    /// Launch `apps[i]`.
    Launch(usize),
    Category(Category),
    Move(i32, i32),
    PageDown,
    PageUp,
    Scrolled(f32),
    ToggleMenu,
    CloseMenu,
    TogglePosition,
    ClearRecent,
    ToggleIcon,
}

/// What the owner of the grid needs to do after an update.
pub enum Event {
    Launch(Arc<App>),
    TogglePosition,
    ClearRecent,
    ToggleIcon,
    Task(Task<Msg>),
}

pub struct Grid {
    apps: Vec<Arc<App>>,
    query: String,
    category: Option<Category>,
    /// Indices into `apps` matching the query and category, best first.
    filtered: Vec<usize>,
    /// Indices into `apps` of the recently used apps (one row at most).
    recent: Vec<usize>,
    recent_ids: Vec<String>,
    /// Categories that have at least one app, in display order.
    categories: Vec<Category>,
    /// (row, column) in [`Grid::rows`].
    selected: Option<(usize, usize)>,
    columns: usize,
    visible_rows: usize,
    icon_size: u16,
    /// Which mode this grid is shown in (decides the menu wording).
    mode: Position,
    menu_open: bool,
    symbolic_icon: bool,
    scroll_y: f32,
}

impl Grid {
    #[must_use]
    pub fn new(mode: Position) -> Self {
        Self {
            apps: Vec::new(),
            query: String::new(),
            category: None,
            filtered: Vec::new(),
            recent: Vec::new(),
            recent_ids: Vec::new(),
            categories: Vec::new(),
            selected: None,
            columns: 5,
            visible_rows: 4,
            icon_size: 52,
            mode,
            menu_open: false,
            symbolic_icon: false,
            scroll_y: 0.0,
        }
    }

    pub fn apply_config(&mut self, config: &AppGridConfig) {
        self.columns = config.columns.clamp(3, 10) as usize;
        self.visible_rows = config.rows.clamp(2, 8) as usize;
        self.icon_size = config.icon_size.clamp(32, 128);
        self.symbolic_icon = config.symbolic_icon;
        if self.recent_ids != config.recent {
            self.recent_ids.clone_from(&config.recent);
            self.refresh_recent();
        }
    }

    /// Reload the application list and reset the view (called on open).
    pub fn reset(&mut self) {
        self.apps = crate::apps::load();
        self.categories = Category::ALL
            .into_iter()
            .filter(|c| self.apps.iter().any(|a| a.category == *c))
            .collect();
        self.query.clear();
        self.category = None;
        self.menu_open = false;
        self.scroll_y = 0.0;
        self.refresh_recent();
        self.refilter();
    }

    fn refresh_recent(&mut self) {
        self.recent = self
            .recent_ids
            .iter()
            .filter_map(|id| self.apps.iter().position(|a| a.id() == id))
            .take(self.columns)
            .collect();
    }

    /// Logical size of the grid panel, used to size/center the surface.
    #[must_use]
    pub fn panel_size(&self) -> (f32, f32) {
        let (cw, _) = self.cell_size();
        let w = 2.0 * OUTER_PAD + self.columns as f32 * (cw + SPACING) - SPACING + GUTTER;
        let h = 2.0 * OUTER_PAD + HEADER_H + GAP + CHIPS_H + GAP + self.viewport_h();
        (w, h)
    }

    fn cell_size(&self) -> (f32, f32) {
        let i = f32::from(self.icon_size);
        (i + CELL_PAD_W, i + CELL_PAD_H)
    }

    fn viewport_h(&self) -> f32 {
        let (_, ch) = self.cell_size();
        self.visible_rows as f32 * (ch + SPACING) - SPACING
    }

    fn show_recent(&self) -> bool {
        self.query.trim().is_empty() && self.category.is_none() && !self.recent.is_empty()
    }

    /// The displayed rows of app indices: the recent row (if shown), then
    /// all matching apps.
    fn rows(&self) -> Vec<&[usize]> {
        let mut rows = Vec::new();
        if self.show_recent() {
            rows.push(self.recent.as_slice());
        }
        rows.extend(self.filtered.chunks(self.columns));
        rows
    }

    /// Top of row `r` inside the scrolled content.
    fn row_y(&self, r: usize) -> f32 {
        let (_, ch) = self.cell_size();
        let step = ch + SPACING;
        if self.show_recent() && r > 0 {
            step + DIVIDER_H + (r - 1) as f32 * step
        } else {
            r as f32 * step
        }
    }

    fn refilter(&mut self) {
        let q = self.query.trim().to_lowercase();
        let mut scored: Vec<(u32, usize)> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| self.category.is_none_or(|c| a.category == c))
            .filter_map(|(i, a)| a.score(&q).map(|s| (s, i)))
            .collect();
        // stable sort keeps A–Z order within the same score
        scored.sort_by_key(|(s, _)| *s);
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = if q.is_empty() || self.filtered.is_empty() {
            None
        } else {
            Some((0, 0))
        };
    }

    fn app_at(&self, sel: (usize, usize)) -> Option<&Arc<App>> {
        let rows = self.rows();
        let i = *rows.get(sel.0)?.get(sel.1)?;
        self.apps.get(i)
    }

    fn scroll_to(&mut self, y: f32) -> Event {
        self.scroll_y = y.max(0.0);
        Event::Task(cosmic::iced::widget::scrollable::scroll_to(
            SCROLL_ID.clone(),
            AbsoluteOffset {
                x: None,
                y: Some(self.scroll_y),
            },
        ))
    }

    /// Scroll so the selected row is fully visible.
    fn reveal_selected(&mut self) -> Option<Event> {
        let (r, _) = self.selected?;
        let (_, ch) = self.cell_size();
        let top = self.row_y(r);
        let bottom = top + ch;
        if top < self.scroll_y {
            Some(self.scroll_to(top))
        } else if bottom > self.scroll_y + self.viewport_h() {
            Some(self.scroll_to(bottom - self.viewport_h()))
        } else {
            None
        }
    }

    fn move_selection(&mut self, dx: i32, dy: i32) -> Option<Event> {
        let lens: Vec<usize> = self.rows().iter().map(|r| r.len()).collect();
        if lens.is_empty() {
            return None;
        }
        let (mut r, mut c) = match self.selected {
            None => (0, 0),
            Some((r, c)) => {
                let (mut r, mut c) = (r as i32, c as i32 + dx);
                // left/right wrap onto the previous/next row
                if c < 0 && r > 0 {
                    r -= 1;
                    c = lens[r as usize] as i32 - 1;
                } else if c >= lens[r as usize] as i32 && (r as usize) + 1 < lens.len() {
                    r += 1;
                    c = 0;
                }
                r = (r + dy).clamp(0, lens.len() as i32 - 1);
                (r as usize, c.max(0) as usize)
            }
        };
        c = c.min(lens[r] - 1);
        r = r.min(lens.len() - 1);
        self.selected = Some((r, c));
        self.reveal_selected()
    }

    pub fn update(&mut self, msg: Msg) -> Option<Event> {
        match msg {
            Msg::Search(q) => {
                self.query = q;
                self.refilter();
                return Some(self.scroll_to(0.0));
            }
            Msg::Submit => {
                let sel = self.selected.unwrap_or((0, 0));
                return self.app_at(sel).map(|a| Event::Launch(a.clone()));
            }
            Msg::Launch(i) => return self.apps.get(i).map(|a| Event::Launch(a.clone())),
            Msg::Category(c) => {
                self.category = if self.category == Some(c) {
                    None
                } else {
                    Some(c)
                };
                self.refilter();
                return Some(self.scroll_to(0.0));
            }
            Msg::Move(dx, dy) => return self.move_selection(dx, dy),
            Msg::PageDown => return self.move_selection(0, self.visible_rows as i32),
            Msg::PageUp => return self.move_selection(0, -(self.visible_rows as i32)),
            Msg::Scrolled(y) => self.scroll_y = y,
            Msg::ToggleMenu => self.menu_open = !self.menu_open,
            Msg::CloseMenu => self.menu_open = false,
            Msg::TogglePosition => {
                self.menu_open = false;
                return Some(Event::TogglePosition);
            }
            Msg::ClearRecent => {
                self.menu_open = false;
                return Some(Event::ClearRecent);
            }
            Msg::ToggleIcon => {
                self.menu_open = false;
                return Some(Event::ToggleIcon);
            }
        }
        None
    }

    /// Search field and ⋯ menu.
    fn header(&self) -> Element<'_, Msg> {
        let menu_btn = button::icon(icon::from_name("view-more-horizontal-symbolic").size(16))
            .padding(8)
            .on_press(Msg::ToggleMenu);
        let mut menu = popover(menu_btn)
            .position(widget::popover::Position::Bottom)
            .on_close(Msg::CloseMenu);
        if self.menu_open {
            let move_label = match self.mode {
                Position::Centered => crate::fl!("attach-to-panel"),
                Position::Panel => crate::fl!("center-on-screen"),
            };
            let icon_label = if self.symbolic_icon {
                crate::fl!("colored-icon")
            } else {
                crate::fl!("monochrome-icon")
            };
            let item = |label: String, msg: Msg| {
                button::custom(text(label).size(13))
                    .class(theme::Button::MenuItem)
                    .padding([6, 12])
                    .width(Length::Fixed(180.0))
                    .on_press(msg)
            };
            menu = menu.popup(
                container(column![
                    item(move_label, Msg::TogglePosition),
                    item(icon_label, Msg::ToggleIcon),
                    item(crate::fl!("clear-recent"), Msg::ClearRecent),
                ])
                .padding(4)
                .class(theme::Container::Dropdown),
            );
        }
        let header = row![
            search_input(crate::fl!("search-placeholder"), &self.query)
                .on_input(Msg::Search)
                .on_paste(Msg::Search)
                .on_submit(|_| Msg::Submit)
                .id(SEARCH_ID.clone())
                .width(Length::Fill),
            menu,
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .height(Length::Fixed(HEADER_H));
        header.into()
    }

    /// Category filter chips (only categories that have apps).
    fn chips(&self) -> Element<'_, Msg> {
        let chips: Vec<Element<'_, Msg>> = self
            .categories
            .iter()
            .map(|&c| {
                button::custom(text(c.label()).size(12))
                    .class(if self.category == Some(c) {
                        theme::Button::Suggested
                    } else {
                        theme::Button::Standard
                    })
                    .padding([4, 12])
                    .on_press(Msg::Category(c))
                    .into()
            })
            .collect();
        let chips =
            cosmic::widget::scrollable::horizontal(row(chips).spacing(6).padding([0, 0, 8, 0]))
                .height(Length::Fixed(CHIPS_H));
        chips.into()
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let (cw, ch) = self.cell_size();
        let header = self.header();
        let chips = self.chips();

        // ---- recent row + all apps (scrolling)
        let rows = self.rows();
        let mut content: Vec<Element<'_, Msg>> = Vec::with_capacity(rows.len() + 1);
        for (r, apps) in rows.iter().enumerate() {
            let mut cells: Vec<Element<'_, Msg>> = apps
                .iter()
                .enumerate()
                .map(|(c, &i)| self.cell(i, (r, c), cw, ch))
                .collect();
            cells.extend((apps.len()..self.columns).map(|_| {
                widget::Space::new()
                    .width(Length::Fixed(cw))
                    .height(Length::Fixed(ch))
                    .into()
            }));
            content.push(row(cells).spacing(SPACING).into());
            if r == 0 && self.show_recent() {
                content.push(
                    container(divider::horizontal::default())
                        .center_y(Length::Fixed(DIVIDER_H - SPACING))
                        .into(),
                );
            }
        }
        let body: Element<'_, Msg> = if rows.is_empty() {
            container(text(crate::fl!("no-results")).size(14))
                .center_x(Length::Fill)
                .center_y(Length::Fixed(self.viewport_h()))
                .into()
        } else {
            cosmic::widget::scrollable(
                column(content)
                    .spacing(SPACING)
                    .padding([0.0, GUTTER, 0.0, 0.0]),
            )
            .id(SCROLL_ID.clone())
            .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
            .width(Length::Fill)
            .height(Length::Fixed(self.viewport_h()))
            .into()
        };

        column![header, chips, body]
            .spacing(GAP)
            .padding(OUTER_PAD)
            .width(Length::Fill)
            .into()
    }

    fn cell(&self, i: usize, pos: (usize, usize), cw: f32, ch: f32) -> Element<'_, Msg> {
        let app = &self.apps[i];
        let icon_px = f32::from(self.icon_size);
        // one line, cut with an ellipsis like the macOS launcher
        let max_chars = ((cw - 12.0) / 6.6) as usize;
        let name = app.name();
        let truncated = name.chars().count() > max_chars;
        let label = if truncated {
            format!(
                "{}…",
                name.chars()
                    .take(max_chars.saturating_sub(1))
                    .collect::<String>()
                    .trim_end()
            )
        } else {
            name.to_string()
        };
        let content = column![
            app.icon
                .clone()
                .icon()
                .width(Length::Fixed(icon_px))
                .height(Length::Fixed(icon_px)),
            text(label).size(12).wrapping(text::Wrapping::None),
        ]
        .spacing(6)
        .align_x(Alignment::Center);
        let btn = button::custom(container(content).center_x(Length::Fill))
            .selected(self.selected == Some(pos))
            .class(theme::Button::IconVertical)
            .width(Length::Fixed(cw))
            .height(Length::Fixed(ch))
            .padding([8, 4])
            .on_press(Msg::Launch(i));
        if truncated {
            // full name on hover when the label is cut
            widget::tooltip(
                btn,
                text(name.to_string()),
                widget::tooltip::Position::Bottom,
            )
            .into()
        } else {
            btn.into()
        }
    }
}
