name := 'cosmic-ext-appgrid'
applet-name := 'cosmic-ext-appgrid-applet'
appid := 'io.github.broly1.AppGrid'
applet-appid := 'io.github.broly1.AppGridApplet'

rootdir := ''
prefix := '/usr'

# Installation paths
base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
appdata-dst := base-dir / 'share' / 'appdata' / applet-appid + '.metainfo.xml'
bin-dst := base-dir / 'bin' / name
applet-bin-dst := base-dir / 'bin' / applet-name
desktop-dst := base-dir / 'share' / 'applications' / appid + '.desktop'
applet-desktop-dst := base-dir / 'share' / 'applications' / applet-appid + '.desktop'
icons-dst := base-dir / 'share' / 'icons' / 'hicolor'
icon-dst := icons-dst / 'scalable' / 'apps' / appid + '.svg'
symbolic-icon-dst := icons-dst / 'symbolic' / 'apps' / appid + '-symbolic.svg'

# Default recipe which runs `just build-release`
default: build-release

# Runs `cargo clean`
clean:
    cargo clean

# Removes vendored dependencies
clean-vendor:
    rm -rf .cargo vendor vendor.tar

# `cargo clean` and removes vendored dependencies
clean-dist: clean clean-vendor

# Compiles with debug profile
build-debug *args:
    cargo build {{args}}

# Compiles with release profile
build-release *args: (build-debug '--release' args)

# Compiles release profile with vendored dependencies
build-vendored *args: vendor-extract (build-release '--frozen --offline' args)

# Runs a clippy check
check *args:
    cargo clippy --all-features {{args}} -- -W clippy::pedantic

# Runs a clippy check with JSON message format
check-json: (check '--message-format=json')

# Run the launcher for testing purposes
run *args:
    env RUST_BACKTRACE=full cargo run --release --bin {{name}} {{args}}

# Installs files
install:
    install -Dm0755 {{ cargo-target-dir / 'release' / name }} {{bin-dst}}
    install -Dm0755 {{ cargo-target-dir / 'release' / applet-name }} {{applet-bin-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.desktop' }} {{desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'applet.desktop' }} {{applet-desktop-dst}}
    install -Dm0644 {{ 'target' / 'xdgen' / 'app.metainfo.xml' }} {{appdata-dst}}
    install -Dm0644 resources/icons/hicolor/scalable/apps/{{appid}}.svg {{icon-dst}}
    install -Dm0644 resources/icons/hicolor/symbolic/apps/{{appid}}-symbolic.svg {{symbolic-icon-dst}}

# Uninstalls installed files
uninstall:
    rm {{bin-dst}} {{applet-bin-dst}} {{desktop-dst}} {{applet-desktop-dst}} {{appdata-dst}} {{icon-dst}} {{symbolic-icon-dst}}

# Vendor dependencies locally
vendor:
    mkdir -p .cargo
    cargo vendor --sync Cargo.toml | head -n -1 > .cargo/config.toml
    echo 'directory = "vendor"' >> .cargo/config.toml
    echo >> .cargo/config.toml
    tar pcf vendor.tar vendor
    rm -rf vendor

# Extracts vendored dependencies
vendor-extract:
    rm -rf vendor
    tar pxf vendor.tar

# Bump cargo version, create git commit, and create tag
tag version:
    find -type f -name Cargo.toml -exec sed -i '0,/^version/s/^version.*/version = "{{version}}"/' '{}' \; -exec git add '{}' \;
    cargo check
    cargo clean
    git add Cargo.lock
    git commit -m 'release: {{version}}'
    git tag -a {{version}} -m ''
