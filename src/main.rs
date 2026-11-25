// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-appgrid`: the centered frosted-glass app grid.
//!
//! Runs as a single instance. Starting it again (from the panel button or a
//! keyboard shortcut) toggles the grid open/closed over D-Bus.
//!
//! The grid is a full-screen layer surface that ignores panel exclusive zones
//! (`exclusive_zone = -1`) and is padded down to the grid's size, so it always
//! sits in the exact center of the screen wherever the panel is. Blur is
//! requested only when the user enabled Frosted glass for "System interface",
//! and is re-applied live when that setting changes.

use std::str::FromStr;
use std::time::{Duration, Instant};

use appgrid::config::{AppGridConfig, Position};
use appgrid::grid::{self, Grid, SEARCH_ID};
use clap::Parser;
use cosmic::app::{Core, CosmicFlags, Settings, Task};
use cosmic::cctk::sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use cosmic::cosmic_config;
use cosmic::iced::core::event::{PlatformSpecific, wayland};
use cosmic::iced::core::keyboard::{Key, key::Named};
use cosmic::iced::core::window::Id as SurfaceId;
use cosmic::iced::event::listen_with;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{destroy_layer_surface, set_padding};
use cosmic::iced::platform_specific::shell::wayland::commands::activation::request_token;
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::{IcedMargin, SctkLayerSurfaceSettings};
use cosmic::iced::window::Event as WindowEvent;
use cosmic::iced::{Length, Size, Subscription};
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::{container, mouse_area, text_input};
use cosmic::{Element, dbus_activation};
use serde::{Deserialize, Serialize};

#[derive(Parser, Debug, Clone, Serialize, Deserialize)]
#[command(version, about = "Frosted-glass application grid for COSMIC")]
struct Args {
    #[clap(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Debug, Clone, Serialize, Deserialize, clap::Subcommand)]
enum Cmd {
    /// Toggle the grid (default when no subcommand is given)
    Toggle,
    /// Close the grid if it is open
    Close,
}

impl std::fmt::Display for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

impl FromStr for Cmd {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

impl CosmicFlags for Args {
    type SubCommand = Cmd;
    type Args = Vec<String>;
    fn action(&self) -> Option<&Cmd> {
        self.cmd.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Hidden,
    WaitingToBeShown,
    Visible,
}

#[derive(Debug, Clone)]
enum Message {
    Grid(grid::Msg),
    Hide,
    Noop,
    Opened(Size, SurfaceId),
    Layer(wayland::LayerEvent, SurfaceId),
    Config(AppGridConfig),
    Launch(std::sync::Arc<appgrid::apps::App>, Option<String>),
}

struct AppGrid {
    core: Core,
    helper: Option<cosmic_config::Config>,
    config: AppGridConfig,
    grid: Grid,
    state: State,
    /// Output size reported when the full-screen surface opened.
    size: Size,
    last_hide: Option<Instant>,
}

impl AppGrid {
    fn padding(&self) -> IcedMargin {
        let (w, h) = self.grid.panel_size();
        let w = w.min(self.size.width - 32.0);
        let h = h.min(self.size.height - 32.0);
        let x = ((self.size.width - w) / 2.0).max(0.0).floor() as i32;
        let y = ((self.size.height - h) / 2.0).max(0.0).floor() as i32;
        IcedMargin { top: y, bottom: y, left: x, right: x }
    }

    fn toggle(&mut self) -> Task<Message> {
        match self.state {
            State::Visible => self.hide(),
            State::Hidden if self.last_hide.is_none_or(|t| t.elapsed() > Duration::from_millis(150)) => {
                self.show()
            }
            _ => Task::none(),
        }
    }

    fn show(&mut self) -> Task<Message> {
        self.state = State::WaitingToBeShown;
        self.grid.reset();
        cosmic::surface::surface_task(app_layer_shell(
            |app: &AppGrid| LiveSettings {
                padding: Some(app.padding()),
                corners: Some(appgrid::panel_corners(app.core.system_theme().cosmic())),
                // None = libcosmic owns the blur: it enables/disables it from the
                // user's Frosted glass "System interface" setting (live), and the
                // compositor clips it to the padded, rounded panel.
                blur: None,
            },
            |_: &mut AppGrid| SctkLayerSurfaceSettings {
                id: SurfaceId::RESERVED,
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::all(),
                exclusive_zone: -1,
                namespace: "cosmic-appgrid".into(),
                size: Some((None, None)),
                ..Default::default()
            },
            None,
        ))
    }

    fn hide(&mut self) -> Task<Message> {
        if self.state == State::Hidden {
            return Task::none();
        }
        self.state = State::Hidden;
        self.last_hide = Some(Instant::now());
        destroy_layer_surface(SurfaceId::RESERVED)
    }
}

impl cosmic::Application for AppGrid {
    type Executor = cosmic::executor::Default;
    type Flags = Args;
    type Message = Message;
    const APP_ID: &'static str = appgrid::config::APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, args: Args) -> (Self, Task<Message>) {
        core.set_keyboard_nav(false);
        core.set_app_type(cosmic::core::AppType::System);
        let (helper, config) = AppGridConfig::load();
        let mut grid = Grid::new(Position::Centered);
        grid.apply_config(&config);
        let mut app = Self {
            core,
            helper,
            config,
            grid,
            state: State::Hidden,
            size: Size::new(1920.0, 1080.0),
            last_hide: None,
        };
        // the first launch is itself a request to open
        let task = match args.cmd {
            Some(Cmd::Close) => Task::none(),
            _ => app.show(),
        };
        (app, task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Grid(msg) => match self.grid.update(msg) {
                Some(grid::Event::Launch(app)) => {
                    return request_token(Some(Self::APP_ID.to_string()), Some(SurfaceId::RESERVED))
                        .map(move |token| cosmic::Action::App(Message::Launch(app.clone(), token)));
                }
                Some(grid::Event::ToggleFavorite(id)) => {
                    self.config.toggle_favorite(self.helper.as_ref(), &id);
                    self.grid.apply_config(&self.config);
                }
                Some(grid::Event::TogglePosition) => {
                    // the panel applet watches this setting and takes over
                    self.config.save_position(self.helper.as_ref(), Position::Panel);
                    return self.hide();
                }
                None => {}
            },
            Message::Launch(app, token) => {
                tokio::spawn(appgrid::apps::launch(app, token));
                return self.hide();
            }
            Message::Hide => return self.hide(),
            Message::Noop => {}
            Message::Opened(size, id) if id == SurfaceId::RESERVED => {
                if self.state == State::WaitingToBeShown {
                    self.state = State::Visible;
                }
                self.size = size;
                return Task::batch([
                    set_padding::<()>(SurfaceId::RESERVED, self.padding()).discard(),
                    text_input::focus(SEARCH_ID.clone()),
                ]);
            }
            Message::Opened(..) => {}
            Message::Layer(ev, id) if id == SurfaceId::RESERVED => match ev {
                wayland::LayerEvent::Focused => return text_input::focus(SEARCH_ID.clone()),
                wayland::LayerEvent::Unfocused if self.state == State::Visible => return self.hide(),
                _ => {}
            },
            Message::Layer(..) => {}
            Message::Config(config) => {
                self.config = config;
                self.grid.apply_config(&self.config);
                if self.state == State::Visible {
                    return set_padding::<()>(SurfaceId::RESERVED, self.padding()).discard();
                }
            }
        }
        Task::none()
    }

    fn dbus_activation(&mut self, msg: dbus_activation::Message) -> Task<Message> {
        match msg.msg {
            dbus_activation::Details::Activate => self.toggle(),
            dbus_activation::Details::ActivateAction { action, .. } => match Cmd::from_str(&action) {
                Ok(Cmd::Close) => self.hide(),
                Ok(Cmd::Toggle) => self.toggle(),
                Err(_) => Task::none(),
            },
            _ => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        unimplemented!("no main window")
    }

    fn view_window(&self, _id: SurfaceId) -> Element<'_, Message> {
        use cosmic::iced::widget::{column, stack};
        use cosmic::widget::space;
        let pad = self.padding();
        let w = (self.size.width - 2.0 * pad.left as f32).max(1.0);
        let h = (self.size.height - 2.0 * pad.top as f32).max(1.0);
        let panel = container(self.grid.view().map(Message::Grid))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .class(cosmic::theme::Container::custom(appgrid::glass_style));
        // The surface covers the whole output (it ignores panel exclusive
        // zones), so the panel is placed at the exact center of the screen.
        // Clicking anywhere outside it closes the grid.
        stack![
            mouse_area(container(space::horizontal()).width(Length::Fill).height(Length::Fill))
                .on_press(Message::Hide),
            column![
                space::vertical().height(Length::Fixed(pad.top as f32)),
                mouse_area(panel).on_press(Message::Noop),
            ]
            .align_x(cosmic::iced::Alignment::Center)
            .width(Length::Fill),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            listen_with(|event, status, id| match event {
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(wayland::Event::Layer(e, _, id))) => {
                    Some(Message::Layer(e, id))
                }
                cosmic::iced::Event::Window(WindowEvent::Opened { size, .. }) => Some(Message::Opened(size, id)),
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    let ignored = matches!(status, cosmic::iced::event::Status::Ignored);
                    match key {
                        Key::Named(Named::Escape) => Some(Message::Hide),
                        Key::Named(Named::ArrowUp) => Some(Message::Grid(grid::Msg::Move(0, -1))),
                        Key::Named(Named::ArrowDown) => Some(Message::Grid(grid::Msg::Move(0, 1))),
                        Key::Named(Named::ArrowLeft) if ignored => Some(Message::Grid(grid::Msg::Move(-1, 0))),
                        Key::Named(Named::ArrowRight) if ignored => Some(Message::Grid(grid::Msg::Move(1, 0))),
                        Key::Named(Named::Tab) if modifiers.shift() => Some(Message::Grid(grid::Msg::Move(-1, 0))),
                        Key::Named(Named::Tab) => Some(Message::Grid(grid::Msg::Move(1, 0))),
                        Key::Named(Named::PageDown) => Some(Message::Grid(grid::Msg::NextPage)),
                        Key::Named(Named::PageUp) => Some(Message::Grid(grid::Msg::PrevPage)),
                        _ => None,
                    }
                }
                _ => None,
            }),
            self.core
                .watch_config::<AppGridConfig>(appgrid::config::APP_ID)
                .map(|update| Message::Config(update.config)),
        ])
    }
}

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = Args::parse();
    let settings = Settings::default()
        .antialiasing(true)
        .client_decorations(true)
        .no_main_window(true)
        .exit_on_close(false);
    cosmic::app::run_single_instance::<AppGrid>(settings, args)
}
