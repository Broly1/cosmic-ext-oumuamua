// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-appgrid-applet`: the panel button.
//!
//! * Position::Centered (default): clicking runs `cosmic-appgrid`, which
//!   toggles the centered grid (single instance over D-Bus).
//! * Position::Panel: clicking opens the same grid as a popup attached to the
//!   panel button, like the Windows 11 Start menu. As an applet popup it
//!   follows the user's Frosted glass "Applets" setting.

use std::sync::Arc;

use appgrid::apps::App;
use appgrid::config::{AppGridConfig, Position};
use appgrid::grid::{self, Grid, SEARCH_ID};
use cosmic::app::{Core, Task};
use cosmic::cosmic_config;
use cosmic::iced::core::keyboard::{Key, key::Named};
use cosmic::iced::event::listen_with;
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Limits, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{container, text_input};
use cosmic::Element;

const ICON: &str = "view-app-grid-symbolic";

#[derive(Debug, Clone)]
enum Message {
    Pressed(Rectangle),
    PopupClosed(Id),
    Grid(grid::Msg),
    Close,
    Config(AppGridConfig),
    Surface(cosmic::surface::Action<Message>),
}

struct Applet {
    core: Core,
    helper: Option<cosmic_config::Config>,
    config: AppGridConfig,
    grid: Grid,
    popup: Option<Id>,
}

impl Applet {
    fn close_popup(&mut self) -> Task<Message> {
        match self.popup.take() {
            Some(id) => cosmic::task::message(cosmic::Action::Surface(destroy_popup(id))),
            None => Task::none(),
        }
    }

    fn open_popup(&mut self, anchor: Rectangle) -> Task<Message> {
        self.grid.reset();
        let (w, h) = self.grid.panel_size();
        let open = cosmic::task::message(cosmic::Action::Surface(app_popup::<Applet>(
            // default live settings: libcosmic applies the theme's applet
            // corners and the user's frosted "Applets" blur to the popup
            |_: &Applet| Default::default(),
            move |state: &mut Applet| {
                let id = Id::unique();
                state.popup = Some(id);
                let mut s = state
                    .core
                    .applet
                    .get_popup_settings(state.core.main_window_id().unwrap(), id, None, None, None);
                s.positioner.anchor_rect = Rectangle {
                    x: anchor.x as i32,
                    y: anchor.y as i32,
                    width: anchor.width as i32,
                    height: anchor.height as i32,
                };
                s.positioner.size_limits = Limits::NONE.min_width(w).max_width(w).min_height(h).max_height(h);
                s
            },
            Some(Box::new(|state: &Applet| {
                let content = container(state.grid.view().map(Message::Grid))
                    .width(Length::Fill)
                    .height(Length::Fill);
                Element::from(state.core.applet.popup_container(content)).map(cosmic::Action::App)
            })),
        )));
        Task::batch([open, text_input::focus(SEARCH_ID.clone())])
    }

    fn launch(&mut self, app: Arc<App>) -> Task<Message> {
        tokio::spawn(appgrid::apps::launch(app, None));
        self.close_popup()
    }
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.broly1.CosmicAppGridApplet";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let (helper, config) = AppGridConfig::load();
        let mut grid = Grid::new(Position::Panel);
        grid.apply_config(&config);
        (Self { core, helper, config, grid, popup: None }, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Pressed(anchor) => {
                if self.popup.is_some() {
                    return self.close_popup();
                }
                match self.config.position {
                    Position::Centered => {
                        if let Err(e) = std::process::Command::new("cosmic-appgrid").spawn() {
                            log::error!("failed to start cosmic-appgrid: {e}");
                        }
                    }
                    Position::Panel => return self.open_popup(anchor),
                }
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                }
            }
            Message::Close => return self.close_popup(),
            Message::Grid(msg) => match self.grid.update(msg) {
                Some(grid::Event::Launch(app)) => return self.launch(app),
                Some(grid::Event::ToggleFavorite(id)) => {
                    self.config.toggle_favorite(self.helper.as_ref(), &id);
                    self.grid.apply_config(&self.config);
                }
                Some(grid::Event::TogglePosition) => {
                    self.config.save_position(self.helper.as_ref(), Position::Centered);
                    let close = self.close_popup();
                    // open the centered grid right away so the switch feels instant
                    let _ = std::process::Command::new("cosmic-appgrid").spawn();
                    return close;
                }
                None => {}
            },
            Message::Config(config) => {
                self.config = config;
                self.grid.apply_config(&self.config);
            }
            Message::Surface(a) => return cosmic::task::message(cosmic::Action::Surface(a)),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let btn = self
            .core
            .applet
            .icon_button(ICON)
            .on_press_with_rectangle(|offset, bounds| {
                Message::Pressed(Rectangle {
                    x: bounds.x - offset.x,
                    y: bounds.y - offset.y,
                    width: bounds.width,
                    height: bounds.height,
                })
            });
        Element::from(self.core.applet.applet_tooltip::<Message>(
            btn,
            "Applications",
            self.popup.is_some(),
            Message::Surface,
            None,
        ))
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        "".into()
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            self.core
                .watch_config::<AppGridConfig>(appgrid::config::APP_ID)
                .map(|u| Message::Config(u.config)),
        ];
        if self.popup.is_some() {
            subs.push(listen_with(|event, status, _| match event {
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed { key, .. }) => {
                    let ignored = matches!(status, cosmic::iced::event::Status::Ignored);
                    match key {
                        Key::Named(Named::Escape) => Some(Message::Close),
                        Key::Named(Named::ArrowUp) => Some(Message::Grid(grid::Msg::Move(0, -1))),
                        Key::Named(Named::ArrowDown) => Some(Message::Grid(grid::Msg::Move(0, 1))),
                        Key::Named(Named::ArrowLeft) if ignored => Some(Message::Grid(grid::Msg::Move(-1, 0))),
                        Key::Named(Named::ArrowRight) if ignored => Some(Message::Grid(grid::Msg::Move(1, 0))),
                        Key::Named(Named::PageDown) => Some(Message::Grid(grid::Msg::NextPage)),
                        Key::Named(Named::PageUp) => Some(Message::Grid(grid::Msg::PrevPage)),
                        _ => None,
                    }
                }
                _ => None,
            }));
        }
        Subscription::batch(subs)
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    cosmic::applet::run::<Applet>(())
}
