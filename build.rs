// SPDX-License-Identifier: GPL-3.0-only
//! Expands the desktop entries and `AppStream` metainfo with the translated
//! names, comments and keywords from `i18n/`, like the COSMIC templates.

use std::{env, fs, path::Path};
use xdgen::{App, Context, FluentString};

fn main() {
    println!("cargo::rerun-if-changed=i18n");
    println!("cargo::rerun-if-changed=resources");

    let ctx = Context::new("i18n", env::var("CARGO_PKG_NAME").unwrap()).unwrap();
    let app = App::new(FluentString("app-title"))
        .comment(FluentString("app-comment"))
        .keywords(FluentString("app-keywords"));
    let applet = App::new(FluentString("applet-title"))
        .comment(FluentString("applet-comment"))
        .keywords(FluentString("applet-keywords"));

    let output = Path::new("target/xdgen/");
    fs::create_dir_all(output).unwrap();
    fs::write(
        output.join("app.desktop"),
        app.expand_desktop("resources/app.desktop", &ctx).unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("applet.desktop"),
        applet
            .expand_desktop("resources/applet.desktop", &ctx)
            .unwrap(),
    )
    .unwrap();
    fs::write(
        output.join("app.metainfo.xml"),
        applet
            .expand_metainfo("resources/app.metainfo.xml", &ctx)
            .unwrap(),
    )
    .unwrap();
}
