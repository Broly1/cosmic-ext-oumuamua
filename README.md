# App Grid for the COSMIC™ desktop

A frosted-glass application grid launcher for the [COSMIC™](https://system76.com/cosmic) desktop, written in Rust with [libcosmic](https://github.com/pop-os/libcosmic). It's inspired by [Plasma 6 AppGrid](https://github.com/xarbit/plasma6-applet-appgrid).

This is a community project. It is not affiliated with or endorsed by System76.

- **Always centered.** By default the grid opens in the exact middle of the screen, wherever the panel or dock is.
- **Or attached to the panel.** The ⋯ menu turns it into a popup attached to its panel button, like the Windows 11 Start menu, and back again.
- **Launchpad-style layout.** A search field, category chips (Productivity & Finance, Utilities, Social, Creativity, Entertainment, Other), a row of recently used apps, and a scrolling grid of all apps.
- **Glass that follows your desktop settings.** The grid uses COSMIC's own Frosted glass settings (*Settings → Desktop → Appearance → Frosted glass*):
  - The centered grid follows the **System interface** toggle.
  - The panel popup follows the **Applets** toggle.
  - Both use your blur strength and glass opacity, and update live when you change them.
- **Fast keyboard use.** Start typing to search. The arrow keys and Tab move the selection, Enter launches, Esc closes, and PgUp/PgDn jump a screen.
- **Its own icon.** A porthole with a planet and a rocket, inspired by the Pop!_OS installer artwork. It comes in color for a Windows-style bottom panel or dock, or in monochrome to match the other top-panel icons (switch it in the ⋯ menu).

## Components

| Binary | What it is |
|---|---|
| `cosmic-ext-appgrid` | The centered launcher. It runs as a single instance: running it again toggles it open or closed. Bind it to a key in *Settings → Keyboard → Custom shortcuts*. |
| `cosmic-ext-appgrid-applet` | The panel button, listed as *App Grid* in *Settings → Desktop → Panel → Applets*. |

Settings are stored with cosmic-config at `~/.config/cosmic/io.github.broly1.AppGrid/v1/`:

| Key | Default | Meaning |
|---|---|---|
| `position` | `Centered` | `Centered` or `Panel` |
| `columns` | `5` | Icons per row |
| `rows` | `4` | Rows visible at once (the rest scrolls) |
| `icon_size` | `52` | Icon size in pixels (32–128) |
| `symbolic_icon` | `false` | Use the monochrome panel icon |
| `recent` | `[]` | Recently launched desktop entry IDs, newest first |

Categories come from each app's freedesktop.org `Categories=` key. Office and finance apps go to Productivity & Finance, graphics and development tools to Creativity, browsers, mail and chat to Social, games and media players to Entertainment, and system tools to Utilities. Everything else goes to Other.

## Installation

A [justfile](./justfile) is included for the [casey/just][just] command runner. You need Rust 1.90 or newer and the libcosmic build dependencies. On Pop!_OS / Ubuntu:

```sh
sudo apt install build-essential pkg-config just cmake libxkbcommon-dev libwayland-dev \
  libexpat1-dev libfontconfig-dev libfreetype-dev libegl-dev libdbus-1-dev
just
sudo just install
```

Then add *App Grid* to your panel or dock, or run `cosmic-ext-appgrid`.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the launcher
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## Open it with the Super (Windows) key

COSMIC binds the Super key on its own to its launcher. To open App Grid instead, add this to `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom` (create the file if it doesn't exist; keep any other entries already in it):

```ron
{
    (modifiers: [Super]): Spawn("cosmic-ext-appgrid"),
}
```

Pressing Super again closes the grid. COSMIC's launcher stays available on Super+/. You can also add a custom shortcut for `cosmic-ext-appgrid` in *Settings → Keyboard → Shortcuts*.

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the `vendor` rule, and build with the vendored sources using the `build-vendored` rule. When installing files, use the `rootdir` and `prefix` variables to change installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/cosmic-ext-appgrid prefix=/usr install
```

It is recommended to build a source tarball with the vendored dependencies, which can typically be done by running `just vendor` on the host system before it enters the build environment.

## How the centering works

The centered grid is a full-screen Wayland layer surface on the overlay layer with `exclusive_zone = -1`, so panels and docks don't shift it. libcosmic then pads the surface down to the grid's size, with equal padding on each side, and the blur is requested together with that padding so it only ever covers the panel. Clicking outside the grid closes it.

## How this was made

This project was written with the help of an AI coding assistant (Claude, by Anthropic), directed, tested and reviewed by the maintainer. Its code is not intended for submission to System76's COSMIC repositories, which do not accept AI-generated contributions.

## License

GPL-3.0-only. See [LICENSE](LICENSE).

COSMIC™ is a trademark of System76, Inc.

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
