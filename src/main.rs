// SPDX-License-Identifier: GPL-3.0-only
//! `cosmic-ext-oumuamua`: the centered frosted-glass app grid.
//!
//! Runs as a single instance. Starting it again (from the panel button or a
//! keyboard shortcut) toggles the grid open/closed over D-Bus.
//!
//! The grid is a full-screen layer surface that ignores panel exclusive zones
//! (`exclusive_zone = -1`) and is padded down to the grid's size, so it always
//! sits in the exact center of the screen wherever the panel is. Blur is
//! requested only when the user enabled Frosted glass for "System interface",
//! together with the padding (so it never covers the whole screen), and is
//! re-applied live when that setting changes.

use std::str::FromStr;
use std::time::{Duration, Instant};

use clap::Parser;
use cosmic::app::{Core, CosmicFlags, Settings, Task};
use cosmic::cctk::sctk::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};
use cosmic::cosmic_config;
use cosmic::iced::core::event::{PlatformSpecific, wayland};
use cosmic::iced::core::keyboard::{Key, key::Named};
use cosmic::iced::core::window::Id as SurfaceId;
use cosmic::iced::event::listen_with;
use cosmic::iced::platform_specific::shell::commands::layer_surface::{
    destroy_layer_surface, set_padding,
};
use cosmic::iced::platform_specific::shell::wayland::commands::activation::request_token;
use cosmic::iced::runtime::platform_specific::wayland::layer_surface::{
    IcedMargin, SctkLayerSurfaceSettings,
};
use cosmic::iced::window::Event as WindowEvent;
use cosmic::iced::{Length, Size, Subscription};
use cosmic::surface::action::{LiveSettings, app_layer_shell};
use cosmic::widget::{container, mouse_area, text_input};
use cosmic::{Element, dbus_activation};
use oumuamua::config::{OumuamuaConfig, Position};
use oumuamua::grid::{self, Grid, SEARCH_ID};
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
    /// The compositor confirmed blur support (startup).
    BlurEnabled,
    ReapplyBlur,
    Opened(Size, SurfaceId),
    Layer(wayland::LayerEvent, SurfaceId),
    /// An output appeared or changed: recreate the keep-alive surface if
    /// the compositor closed it.
    Output,
    Config(OumuamuaConfig),
    Launch(std::sync::Arc<oumuamua::apps::App>, Option<String>),
}

struct Oumuamua {
    core: Core,
    helper: Option<cosmic_config::Config>,
    config: OumuamuaConfig,
    grid: Grid,
    state: State,
    /// Output size reported when the full-screen surface opened.
    size: Size,
    last_hide: Option<Instant>,
    /// The invisible surface that stays open while the grid is hidden
    /// ([`Self::keep_alive_surface`]); `None` once the compositor closed it.
    keep_alive: Option<SurfaceId>,
}

impl Oumuamua {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss
    )] // pixel sizes
    fn padding(&self) -> IcedMargin {
        let (w, h) = self.grid.panel_size();
        let w = w.min(self.size.width - 32.0);
        let h = h.min(self.size.height - 32.0);
        let x = ((self.size.width - w) / 2.0).max(0.0).floor() as i32;
        let y = ((self.size.height - h) / 2.0).max(0.0).floor() as i32;
        IcedMargin {
            top: y,
            bottom: y,
            left: x,
            right: x,
        }
    }

    fn toggle(&mut self) -> Task<Message> {
        match self.state {
            State::Visible => self.hide(),
            State::Hidden
                if self
                    .last_hide
                    .is_none_or(|t| t.elapsed() > Duration::from_millis(150)) =>
            {
                self.show()
            }
            _ => Task::none(),
        }
    }

    /// Blur behind the panel if the user enabled Frosted glass for
    /// "System interface". The compositor clips it to the padded,
    /// rounded panel.
    fn blur_task(frosted: bool) -> Task<Message> {
        use cosmic::iced::runtime::{Action, platform_specific, task};
        let rects = frosted.then(|| {
            vec![cosmic::iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: f32::MAX,
                height: f32::MAX,
            }]
        });
        task::effect(Action::PlatformSpecific(
            platform_specific::Action::Wayland(platform_specific::wayland::Action::BlurSurface(
                SurfaceId::RESERVED,
                rects,
            )),
        ))
    }

    fn frosted(&self) -> bool {
        self.core.system_theme().cosmic().frosted_system_interface
    }

    fn show(&mut self) -> Task<Message> {
        self.state = State::WaitingToBeShown;
        self.grid.reset();
        cosmic::surface::surface_task(app_layer_shell(
            |app: &Oumuamua| LiveSettings {
                padding: Some(app.padding()),
                corners: Some(oumuamua::panel_corners(app.core.system_theme().cosmic())),
                // Not None: libcosmic would blur the whole surface before the
                // padding exists, flashing the entire screen. Blur is sent
                // together with the padding once the surface is open.
                blur: Some(false),
            },
            |_: &mut Oumuamua| SctkLayerSurfaceSettings {
                id: SurfaceId::RESERVED,
                layer: Layer::Overlay,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                anchor: Anchor::all(),
                exclusive_zone: -1,
                namespace: "oumuamua".into(),
                size: Some((None, None)),
                ..Default::default()
            },
            None,
        ))
    }

    /// A 1×1 invisible surface on the bottom layer that is never closed,
    /// like COSMIC Launcher's "dummy" surface. Without it the grid's surface
    /// is the app's only one, and destroying it on every close lost about
    /// 26 KB per open inside the toolkit; with it the loss is gone.
    fn keep_alive_surface(&mut self) -> Task<Message> {
        let id = SurfaceId::unique();
        self.keep_alive = Some(id);
        cosmic::surface::surface_task(cosmic::surface::action::simple_layer_shell::<Message>(
            || LiveSettings {
                padding: None,
                corners: None,
                // no blur, even with Frosted glass on
                blur: Some(false),
            },
            move || SctkLayerSurfaceSettings {
                id,
                layer: Layer::Bottom,
                keyboard_interactivity: KeyboardInteractivity::None,
                input_zone: Some(Vec::new()),
                anchor: Anchor::TOP,
                namespace: "oumuamua_keep_alive".into(),
                size: Some((Some(1), Some(1))),
                exclusive_zone: -1,
                ..Default::default()
            },
            None::<fn() -> Element<'static, cosmic::Action<Message>>>,
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

/// Re-apply our blur after libcosmic's own blur commands for this update.
fn reapply_blur_later() -> Task<Message> {
    Task::future(async {
        tokio::time::sleep(Duration::from_millis(120)).await;
        cosmic::Action::App(Message::ReapplyBlur)
    })
}

impl cosmic::Application for Oumuamua {
    type Executor = cosmic::executor::Default;
    type Flags = Args;
    type Message = Message;
    const APP_ID: &'static str = oumuamua::config::APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(mut core: Core, args: Args) -> (Self, Task<Message>) {
        core.set_keyboard_nav(false);
        core.set_app_type(cosmic::core::AppType::System);
        // start watching open windows now, off the UI thread
        std::thread::spawn(oumuamua::plugins::windows::warm_up);
        let (helper, config) = OumuamuaConfig::load();
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
            keep_alive: None,
        };
        // the first launch is itself a request to open
        let task = match args.cmd {
            Some(Cmd::Close) => Task::none(),
            _ => app.show(),
        };
        let keep_alive = app.keep_alive_surface();
        (app, Task::batch([keep_alive, task]))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Grid(msg) => match self.grid.update(msg) {
                Some(grid::Event::Launch(app)) => {
                    return request_token(
                        Some(Self::APP_ID.to_string()),
                        Some(SurfaceId::RESERVED),
                    )
                    .map(move |token| cosmic::Action::App(Message::Launch(app.clone(), token)));
                }
                Some(grid::Event::Action(action)) => {
                    // the clipboard is written while the surface still has
                    // focus, then the grid closes
                    if let oumuamua::plugins::Action::Copy(text) = action {
                        if oumuamua::clipboard::copy(text.clone()) {
                            return self.hide();
                        }
                        // no data-control protocol: libcosmic's clipboard
                        // only lasts while the grid is open, so stay open
                        return cosmic::iced::clipboard::write(text);
                    }
                    oumuamua::plugins::perform(&action);
                    return self.hide();
                }
                Some(grid::Event::ToggleIcon) => {
                    self.config.toggle_symbolic_icon(self.helper.as_ref());
                    self.grid.apply_config(&self.config);
                }
                Some(grid::Event::TextSize(step)) => {
                    self.config.step_font_size(self.helper.as_ref(), step);
                    log::debug!("font size step {step}: now {}", self.config.font_size);
                    self.grid.apply_config(&self.config);
                    // the grid changed size: re-centre it
                    if self.state == State::Visible {
                        return set_padding::<()>(SurfaceId::RESERVED, self.padding()).discard();
                    }
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
                    return self.hide();
                }
                Some(grid::Event::Power(p)) => {
                    p.run();
                    return self.hide();
                }
                Some(grid::Event::Close) => return self.hide(),
                Some(grid::Event::TogglePosition) => {
                    // the panel applet watches this setting and takes over
                    self.config
                        .save_position(self.helper.as_ref(), Position::Panel);
                    return self.hide();
                }
                None => {}
            },
            Message::Launch(app, token) => {
                self.config.record_launch(self.helper.as_ref(), app.id());
                self.grid.apply_config(&self.config);
                tokio::spawn(oumuamua::apps::launch(app, token));
                return self.hide();
            }
            Message::Hide => return self.hide(),
            // libcosmic answers this event by disabling blur on surfaces that
            // opted out of its auto blur (ours); re-apply ours after it.
            Message::BlurEnabled => return reapply_blur_later(),
            Message::ReapplyBlur if self.state == State::Visible => {
                return Self::blur_task(self.frosted());
            }
            Message::Opened(size, id) if id == SurfaceId::RESERVED => {
                if self.state == State::WaitingToBeShown {
                    self.state = State::Visible;
                }
                self.size = size;
                // padding and blur go out together so they land in the same commit
                return Task::batch([
                    set_padding::<()>(SurfaceId::RESERVED, self.padding()).discard(),
                    Self::blur_task(self.frosted()),
                    text_input::focus(SEARCH_ID.clone()),
                ]);
            }
            Message::Layer(wayland::LayerEvent::Done, id) if self.keep_alive == Some(id) => {
                self.keep_alive = None;
            }
            Message::Output if self.keep_alive.is_none() => return self.keep_alive_surface(),
            Message::Layer(ev, id) if id == SurfaceId::RESERVED => match ev {
                wayland::LayerEvent::Focused => return text_input::focus(SEARCH_ID.clone()),
                wayland::LayerEvent::Unfocused if self.state == State::Visible => {
                    return self.hide();
                }
                _ => {}
            },
            Message::Noop
            | Message::ReapplyBlur
            | Message::Opened(..)
            | Message::Layer(..)
            | Message::Output => {}
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

    /// Theme or Frosted glass setting changed: follow it live. libcosmic
    /// sends its own (disabling) blur commands for our surface right after
    /// this hook, so ours is re-applied once those have run.
    fn system_theme_update(
        &mut self,
        _keys: &[&'static str],
        _theme: &cosmic::cosmic_theme::Theme,
    ) -> Task<Message> {
        if self.state == State::Visible {
            reapply_blur_later()
        } else {
            Task::none()
        }
    }

    fn dbus_activation(&mut self, msg: dbus_activation::Message) -> Task<Message> {
        match msg.msg {
            dbus_activation::Details::Activate => self.toggle(),
            dbus_activation::Details::ActivateAction { action, .. } => match Cmd::from_str(&action)
            {
                Ok(Cmd::Close) => self.hide(),
                Ok(Cmd::Toggle) => self.toggle(),
                Err(_) => Task::none(),
            },
            dbus_activation::Details::Open { .. } => Task::none(),
        }
    }

    fn view(&self) -> Element<'_, Message> {
        unimplemented!("no main window")
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss
    )] // pixel sizes
    fn view_window(&self, id: SurfaceId) -> Element<'_, Message> {
        use cosmic::iced::widget::{column, stack};
        use cosmic::widget::space;
        if self.keep_alive == Some(id) {
            return space::horizontal().into();
        }
        let pad = self.padding();
        let w = (self.size.width - 2.0 * pad.left as f32).max(1.0);
        let h = (self.size.height - 2.0 * pad.top as f32).max(1.0);
        let panel = container(self.grid.view().map(Message::Grid))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .class(cosmic::theme::Container::custom(oumuamua::glass_style));
        // The surface covers the whole output (it ignores panel exclusive
        // zones), so the panel is placed at the exact center of the screen.
        // Clicking anywhere outside it closes the grid.
        stack![
            mouse_area(
                container(space::horizontal())
                    .width(Length::Fill)
                    .height(Length::Fill)
            )
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
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(
                    wayland::Event::Layer(e, _, id),
                )) => Some(Message::Layer(e, id)),
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(
                    wayland::Event::Output(
                        wayland::OutputEvent::Created(_) | wayland::OutputEvent::InfoUpdate(_),
                        _,
                    ),
                )) => Some(Message::Output),
                cosmic::iced::Event::PlatformSpecific(PlatformSpecific::Wayland(
                    wayland::Event::BlurEnabled,
                )) => Some(Message::BlurEnabled),
                cosmic::iced::Event::Window(WindowEvent::Opened { size, .. }) => {
                    Some(Message::Opened(size, id))
                }
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::ModifiersChanged(
                    m,
                )) => Some(Message::Grid(grid::Msg::AltHeld(m.alt()))),
                cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                    key,
                    modifiers,
                    ..
                }) => {
                    let ignored = matches!(status, cosmic::iced::event::Status::Ignored);
                    match key {
                        Key::Named(Named::Escape) => Some(Message::Grid(grid::Msg::Back)),
                        Key::Named(Named::ArrowUp) => Some(Message::Grid(grid::Msg::Move(0, -1))),
                        Key::Named(Named::ArrowDown) => Some(Message::Grid(grid::Msg::Move(0, 1))),
                        Key::Named(Named::ArrowLeft) if ignored => {
                            Some(Message::Grid(grid::Msg::Move(-1, 0)))
                        }
                        Key::Named(Named::ArrowRight) if ignored => {
                            Some(Message::Grid(grid::Msg::Move(1, 0)))
                        }
                        Key::Named(Named::Tab) if modifiers.shift() => {
                            Some(Message::Grid(grid::Msg::Move(-1, 0)))
                        }
                        Key::Named(Named::Tab) => Some(Message::Grid(grid::Msg::Complete)),
                        Key::Named(Named::PageDown) => Some(Message::Grid(grid::Msg::PageDown)),
                        Key::Named(Named::PageUp) => Some(Message::Grid(grid::Msg::PageUp)),
                        Key::Character(c) if modifiers.alt() => c
                            .parse::<usize>()
                            .ok()
                            .filter(|n| (1..=9).contains(n))
                            .map(|n| Message::Grid(grid::Msg::Activate(n - 1))),
                        _ => None,
                    }
                }
                _ => None,
            }),
            self.core
                .watch_config::<OumuamuaConfig>(oumuamua::config::APP_ID)
                .map(|update| Message::Config(update.config)),
        ])
    }
}

fn main() -> cosmic::iced::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    oumuamua::i18n::init_from_desktop();
    let args = Args::parse();
    let settings = Settings::default()
        .antialiasing(true)
        .client_decorations(true)
        .no_main_window(true)
        .exit_on_close(false);
    cosmic::app::run_single_instance::<Oumuamua>(settings, args)
}
