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
/// Monochrome variant, tinted by the theme like other panel icons.
pub const ICON_SYMBOLIC_SVG: &[u8] = include_bytes!(
    "../resources/icons/hicolor/symbolic/apps/io.github.broly1.Oumuamua-symbolic.svg"
);

pub fn launcher_icon(symbolic: bool) -> cosmic::widget::icon::Handle {
    if symbolic {
        let mut h = cosmic::widget::icon::from_svg_bytes(ICON_SYMBOLIC_SVG);
        h.symbolic = true;
        h
    } else {
        cosmic::widget::icon::from_svg_bytes(ICON_COLOR_SVG)
    }
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
