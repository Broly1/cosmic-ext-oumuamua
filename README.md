# COSMIC App Grid

A frosted-glass application grid launcher for the [COSMIC](https://system76.com/cosmic) desktop, written in Rust with [libcosmic](https://github.com/pop-os/libcosmic). It's inspired by [Plasma 6 AppGrid](https://github.com/xarbit/plasma6-applet-appgrid).

- **Always centered.** By default the grid opens in the exact middle of the screen, wherever the panel or dock is.
- **Or attached to the panel.** One click on the move button turns it into a popup attached to its panel button, like the Windows 11 Start menu. Click it again to go back.
- **Glass that follows your desktop settings.** There is no custom blur code. The grid uses COSMIC's own Frosted glass settings (*Settings → Desktop → Appearance → Frosted glass*):
  - The centered grid follows the **System interface** toggle.
  - The panel popup follows the **Applets** toggle.
  - Both use your blur strength and glass opacity, and update live when you change them.
- **Fast keyboard use.** Start typing to search. The arrow keys and Tab move the selection, Enter launches, Esc closes, and PgUp/PgDn change page. The mouse wheel also changes page.
- **Favorites.** Right-click an app to pin it to the front of the grid (★).

## Components

| Binary | What it is |
|---|---|
| `cosmic-appgrid` | The centered launcher. It runs as a single instance: running it again toggles it open or closed. Bind it to a key in *Settings → Keyboard → Custom shortcuts*. |
| `cosmic-appgrid-applet` | The panel button, listed as *App Grid* in *Settings → Desktop → Panel → Applets*. |

Settings are stored with cosmic-config at `~/.config/cosmic/io.github.broly1.CosmicAppGrid/v1/`:

| Key | Default | Meaning |
|---|---|---|
| `position` | `Centered` | `Centered` or `Panel` |
| `columns`, `rows` | `7`, `4` | Grid size of one page when centered |
| `panel_columns`, `panel_rows` | `6`, `4` | Grid size of one page in the panel popup |
| `icon_size` | `64` | Icon size in pixels (32–128) |
| `favorites` | `[]` | Desktop entry IDs pinned first |

## Build and install

You need Rust 1.90 or newer, `just`, and the libcosmic build dependencies. On Pop!_OS / Ubuntu:

```sh
sudo apt install build-essential pkg-config just cmake libxkbcommon-dev libwayland-dev \
  libexpat1-dev libfontconfig-dev libfreetype-dev libegl-dev libdbus-1-dev
just build
sudo just install   # installs to /usr/local
```

Then add *App Grid* to your panel, or run `cosmic-appgrid`.

## How the centering works

The centered grid is a full-screen Wayland layer surface on the overlay layer with `exclusive_zone = -1`, so panels and docks don't shift it. libcosmic then pads the surface down to the grid's size, with equal padding on each side. Clicking outside the grid moves focus away, which closes it.

## License

GPL-3.0-only. See [LICENSE](LICENSE).
