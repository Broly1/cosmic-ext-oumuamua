// SPDX-License-Identifier: GPL-3.0-only
//! Shared code for `cosmic-appgrid` (centered launcher) and
//! `cosmic-appgrid-applet` (panel button / panel popup).

pub mod apps;
pub mod config;
pub mod grid;

/// Corner radius of the glass panel, the same one COSMIC uses for system
/// layer surfaces (App Library etc.). Used both for drawing the panel and for
/// the compositor's blur/clip shape, so the blur never shows past the corners.
pub fn panel_radius(t: &cosmic::cosmic_theme::Theme) -> [f32; 4] {
    t.radius_s().map(|x| if x < 4.0 { x } else { x + 4.0 })
}

/// The same radius as a Wayland corner-radius hint for the compositor.
pub fn panel_corners(t: &cosmic::cosmic_theme::Theme) -> cosmic::iced::runtime::platform_specific::wayland::CornerRadius {
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
