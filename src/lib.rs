// SPDX-License-Identifier: GPL-3.0-only
//! Shared code for `cosmic-ext-oumuamua` (centered launcher) and
//! `cosmic-ext-oumuamua-applet` (panel button / panel popup).

pub mod apps;
pub mod config;
pub mod grid;
pub mod i18n;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
pub const ISSUES: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues");

/// Open a web link in the user's browser.
pub fn open_url(url: &str) {
    match std::process::Command::new("xdg-open").arg(url).spawn() {
        // reap it in the background so it doesn't linger as a zombie
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => log::error!("failed to open {url}: {e}"),
    }
}

/// Colored launcher icon (porthole, planet, rocket).
pub const ICON_COLOR_SVG: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/io.github.broly1.Oumuamua.svg");
/// Grey-disc variant in the style of the COSMIC "Universe" icon themes (same
/// disc, outline and accent as their Launcher button). Its blue accent is
/// swapped for the accent of the Universe variant in use.
pub const ICON_UNIVERSE_SVG: &str = include_str!("../resources/icons/oumuamua-universe.svg");
/// Flat variant in the colours of COSMIC's own panel icons (teal, purple,
/// orange, white), for every other icon theme.
pub const ICON_COSMIC_SVG: &[u8] = include_bytes!("../resources/icons/oumuamua-cosmic.svg");

/// Accent colour of each Universe icon theme variant, as used in its icons.
const UNIVERSE_ACCENTS: [(&str, &str); 4] = [
    ("Blue", "#3479a1"),
    ("Green", "#626f47"),
    ("Orange", "#bc5d3d"),
    ("Red", "#a13434"),
];

/// The grey-disc icon for each Universe variant, built once.
static UNIVERSE_ICONS: std::sync::LazyLock<Vec<(&'static str, Vec<u8>)>> =
    std::sync::LazyLock::new(|| {
        UNIVERSE_ACCENTS
            .iter()
            .map(|(variant, accent)| {
                let svg = ICON_UNIVERSE_SVG.replace("#3479a1", accent);
                (*variant, svg.into_bytes())
            })
            .collect()
    });

/// Panel icon. The colored one is the app's own art. The theme-matching one
/// follows the icon theme, like the Launcher button beside it: the grey disc
/// in its accent colour under a Universe theme, and the flat COSMIC-style
/// icon under any other theme (those use COSMIC's own Launcher art).
#[must_use]
pub fn launcher_icon(theme_style: bool) -> cosmic::widget::icon::Handle {
    if !theme_style {
        return cosmic::widget::icon::from_svg_bytes(ICON_COLOR_SVG);
    }
    let theme = cosmic::icon_theme::default();
    if let Some(variant) = theme.strip_prefix("Universe - ") {
        let icons: &'static [(&str, Vec<u8>)] = &UNIVERSE_ICONS;
        let bytes = icons
            .iter()
            .find(|(v, _)| v.eq_ignore_ascii_case(variant.trim()))
            .unwrap_or(&icons[0])
            .1
            .as_slice();
        return cosmic::widget::icon::from_svg_bytes(bytes);
    }
    cosmic::widget::icon::from_svg_bytes(ICON_COSMIC_SVG)
}

/// Corner radius of the glass panel, the same one COSMIC uses for system
/// layer surfaces (App Library etc.). Used both for drawing the panel and for
/// the compositor's blur/clip shape, so the blur never shows past the corners.
#[must_use]
pub fn panel_radius(t: &cosmic::cosmic_theme::Theme) -> [f32; 4] {
    t.radius_s().map(|x| if x < 4.0 { x } else { x + 4.0 })
}

/// The same radius as a Wayland corner-radius hint for the compositor.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // small positive radii
pub fn panel_corners(
    t: &cosmic::cosmic_theme::Theme,
) -> cosmic::iced::runtime::platform_specific::wayland::CornerRadius {
    let r = panel_radius(t).map(|x| x.round() as u32);
    cosmic::iced::runtime::platform_specific::wayland::CornerRadius {
        top_left: r[0],
        top_right: r[1],
        bottom_right: r[2],
        bottom_left: r[3],
    }
}

/// Glass panel style: COSMIC background that follows the user's
/// "Frosted glass" opacity when libcosmic marks this surface transparent
/// (`theme.transparent`), plus the theme's corner radius and divider border.
#[must_use]
pub fn glass_style(theme: &cosmic::Theme) -> cosmic::widget::container::Style {
    use cosmic::iced::{Border, Color, Shadow};
    let t = theme.cosmic();
    cosmic::widget::container::Style {
        text_color: Some(t.on_bg_color().into()),
        icon_color: Some(t.on_bg_color().into()),
        background: Some(Color::from(t.background(theme.transparent).base).into()),
        border: Border {
            radius: panel_radius(t).into(),
            width: 1.0,
            color: t.bg_divider().into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}
