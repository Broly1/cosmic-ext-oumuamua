// SPDX-License-Identifier: GPL-3.0-only
//! Open windows: typing part of a window's title or app finds it, Enter
//! brings it to the front.
//!
//! A background thread with its own Wayland connection keeps the list up to
//! date from `ext-foreign-toplevel-list-v1`, and focuses windows through
//! COSMIC's `zcosmic-toplevel-manager-v1`. Searching only reads that list,
//! so it costs nothing per keystroke. On a compositor without these
//! protocols the plugin is simply empty.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use cosmic::cctk::cosmic_protocols::toplevel_info::v1::client::{
    zcosmic_toplevel_handle_v1::{self, ZcosmicToplevelHandleV1},
    zcosmic_toplevel_info_v1::{self, ZcosmicToplevelInfoV1},
};
use cosmic::cctk::cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1::{
    self, ZcosmicToplevelManagerV1,
};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat::{self, WlSeat}};
use wayland_client::{Connection, Dispatch, QueueHandle, event_created_child};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};

use super::{Action, Item};

/// One open window.
#[derive(Debug, Clone)]
pub struct Window {
    /// Our own key for [`focus`].
    pub key: u64,
    pub title: String,
    pub app_id: String,
    ext: ExtForeignToplevelHandleV1,
    cosmic: Option<ZcosmicToplevelHandleV1>,
}

struct Shared {
    conn: Connection,
    seat: WlSeat,
    manager: Option<ZcosmicToplevelManagerV1>,
    windows: Mutex<Vec<Window>>,
}

/// Started on first use; `None` when the protocols are missing.
static SHARED: LazyLock<Option<Arc<Shared>>> = LazyLock::new(|| match start() {
    Ok(shared) => Some(shared),
    Err(e) => {
        log::warn!("open windows unavailable: {e}");
        None
    }
});

static NEXT_KEY: AtomicU64 = AtomicU64::new(1);

/// Start watching windows early, so the list is ready by the first search.
pub fn warm_up() {
    LazyLock::force(&SHARED);
}

/// The open windows right now, in the order they opened.
///
/// # Panics
///
/// If the window-list thread panicked while holding the list.
#[must_use]
pub fn list() -> Vec<Window> {
    SHARED
        .as_ref()
        .map(|s| s.windows.lock().unwrap().clone())
        .unwrap_or_default()
}

/// Windows whose title or app id contains `query`, as search results.
#[must_use]
pub fn search(query: &str) -> Vec<Item> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    list()
        .into_iter()
        .filter(|w| !w.title.is_empty() || !w.app_id.is_empty())
        .filter(|w| w.title.to_lowercase().contains(&q) || w.app_id.to_lowercase().contains(&q))
        .map(|w| Item {
            icons: icon_names(&w.app_id),
            title: if w.title.is_empty() {
                w.app_id.clone()
            } else {
                w.title.clone()
            },
            subtitle: crate::fl!("switch-to-window"),
            source: crate::fl!("plugin-window"),
            action: Action::Focus(w.key),
            complete: None,
        })
        .collect()
}

/// Icon names to try for an app id: the id itself (most COSMIC and Flatpak
/// apps name their icon after it), its last part, then a generic window.
fn icon_names(app_id: &str) -> Vec<String> {
    let mut names = vec![app_id.to_string(), app_id.to_lowercase()];
    if let Some(last) = app_id.rsplit('.').next() {
        names.push(last.to_lowercase());
    }
    names.push("application-x-executable".into());
    names.dedup();
    names
}

/// Bring window `key` to the front. Waits a moment first: the launcher
/// closes at the same time, and its surface holds the keyboard until gone.
///
/// # Panics
///
/// If the window-list thread panicked while holding the list.
pub fn focus(key: u64) {
    let Some(shared) = SHARED.as_ref().cloned() else {
        return;
    };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        let Some(manager) = &shared.manager else {
            return;
        };
        let target = shared
            .windows
            .lock()
            .unwrap()
            .iter()
            .find(|w| w.key == key)
            .and_then(|w| w.cosmic.clone());
        if let Some(handle) = target {
            manager.activate(&handle, &shared.seat);
            let _ = shared.conn.flush();
        }
    });
}

fn start() -> Result<Arc<Shared>, Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let _list: ExtForeignToplevelListV1 = globals.bind(&qh, 1..=1, ())?;
    let info: Option<ZcosmicToplevelInfoV1> = globals.bind(&qh, 2..=3, ()).ok();
    let manager: Option<ZcosmicToplevelManagerV1> = globals.bind(&qh, 1..=4, ()).ok();
    let seat: WlSeat = globals.bind(&qh, 1..=7, ())?;

    let shared = Arc::new(Shared {
        conn,
        seat,
        manager,
        windows: Mutex::new(Vec::new()),
    });
    let mut state = State {
        shared: shared.clone(),
        info,
    };
    // the first roundtrip delivers the windows already open
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;
    std::thread::spawn(move || {
        loop {
            if let Err(e) = queue.blocking_dispatch(&mut state) {
                log::warn!("window list connection ended: {e}");
                break;
            }
        }
    });
    Ok(shared)
}

struct State {
    shared: Arc<Shared>,
    info: Option<ZcosmicToplevelInfoV1>,
}

impl State {
    fn edit(&self, handle: &ExtForeignToplevelHandleV1, f: impl FnOnce(&mut Window)) {
        let mut windows = self.shared.windows.lock().unwrap();
        if let Some(w) = windows.iter_mut().find(|w| &w.ext == handle) {
            f(w);
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        (): &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            let cosmic = state
                .info
                .as_ref()
                .map(|info| info.get_cosmic_toplevel(&toplevel, qh, ()));
            state.shared.windows.lock().unwrap().push(Window {
                key: NEXT_KEY.fetch_add(1, Ordering::Relaxed),
                title: String::new(),
                app_id: String::new(),
                ext: toplevel,
                cosmic,
            });
        }
    }

    event_created_child!(State, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for State {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_foreign_toplevel_handle_v1::Event::Title { title } => {
                state.edit(handle, |w| w.title = title);
            }
            ext_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                state.edit(handle, |w| w.app_id = app_id);
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => {
                let mut windows = state.shared.windows.lock().unwrap();
                if let Some(i) = windows.iter().position(|w| &w.ext == handle) {
                    let w = windows.remove(i);
                    if let Some(c) = w.cosmic {
                        c.destroy();
                    }
                }
                handle.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<ZcosmicToplevelHandleV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelHandleV1,
        _: zcosmic_toplevel_handle_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZcosmicToplevelInfoV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelInfoV1,
        _: zcosmic_toplevel_info_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(State, ZcosmicToplevelInfoV1, [
        zcosmic_toplevel_info_v1::EVT_TOPLEVEL_OPCODE => (ZcosmicToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZcosmicToplevelManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZcosmicToplevelManagerV1,
        _: zcosmic_toplevel_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
