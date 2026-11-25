prefix := "/usr/local"
bindir := prefix / "bin"
appdir := prefix / "share/applications"

default: build

build:
    cargo build --release

# install both binaries and desktop entries (run with sudo)
install:
    install -Dm0755 target/release/cosmic-appgrid {{bindir}}/cosmic-appgrid
    install -Dm0755 target/release/cosmic-appgrid-applet {{bindir}}/cosmic-appgrid-applet
    install -Dm0644 data/io.github.broly1.CosmicAppGrid.desktop {{appdir}}/io.github.broly1.CosmicAppGrid.desktop
    install -Dm0644 data/io.github.broly1.CosmicAppGridApplet.desktop {{appdir}}/io.github.broly1.CosmicAppGridApplet.desktop

uninstall:
    rm -f {{bindir}}/cosmic-appgrid {{bindir}}/cosmic-appgrid-applet
    rm -f {{appdir}}/io.github.broly1.CosmicAppGrid.desktop {{appdir}}/io.github.broly1.CosmicAppGridApplet.desktop

# copy the source to the dev VM, build and install it there
vm host="broly@192.168.122.197":
    rsync -a --delete --exclude target --exclude .git ./ {{host}}:cosmic-appgrid/
    ssh {{host}} 'cd cosmic-appgrid && cargo build --release && sudo just install'
