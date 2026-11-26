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
use crate::config::{OumuamuaConfig, Position};
use crate::plugins::{self, Action, Item};

pub static SEARCH_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("oumuamua-search"));
static SCROLL_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("oumuamua-scroll"));

/// Extra width/height around the icon inside one cell (label + padding).
const CELL_PAD_W: f32 = 52.0;
const CELL_PAD_H: f32 = 44.0;
const SPACING: f32 = 4.0;
const OUTER_PAD: f32 = 16.0;
const HEADER_H: f32 = 40.0;
const CHIPS_H: f32 = 28.0;
const CHIP_SPACING: f32 = 6.0;
/// Horizontal padding inside a category chip, each side.
const CHIP_PAD: f32 = 12.0;
/// Text size of the category chips.
const CHIP_TEXT: f32 = 12.0;
const GAP: f32 = 8.0;
/// Height of the divider between the recent row and all apps.
const DIVIDER_H: f32 = 17.0;
/// Room on the right of the grid for the scrollbar.
const GUTTER: f32 = 14.0;
/// ⋯ menu text size, icon size, gap between them, and item padding.
const MENU_TEXT: f32 = 13.0;
const MENU_ICON: f32 = 16.0;
const MENU_ICON_GAP: f32 = 10.0;
const MENU_ITEM_PAD: f32 = 12.0;
/// Height of one search result row, its icon, and a section header.
const RESULT_H: f32 = 52.0;
const RESULT_ICON: u16 = 36;
const SECTION_H: f32 = 24.0;

#[derive(Debug, Clone)]
pub enum Msg {
    Search(String),
    Submit,
    /// Launch `apps[i]`.
    Launch(usize),
    /// Activate search result row `i` (click, or Alt+1…9).
    Activate(usize),
    /// Tab: complete the selected file path, or move on.
    Complete,
    /// The ⌫ button: empty the search field.
    Clear,
    /// Whether Alt is held (keyboard modifiers changed).
    AltHeld(bool),
    /// A background file search finished: (generation, results).
    Found(u64, Vec<Item>),
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
    ShowAbout(bool),
    OpenUrl(&'static str),
    Power(Power),
}

/// Session and power actions in the ⋯ menu, like the KDE launcher's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Lock,
    Suspend,
    LogOut,
    Restart,
    Shutdown,
}

impl Power {
    pub const ALL: [Power; 5] = [
        Power::Lock,
        Power::Suspend,
        Power::LogOut,
        Power::Restart,
        Power::Shutdown,
    ];

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Power::Lock => "system-lock-screen-symbolic",
            Power::Suspend => "system-suspend-symbolic",
            Power::LogOut => "system-log-out-symbolic",
            Power::Restart => "system-reboot-symbolic",
            Power::Shutdown => "system-shutdown-symbolic",
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Power::Lock => crate::fl!("lock"),
            Power::Suspend => crate::fl!("suspend"),
            Power::LogOut => crate::fl!("log-out"),
            Power::Restart => crate::fl!("restart"),
            Power::Shutdown => crate::fl!("shutdown"),
        }
    }

    /// Run the action. Log out, restart and shut down open COSMIC's own
    /// confirmation dialog (with its countdown), like the Power applet.
    pub fn run(self) {
        let (cmd, args): (&str, &[&str]) = match self {
            Power::Lock => ("loginctl", &["lock-session"]),
            Power::Suspend => ("systemctl", &["suspend"]),
            Power::LogOut => ("cosmic-osd", &["log-out"]),
            Power::Restart => ("cosmic-osd", &["restart"]),
            Power::Shutdown => ("cosmic-osd", &["shutdown"]),
        };
        match std::process::Command::new(cmd).args(args).spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || child.wait());
            }
            Err(e) => log::error!("failed to run {cmd}: {e}"),
        }
    }
}

/// What the owner of the grid needs to do after an update.
pub enum Event {
    Launch(Arc<App>),
    /// A plugin result was chosen (copy, open, run, power).
    Action(Action),
    TogglePosition,
    ClearRecent,
    ToggleIcon,
    OpenUrl(&'static str),
    Power(Power),
    Task(Task<Msg>),
}

/// One row of the search results list.
#[derive(Debug, Clone, Copy)]
enum Hit {
    /// `apps[i]`
    App(usize),
    /// `results[i]`
    Plugin(usize),
}

// the bools are independent UI flags (menu, About page, icon, Alt key)
#[allow(clippy::struct_excessive_bools)]
pub struct Grid {
    apps: Vec<Arc<App>>,
    query: String,
    category: Option<Category>,
    /// Indices into `apps` matching the query and category, best first.
    filtered: Vec<usize>,
    /// Plugin results, listed after the matching apps while searching.
    results: Vec<Item>,
    /// The query used a plugin prefix: only `results` are shown.
    isolate: bool,
    /// Alt is held: a digit typed now is an Alt+N shortcut, not text.
    alt_held: bool,
    /// Generation of the file search running for the current query, if any.
    finding: Option<u64>,
    /// Search term `refilter` found, waiting for [`Grid::find_task`].
    pending_find: Option<String>,
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
    /// Measured width of the chips row (all categories on one line).
    chips_w: f32,
    menu_open: bool,
    /// Showing the About page instead of the apps.
    about: bool,
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
            results: Vec::new(),
            isolate: false,
            alt_held: false,
            finding: None,
            pending_find: None,
            recent: Vec::new(),
            recent_ids: Vec::new(),
            categories: Vec::new(),
            selected: None,
            columns: 5,
            visible_rows: 4,
            icon_size: 52,
            mode,
            chips_w: 0.0,
            menu_open: false,
            about: false,
            symbolic_icon: false,
            scroll_y: 0.0,
        }
    }

    pub fn apply_config(&mut self, config: &OumuamuaConfig) {
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
        self.chips_w = self.measure_chips();
        self.query.clear();
        self.category = None;
        self.finding = None;
        self.menu_open = false;
        self.about = false;
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
        // the grid already widens its cells to fit all category chips
        let w = 2.0 * OUTER_PAD + self.grid_width();
        let h = 2.0 * OUTER_PAD + HEADER_H + GAP + CHIPS_H + GAP + self.viewport_h();
        (w, h)
    }

    /// Width of the app grid (all columns plus the scrollbar gutter).
    fn grid_width(&self) -> f32 {
        let (cw, _) = self.cell_size();
        self.columns as f32 * (cw + SPACING) - SPACING + GUTTER
    }

    /// Width of the chips row with every category on one line, measured
    /// with the real font so the panel grows only as much as a language
    /// needs (and not at all when the chips already fit).
    fn measure_chips(&self) -> f32 {
        let chips: f32 = self
            .categories
            .iter()
            // +2: rounding room so the last chip never clips
            .map(|c| text_width(&c.label(), CHIP_TEXT) + 2.0 * CHIP_PAD + 2.0)
            .sum();
        chips + CHIP_SPACING * self.categories.len().saturating_sub(1) as f32
    }

    /// Size of one app cell. Cells get wider when a language's category
    /// names need more room than the grid, so the grid always fills the
    /// panel edge to edge (no empty strips beside it).
    fn cell_size(&self) -> (f32, f32) {
        let i = f32::from(self.icon_size);
        let n = self.columns as f32;
        let fit_chips = (self.chips_w - GUTTER + SPACING) / n - SPACING;
        ((i + CELL_PAD_W).max(fit_chips.ceil()), i + CELL_PAD_H)
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

    /// Typing switches the grid to a results list, like Plasma's `AppGrid`:
    /// the category chips hide and apps and plugin results are listed in
    /// sections.
    fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    /// Rows of the results list: matching apps, then plugin results.
    fn hits(&self) -> Vec<Hit> {
        self.filtered
            .iter()
            .map(|&i| Hit::App(i))
            .chain((0..self.results.len()).map(Hit::Plugin))
            .collect()
    }

    /// Height of the scrolling area below the header.
    fn body_h(&self) -> f32 {
        if self.searching() {
            // the chips row is hidden, the list takes its room
            CHIPS_H + GAP + self.viewport_h()
        } else {
            self.viewport_h()
        }
    }

    /// Length of every selectable row.
    fn row_lens(&self) -> Vec<usize> {
        if self.searching() {
            vec![1; self.filtered.len() + self.results.len()]
        } else {
            self.rows().iter().map(|r| r.len()).collect()
        }
    }

    /// Top of selectable row `r` inside the scrolled content.
    fn row_y(&self, r: usize) -> f32 {
        if self.searching() {
            let step = RESULT_H + SPACING;
            let header = SECTION_H + SPACING;
            let apps = self.filtered.len();
            let mut y = 0.0;
            if apps > 0 {
                y += header;
            }
            if r < apps {
                return y + r as f32 * step;
            }
            y += apps as f32 * step + header;
            return y + (r - apps) as f32 * step;
        }
        let (_, ch) = self.cell_size();
        let step = ch + SPACING;
        if self.show_recent() && r > 0 {
            step + DIVIDER_H + (r - 1) as f32 * step
        } else {
            r as f32 * step
        }
    }

    fn row_h(&self) -> f32 {
        if self.searching() {
            RESULT_H
        } else {
            self.cell_size().1
        }
    }

    fn refilter(&mut self) {
        let found = plugins::search(&self.query);
        self.results = found.items;
        self.isolate = found.isolate;
        self.pending_find = found.find;
        let q = self.query.trim().to_lowercase();
        if self.isolate {
            self.filtered.clear();
            self.selected = (!self.results.is_empty()).then_some((0, 0));
            return;
        }
        // a search looks through every app; the chips filter only the grid
        let category = if q.is_empty() { self.category } else { None };
        let mut scored: Vec<(u32, usize)> = self
            .apps
            .iter()
            .enumerate()
            .filter(|(_, a)| category.is_none_or(|c| a.category == c))
            .filter_map(|(i, a)| a.score(&q).map(|s| (s, i)))
            .collect();
        // stable sort keeps A–Z order within the same score
        scored.sort_by_key(|(s, _)| *s);
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = if q.is_empty() || (self.filtered.is_empty() && self.results.is_empty()) {
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

    fn scroll_task(&mut self, y: f32) -> Task<Msg> {
        self.scroll_y = y.max(0.0);
        cosmic::iced::widget::scrollable::scroll_to(
            SCROLL_ID.clone(),
            AbsoluteOffset {
                x: None,
                y: Some(self.scroll_y),
            },
        )
    }

    fn scroll_to(&mut self, y: f32) -> Event {
        Event::Task(self.scroll_task(y))
    }

    /// Replace the search text from code (Tab completion, the `?` list),
    /// keeping the cursor at the end.
    fn set_query(&mut self, q: String) -> Event {
        self.query = q;
        self.refilter();
        // focus first: Tab can leave the field unfocused, and typing would
        // then go nowhere
        Event::Task(Task::batch([
            self.scroll_task(0.0),
            widget::text_input::focus(SEARCH_ID.clone()),
            widget::text_input::move_cursor_to_end(SEARCH_ID.clone()),
            self.find_task(),
        ]))
    }

    /// Start the file search the query asked for (`find …`), cancelling any
    /// older one; its results arrive as [`Msg::Found`].
    fn find_task(&mut self) -> Task<Msg> {
        let generation = plugins::find::next_generation();
        let Some(term) = self.pending_find.take() else {
            self.finding = None;
            return Task::none();
        };
        self.finding = Some(generation);
        Task::perform(plugins::find::search(term, generation), move |items| {
            Msg::Found(generation, items.unwrap_or_default())
        })
    }

    /// Run results-list row `row`: launch the app or do the plugin action.
    fn activate(&mut self, row: usize) -> Option<Event> {
        match *self.hits().get(row)? {
            Hit::App(i) => self.apps.get(i).map(|a| Event::Launch(a.clone())),
            Hit::Plugin(i) => match &self.results.get(i)?.action {
                Action::Fill(q) => Some(self.set_query(q.clone())),
                action => Some(Event::Action(action.clone())),
            },
        }
    }

    /// Scroll so the selected row is fully visible.
    fn reveal_selected(&mut self) -> Option<Event> {
        let (r, _) = self.selected?;
        let top = self.row_y(r);
        let bottom = top + self.row_h();
        if top < self.scroll_y {
            Some(self.scroll_to(top))
        } else if bottom > self.scroll_y + self.body_h() {
            Some(self.scroll_to(bottom - self.body_h()))
        } else {
            None
        }
    }

    fn move_selection(&mut self, dx: i32, dy: i32) -> Option<Event> {
        let lens = self.row_lens();
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
                // The field also receives Alt+1…9 and would type the digit
                // before the shortcut runs; keep the text as it was. (AltGr
                // characters such as @ or ² are not ASCII digits.)
                if self.alt_held
                    && let Some(added) = q.strip_prefix(self.query.as_str())
                    && added.len() == 1
                    && added.chars().all(|c| matches!(c, '1'..='9'))
                {
                    return None;
                }
                self.query = q;
                self.refilter();
                let find = self.find_task();
                return Some(Event::Task(Task::batch([self.scroll_task(0.0), find])));
            }
            Msg::Submit => {
                let sel = self.selected.unwrap_or((0, 0));
                if self.searching() {
                    return self.activate(sel.0);
                }
                return self.app_at(sel).map(|a| Event::Launch(a.clone()));
            }
            Msg::Launch(i) => return self.apps.get(i).map(|a| Event::Launch(a.clone())),
            Msg::Activate(row) if self.searching() => return self.activate(row),
            Msg::Activate(_) => {}
            Msg::AltHeld(held) => self.alt_held = held,
            Msg::Found(generation, items) => {
                // ignore a search the query has moved on from
                if self.finding == Some(generation) {
                    self.finding = None;
                    self.results = items;
                    self.selected = (!self.results.is_empty()).then_some((0, 0));
                }
            }
            Msg::Complete => {
                if self.searching() {
                    let row = self.selected.map_or(0, |(r, _)| r);
                    if let Some(Hit::Plugin(i)) = self.hits().get(row)
                        && let Some(path) = self.results[*i].complete.clone()
                    {
                        return Some(self.set_query(path));
                    }
                    return self.move_selection(0, 1);
                }
                return self.move_selection(1, 0);
            }
            Msg::Clear => return Some(self.set_query(String::new())),
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
            Msg::ShowAbout(show) => {
                self.menu_open = false;
                self.about = show;
            }
            Msg::OpenUrl(url) => return Some(Event::OpenUrl(url)),
            Msg::Power(p) => return Some(Event::Power(p)),
        }
        None
    }

    /// Search field with the ⋮ menu inside its right end.
    fn header(&self) -> Element<'_, Msg> {
        let menu_btn = button::icon(icon::from_name("view-more-symbolic").size(16))
            .padding(6)
            .on_press(Msg::ToggleMenu);
        let mut menu = popover(menu_btn)
            .position(widget::popover::Position::Bottom)
            .on_close(Msg::CloseMenu);
        if self.menu_open {
            let (move_icon, move_label) = match self.mode {
                Position::Centered => ("pin-symbolic", crate::fl!("attach-to-panel")),
                Position::Panel => ("zoom-fit-best-symbolic", crate::fl!("center-on-screen")),
            };
            let icon_label = if self.symbolic_icon {
                crate::fl!("original-icon")
            } else {
                crate::fl!("theme-icon")
            };
            // every entry has an icon so all labels line up
            let mut entries: Vec<(&'static str, String, Msg)> = vec![
                (move_icon, move_label, Msg::TogglePosition),
                ("image-x-generic-symbolic", icon_label, Msg::ToggleIcon),
                (
                    "edit-clear-symbolic",
                    crate::fl!("clear-recent"),
                    Msg::ClearRecent,
                ),
            ];
            entries.extend(Power::ALL.map(|p| (p.icon(), p.label(), Msg::Power(p))));
            entries.push((
                "help-about-symbolic",
                crate::fl!("about"),
                Msg::ShowAbout(true),
            ));

            // as wide as the longest label in the current language, so
            // nothing wraps; the dividers would otherwise stretch the menu
            // across the whole full-screen surface
            let longest = entries
                .iter()
                .map(|(_, label, _)| text_width(label, MENU_TEXT))
                .fold(0.0, f32::max);
            let width =
                longest + MENU_ICON + MENU_ICON_GAP + 2.0 * MENU_ITEM_PAD + 2.0 * 4.0 + 12.0;

            let mut items: Vec<Element<'_, Msg>> = Vec::with_capacity(entries.len() + 2);
            for (i, (icon_name, label, msg)) in entries.into_iter().enumerate() {
                // dividers before the power actions and before About
                if i == 3 || i == 3 + Power::ALL.len() {
                    items.push(divider::horizontal::light().into());
                }
                items.push(
                    button::custom(
                        row![
                            icon::from_name(icon_name).size(16).icon(),
                            text(label).size(MENU_TEXT).wrapping(text::Wrapping::None),
                        ]
                        .spacing(MENU_ICON_GAP)
                        .align_y(Alignment::Center),
                    )
                    .class(theme::Button::MenuItem)
                    .padding([6.0, MENU_ITEM_PAD])
                    .width(Length::Fill)
                    .on_press(msg)
                    .into(),
                );
            }
            menu = menu.popup(
                container(column(items))
                    .width(Length::Fixed(width.ceil()))
                    .padding(4)
                    .class(theme::Container::Dropdown),
            );
        }
        // The menu sits in the field's trailing slot so it looks part of it.
        // While searching, like Plasma's AppGrid, that slot holds a clear
        // button and the icon of the selected result instead.
        let trailing: Element<'_, Msg> = if self.searching() {
            let clear = widget::tooltip(
                button::icon(icon::from_name("edit-clear-symbolic").size(16))
                    .padding(6)
                    .on_press(Msg::Clear),
                text(crate::fl!("clear-search")),
                widget::tooltip::Position::Bottom,
            );
            let mut slot = row![clear].spacing(6).align_y(Alignment::Center);
            if let Some(hit) = self.selected.and_then(|(r, _)| self.hits().get(r).copied()) {
                slot = slot.push(self.hit_icon(hit, 24));
            }
            container(slot).padding([0, 6, 0, 2]).into()
        } else {
            container(menu).padding([0, 2]).into()
        };
        let header = row![
            search_input(crate::fl!("search-placeholder"), &self.query)
                .on_input(Msg::Search)
                .on_paste(Msg::Search)
                .on_submit(|_| Msg::Submit)
                .id(SEARCH_ID.clone())
                .trailing_icon(trailing)
                .width(Length::Fill),
        ]
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
                button::custom(text(c.label()).size(CHIP_TEXT))
                    .class(if self.category == Some(c) {
                        theme::Button::Suggested
                    } else {
                        theme::Button::Standard
                    })
                    .padding([4.0, CHIP_PAD])
                    .on_press(Msg::Category(c))
                    .into()
            })
            .collect();
        // no scrolling: the panel is sized so the chips always fit
        container(row(chips).spacing(CHIP_SPACING))
            .center_x(Length::Fill)
            .height(Length::Fixed(CHIPS_H))
            .into()
    }

    /// About page: shown in place of the chips and apps, at the same size.
    fn about_view(&self) -> Element<'_, Msg> {
        let back = button::icon(icon::from_name("go-previous-symbolic").size(16))
            .padding(8)
            .on_press(Msg::ShowAbout(false));
        let header = row![back, text(crate::fl!("about")).size(16)]
            .spacing(8)
            .align_y(Alignment::Center)
            .height(Length::Fixed(HEADER_H));

        let small = |s: String| {
            text(s)
                .size(12)
                .align_x(Alignment::Center)
                .width(Length::Fill)
        };
        let links = row![
            button::suggested(crate::fl!("source-code")).on_press(Msg::OpenUrl(crate::REPOSITORY)),
            button::standard(crate::fl!("report-issue")).on_press(Msg::OpenUrl(crate::ISSUES)),
        ]
        .spacing(8);
        let content = column![
            icon::icon(crate::launcher_icon(false))
                .width(Length::Fixed(72.0))
                .height(Length::Fixed(72.0)),
            text("Oumuamua").size(22),
            small(format!("{} {}", crate::fl!("version"), crate::VERSION)),
            small(crate::fl!("app-comment")),
            links,
            small(crate::fl!("license")),
            small(crate::fl!("copyright")),
            column![
                small(crate::fl!("community-notice")),
                small(crate::fl!("trademark-notice")),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .width(Length::Fill);
        let body = cosmic::widget::scrollable(container(content).center_x(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fixed(CHIPS_H + GAP + self.viewport_h()));

        column![header, body]
            .spacing(GAP)
            .padding(OUTER_PAD)
            .width(Length::Fill)
            .into()
    }

    pub fn view(&self) -> Element<'_, Msg> {
        if self.about {
            return self.about_view();
        }
        let header = self.header();
        if self.searching() {
            return column![header, self.search_list()]
                .spacing(GAP)
                .padding(OUTER_PAD)
                .width(Length::Fill)
                .into();
        }
        let (cw, ch) = self.cell_size();
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
            let grid = cosmic::widget::scrollable(
                column(content)
                    .spacing(SPACING)
                    .padding([0.0, GUTTER, 0.0, 0.0]),
            )
            .id(SCROLL_ID.clone())
            .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
            .width(Length::Fixed(self.grid_width()))
            .height(Length::Fixed(self.viewport_h()));
            grid.into()
        };

        column![header, chips, body]
            .spacing(GAP)
            .padding(OUTER_PAD)
            .width(Length::Fill)
            .into()
    }

    /// The results list shown while typing, in place of the chips and the
    /// grid: "Applications (n)" then "Search Plugins (n)", each row with an
    /// Alt+N shortcut badge for the first nine.
    fn search_list(&self) -> Element<'_, Msg> {
        let hits = self.hits();
        if hits.is_empty() {
            let dim = dim_color();
            let empty = column![
                icon::from_name("system-search-symbolic")
                    .size(48)
                    .icon()
                    .class(theme::Svg::Custom(std::rc::Rc::new(move |_| {
                        cosmic::iced::widget::svg::Style { color: Some(dim) }
                    }))),
                text(if self.finding.is_some() {
                    crate::fl!("searching")
                } else {
                    crate::fl!("no-results-for", query = self.query.trim())
                })
                    .size(14)
                    .class(theme::Text::Color(dim)),
            ]
            .spacing(12)
            .align_x(Alignment::Center);
            return container(empty)
                .center_x(Length::Fill)
                .center_y(Length::Fixed(self.body_h()))
                .into();
        }

        let apps = self.filtered.len();
        let plugins = self.results.len();
        let mut content: Vec<Element<'_, Msg>> = Vec::with_capacity(hits.len() + 2);
        for (row, hit) in hits.iter().enumerate() {
            if row == 0 && apps > 0 {
                content.push(section_header(crate::fl!("section-apps", count = apps)));
            }
            if row == apps && plugins > 0 {
                content.push(section_header(crate::fl!("section-plugins", count = plugins)));
            }
            content.push(self.hit_row(row, *hit));
        }
        cosmic::widget::scrollable(
            column(content)
                .spacing(SPACING)
                .padding([0.0, GUTTER, 0.0, 0.0]),
        )
        .id(SCROLL_ID.clone())
        .on_scroll(|v| Msg::Scrolled(v.absolute_offset().y))
        .width(Length::Fixed(self.grid_width()))
        .height(Length::Fixed(self.body_h()))
        .into()
    }

    /// Icon of results-list row `row`, at `size` pixels.
    fn hit_icon(&self, hit: Hit, size: u16) -> Element<'_, Msg> {
        let px = Length::Fixed(f32::from(size));
        match hit {
            Hit::App(i) => self.apps[i].icon.clone().icon().width(px).height(px).into(),
            Hit::Plugin(i) => {
                let item = &self.results[i];
                let mut named = icon::from_name(item.icons[0].as_str()).size(size);
                if item.icons.len() > 1 {
                    named.fallback = Some(icon::IconFallback::Names(
                        item.icons[1..].iter().map(|n| n.clone().into()).collect(),
                    ));
                }
                named.icon().width(px).height(px).into()
            }
        }
    }

    /// One results-list row: Alt+N badge, icon, name, a dimmer second
    /// line, and for plugin results a pill naming the plugin.
    fn hit_row(&self, row: usize, hit: Hit) -> Element<'_, Msg> {
        use cosmic::iced::advanced::text::{Ellipsize, EllipsizeHeightLimit};
        let one_line = Ellipsize::End(EllipsizeHeightLimit::Lines(1));
        let (title, subtitle, source): (&str, String, Option<&str>) = match hit {
            Hit::App(i) => (self.apps[i].name(), self.apps[i].category.label(), None),
            Hit::Plugin(i) => {
                let item = &self.results[i];
                (
                    item.title.as_str(),
                    item.subtitle.clone(),
                    Some(item.source.as_str()),
                )
            }
        };

        let mut content = row![].spacing(12).align_y(Alignment::Center);
        // the first nine rows answer to Alt+1…9
        content = content.push(
            container(if row < 9 {
                Element::from(badge(format!("Alt+{}", row + 1)))
            } else {
                widget::Space::new().into()
            })
            .width(Length::Fixed(44.0)),
        );
        content = content.push(self.hit_icon(hit, RESULT_ICON));
        let mut lines = column![text(title).size(14).ellipsize(one_line)]
            .spacing(2)
            .width(Length::Fill);
        if !subtitle.is_empty() {
            lines = lines.push(
                text(subtitle)
                    .size(11)
                    .ellipsize(one_line)
                    .class(theme::Text::Color(dim_color())),
            );
        }
        content = content.push(lines);
        if let Some(source) = source.filter(|s| !s.is_empty()) {
            content = content.push(pill(source));
        }

        button::custom(content)
            .selected(self.selected == Some((row, 0)))
            // same style as the app cells, so the selection looks the same
            .class(theme::Button::IconVertical)
            .padding([6, 10])
            .width(Length::Fill)
            .height(Length::Fixed(RESULT_H))
            .on_press(Msg::Activate(row))
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

/// The theme's text colour at 70% opacity, for secondary text.
fn dim_color() -> cosmic::iced::Color {
    let t = theme::active();
    let mut c: cosmic::iced::Color = t.cosmic().background(t.transparent).on.into();
    c.a *= 0.7;
    c
}

/// "Applications (3)" with a line running to the right edge.
fn section_header<'a>(label: String) -> Element<'a, Msg> {
    row![
        text(label).size(12).class(theme::Text::Color(dim_color())),
        divider::horizontal::light(),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .height(Length::Fixed(SECTION_H))
    .into()
}

/// A small rounded tag with a faint fill and outline.
fn tag<'a>(label: String, bold: bool) -> container::Container<'a, Msg, cosmic::Theme> {
    let mut t = text(label).size(10).class(theme::Text::Color(dim_color()));
    if bold {
        t = t.font(cosmic::font::bold());
    }
    container(t)
        .padding([2, 6])
        .class(theme::Container::custom(|theme| {
            let c = theme.cosmic();
            let on: cosmic::iced::Color = c.background(theme.transparent).on.into();
            container::Style {
                background: Some(cosmic::iced::Background::Color(cosmic::iced::Color {
                    a: 0.08,
                    ..on
                })),
                border: cosmic::iced::Border {
                    color: cosmic::iced::Color { a: 0.2, ..on },
                    width: 1.0,
                    radius: c.corner_radii.radius_s.into(),
                },
                ..Default::default()
            }
        }))
}

/// Alt+N shortcut badge.
fn badge<'a>(label: String) -> container::Container<'a, Msg, cosmic::Theme> {
    tag(label, true)
}

/// Plugin name pill ("Calculator", "Files"…).
fn pill(label: &str) -> container::Container<'_, Msg, cosmic::Theme> {
    tag(label.to_string(), false)
}

/// Width of one line of `s` in the theme's font at `size` logical pixels.
fn text_width(s: &str, size: f32) -> f32 {
    use cosmic::iced::Pixels;
    use cosmic::iced::advanced::graphics::text::Paragraph;
    use cosmic::iced::advanced::text::{
        self as core_text, Ellipsize, LineHeight, Paragraph as _, Shaping, Wrapping,
    };
    let p = Paragraph::with_text(core_text::Text {
        content: s,
        bounds: cosmic::iced::Size::INFINITE,
        size: Pixels(size),
        line_height: LineHeight::default(),
        font: cosmic::font::default(),
        align_x: core_text::Alignment::Default,
        align_y: cosmic::iced::alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
        ellipsize: Ellipsize::None,
    });
    p.min_bounds().width
}
