// SPDX-License-Identifier: GPL-3.0-only
//! A clipboard that outlives the launcher's surfaces.
//!
//! libcosmic ties the clipboard to a surface, and on Wayland copied text is
//! only available while the client that owns it keeps serving it, so text
//! copied just before the grid closes would vanish with the surface. Instead
//! a background thread with its own Wayland connection owns the selection
//! through the `ext-data-control-v1` protocol and serves it until something
//! else is copied. The launcher process stays resident, so the thread does.

use std::fs::File;
use std::io::Write;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, event_created_child};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

/// MIME types offered for plain text, as other Wayland clipboard owners do.
const MIME_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];

/// Put `text` on the clipboard. Returns `false` if the compositor doesn't
/// offer the data-control protocol (the caller can fall back).
#[must_use]
pub fn copy(text: String) -> bool {
    match own_selection(text) {
        Ok((mut queue, mut state)) => {
            std::thread::spawn(move || {
                while !state.done {
                    if let Err(e) = queue.blocking_dispatch(&mut state) {
                        log::warn!("clipboard connection ended: {e}");
                        break;
                    }
                }
            });
            true
        }
        Err(e) => {
            log::warn!("cannot own the clipboard: {e}");
            false
        }
    }
}

/// Bind the protocol and set the selection; the returned queue then serves
/// paste requests.
fn own_selection(
    text: String,
) -> Result<(EventQueue<State>, State), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let manager: ExtDataControlManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ())?;

    let device = manager.get_data_device(&seat, &qh, ());
    let source = manager.create_data_source(&qh, ());
    for mime in MIME_TYPES {
        source.offer(mime.to_string());
    }
    device.set_selection(Some(&source));

    let mut state = State {
        text: text.into_bytes(),
        done: false,
    };
    // make sure the compositor has the selection before returning
    queue.roundtrip(&mut state)?;
    Ok((queue, state))
}

struct State {
    text: Vec<u8>,
    /// Another client took the clipboard: stop serving.
    done: bool,
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { fd, .. } => {
                // a slow or closed reader must not stop the thread
                let _ = File::from(fd).write_all(&state.text);
            }
            ext_data_control_source_v1::Event::Cancelled => {
                source.destroy();
                state.done = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_data_control_device_v1::Event::Finished = event {
            state.done = true;
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

/// Offers for other clients' selections; not needed.
impl Dispatch<ExtDataControlOfferV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtDataControlOfferV1,
        _: <ExtDataControlOfferV1 as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtDataControlManagerV1,
        _: <ExtDataControlManagerV1 as wayland_client::Proxy>::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
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
