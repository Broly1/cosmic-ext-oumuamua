// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-ext-oumuamua-applet`: the panel button.
//!
//! * `Position::Centered` (default): clicking runs `cosmic-ext-oumuamua`, which
//!   toggles the centered grid (single instance over D-Bus).
//! * `Position::Panel`: clicking opens the same grid as a popup attached to the
//!   panel button, like the Windows 11 Start menu. As an applet popup it
//!   follows the user's Frosted glass "Applets" setting.

use std::collections::HashMap;
use std::sync::Arc;

use oumuamua::apps::App;
use oumuamua::config::{OumuamuaConfig, Position};
use oumuamua::grid::{self, Grid, SEARCH_ID};
use cosmic::Element;
use cosmic::app::{Core, Task};
use cosmic::applet::token::subscription::{
    TokenRequest, TokenUpdate, activation_token_subscription,
};
use cosmic::cctk::sctk::reexports::calloop;
use cosmic::cosmic_config;
use cosmic::iced::core::keyboard::{Key, key::Named};
use cosmic::iced::event::listen_with;
use cosmic::iced::window::Id;
use cosmic::iced::{Length, Limits, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget::{container, text_input};

/// Run `cosmic-ext-oumuamua` (toggles the centered grid) and reap it in the
/// background so no zombie processes pile up in the panel applet.
fn toggle_centered_grid() {
    match std::process::Command::new("cosmic-ext-oumuamua").spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::error!("failed to start cosmic-ext-oumuamua: {e}"),
    }
}

#[derive(Debug, Clone)]
enum Message {
    Pressed(Rectangle),
    PopupClosed(Id),
    Grid(grid::Msg),
    Close,
    Config(OumuamuaConfig),
    Surface(cosmic::surface::Action<Message>),
    Token(TokenUpdate),
}

struct Applet {
    core: Core,
    helper: Option<cosmic_config::Config>,
    config: OumuamuaConfig,
    grid: Grid,
    popup: Option<Id>,
    /// Requests Wayland activation tokens so launched apps get focus.
    token_tx: Option<calloop::channel::Sender<TokenRequest>>,
    /// Apps waiting for their activation token, by desktop-entry ID.
    pending_launch: HashMap<String, Arc<App>>,
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
            |_: &Applet| cosmic::surface::action::LiveSettings::default(),
            move |state: &mut Applet| {
                let id = Id::unique();
                state.popup = Some(id);
                let mut s = state.core.applet.get_popup_settings(
                    state.core.main_window_id().unwrap(),
                    id,
                    None,
                    None,
                    None,
                );
                // logical pixel coordinates of the panel button
                #[allow(clippy::cast_possible_truncation)]
                let anchor_rect = Rectangle {
                    x: anchor.x as i32,
                    y: anchor.y as i32,
                    width: anchor.width as i32,
                    height: anchor.height as i32,
                };
                s.positioner.anchor_rect = anchor_rect;
                s.positioner.size_limits = Limits::NONE
                    .min_width(w)
                    .max_width(w)
                    .min_height(h)
                    .max_height(h);
                s
            },
            Some(Box::new(|state: &Applet| {
                let (w, h) = state.grid.panel_size();
                let content = container(state.grid.view().map(Message::Grid))
                    .width(Length::Fixed(w))
                    .height(Length::Fixed(h));
                // popup_container defaults to a fixed 360 px applet width; the
                // grid needs its own size
                let popup = state.core.applet.popup_container(content).limits(
                    Limits::NONE
                        .min_width(w)
                        .max_width(w)
                        .min_height(h)
                        .max_height(h),
                );
                Element::from(popup).map(cosmic::Action::App)
            })),
        )));
        Task::batch([open, text_input::focus(SEARCH_ID.clone())])
    }

    fn launch(&mut self, app: Arc<App>) -> Task<Message> {
        self.config.record_launch(self.helper.as_ref(), app.id());
        self.grid.apply_config(&self.config);
        match self.token_tx.as_ref() {
            // ask the compositor for an activation token first (see Message::Token)
            Some(tx) => {
                let id = app.id().to_string();
                if tx
                    .send(TokenRequest {
                        app_id: <Self as cosmic::Application>::APP_ID.to_string(),
                        exec: id.clone(),
                    })
                    .is_ok()
                {
                    self.pending_launch.insert(id, app);
                } else {
                    tokio::spawn(oumuamua::apps::launch(app, None));
                }
            }
            None => {
                tokio::spawn(oumuamua::apps::launch(app, None));
            }
        }
        self.close_popup()
    }
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "io.github.broly1.OumuamuaApplet";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, (): ()) -> (Self, Task<Message>) {
        let (helper, config) = OumuamuaConfig::load();
        let mut grid = Grid::new(Position::Panel);
        grid.apply_config(&config);
        (
            Self {
                core,
                helper,
                config,
                grid,
                popup: None,
                token_tx: None,
                pending_launch: HashMap::new(),
            },
            Task::none(),
        )
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
                    Position::Centered => toggle_centered_grid(),
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
                Some(grid::Event::ToggleIcon) => {
                    self.config.toggle_symbolic_icon(self.helper.as_ref());
                    self.grid.apply_config(&self.config);
                }
                Some(grid::Event::ClearRecent) => {
                    self.config.clear_recent(self.helper.as_ref());
                    self.grid.apply_config(&self.config);
                }
                Some(grid::Event::Task(task)) => {
                    return task.map(|m| cosmic::Action::App(Message::Grid(m)));
                }
                Some(grid::Event::OpenUrl(url)) => {
                    oumuamua::open_url(url);
                    return self.close_popup();
                }
                Some(grid::Event::TogglePosition) => {
                    self.config
                        .save_position(self.helper.as_ref(), Position::Centered);
                    let close = self.close_popup();
                    // open the centered grid right away so the switch feels instant
                    toggle_centered_grid();
                    return close;
                }
                None => {}
            },
            Message::Config(config) => {
                self.config = config;
                self.grid.apply_config(&self.config);
            }
            Message::Surface(a) => return cosmic::task::message(cosmic::Action::Surface(a)),
            Message::Token(u) => match u {
                TokenUpdate::Init(tx) => self.token_tx = Some(tx),
                TokenUpdate::Finished => self.token_tx = None,
                TokenUpdate::ActivationToken { token, exec } => {
                    if let Some(app) = self.pending_launch.remove(&exec) {
                        tokio::spawn(oumuamua::apps::launch(app, token));
                    }
                }
            },
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let btn = self
            .core
            .applet
            .icon_button_from_handle(oumuamua::launcher_icon(self.config.symbolic_icon))
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
            oumuamua::fl!("applications"),
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
            activation_token_subscription(0).map(Message::Token),
            self.core
                .watch_config::<OumuamuaConfig>(oumuamua::config::APP_ID)
                .map(|u| Message::Config(u.config)),
        ];
        if self.popup.is_some() {
            subs.push(listen_with(|event, status, _| match event {
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                    key,
                    ..
                }) => {
                    let ignored = matches!(status, cosmic::iced::event::Status::Ignored);
                    match key {
                        Key::Named(Named::Escape) => Some(Message::Close),
                        Key::Named(Named::ArrowUp) => Some(Message::Grid(grid::Msg::Move(0, -1))),
                        Key::Named(Named::ArrowDown) => Some(Message::Grid(grid::Msg::Move(0, 1))),
                        Key::Named(Named::ArrowLeft) if ignored => {
                            Some(Message::Grid(grid::Msg::Move(-1, 0)))
                        }
                        Key::Named(Named::ArrowRight) if ignored => {
                            Some(Message::Grid(grid::Msg::Move(1, 0)))
                        }
                        Key::Named(Named::PageDown) => Some(Message::Grid(grid::Msg::PageDown)),
                        Key::Named(Named::PageUp) => Some(Message::Grid(grid::Msg::PageUp)),
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
    oumuamua::i18n::init_from_desktop();
    cosmic::applet::run::<Applet>(())
}
