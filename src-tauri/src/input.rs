//! Pedal input backend: a dedicated thread that owns the SDL3 joystick subsystem.
//!
//! SDL contexts aren't `Send`, so the thread initialises SDL, keeps every connected joystick
//! open and publishes the device list. The UI gets the current list from `list_devices` and
//! is told about hot-plug changes through the `devices-changed` event.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use sct_core::device::{DeviceInfo, DevicesSnapshot, usb_ids_from_guid};
use sdl3::JoystickSubsystem;
use sdl3::event::Event;
use sdl3::joystick::{Joystick, JoystickId};
use tauri::{AppHandle, Emitter};

/// Event emitted with a [`DevicesSnapshot`] whenever the device list changes.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

/// How long the thread blocks waiting for SDL events. Bounds hot-plug latency.
const EVENT_WAIT: Duration = Duration::from_millis(50);

/// Shared handle to the input thread's state, managed by Tauri.
#[derive(Clone, Default)]
pub struct InputService {
    snapshot: Arc<Mutex<DevicesSnapshot>>,
}

impl InputService {
    /// Spawns the input thread. Failures are reported through the snapshot's `error`.
    pub fn spawn(app: AppHandle) -> Self {
        let service = Self::default();
        let shared = service.clone();
        let spawned = thread::Builder::new()
            .name("sct-input".into())
            .spawn(move || {
                if let Err(error) = run(&app, &shared) {
                    eprintln!("input thread stopped: {error}");
                    shared.publish(
                        &app,
                        DevicesSnapshot {
                            devices: Vec::new(),
                            error: Some(error),
                        },
                    );
                }
            });
        if let Err(error) = spawned {
            service.lock().error = Some(format!("failed to start input thread: {error}"));
        }
        service
    }

    /// Returns the latest device list.
    pub fn snapshot(&self) -> DevicesSnapshot {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DevicesSnapshot> {
        // The snapshot is replaced wholesale, so a poisoned value is still consistent.
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, app: &AppHandle, snapshot: DevicesSnapshot) {
        eprintln!("input: {} device(s)", snapshot.devices.len());
        for d in &snapshot.devices {
            eprintln!(
                "  [{}] {:?} guid={} axes={} buttons={} hats={}",
                d.id, d.name, d.guid, d.axis_count, d.button_count, d.hat_count
            );
        }
        *self.lock() = snapshot.clone();
        if let Err(error) = app.emit(DEVICES_CHANGED_EVENT, snapshot) {
            eprintln!("failed to emit {DEVICES_CHANGED_EVENT}: {error}");
        }
    }
}

fn run(app: &AppHandle, service: &InputService) -> Result<(), String> {
    // Our window isn't an SDL window, so without this SDL would treat the app as always
    // unfocused and drop joystick input.
    sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");

    let sdl = sdl3::init().map_err(|e| format!("SDL init failed: {e}"))?;
    let joysticks = sdl
        .joystick()
        .map_err(|e| format!("SDL joystick init failed: {e}"))?;
    let mut events = sdl
        .event_pump()
        .map_err(|e| format!("SDL event pump failed: {e}"))?;

    let mut open: HashMap<u32, Joystick> = HashMap::new();
    for id in joysticks
        .joysticks()
        .map_err(|e| format!("failed to list joysticks: {e}"))?
    {
        open_joystick(&joysticks, &mut open, id);
    }
    service.publish(app, snapshot_of(&open));

    loop {
        let Some(first) = events.wait_event_timeout(EVENT_WAIT) else {
            continue;
        };
        let mut changed = false;
        for event in std::iter::once(first).chain(events.poll_iter()) {
            match event {
                Event::JoyDeviceAdded { which, .. } => {
                    changed |= open_joystick(&joysticks, &mut open, which);
                }
                Event::JoyDeviceRemoved { which, .. } => {
                    changed |= open.remove(&u32::from(which)).is_some();
                }
                _ => {}
            }
        }
        if changed {
            service.publish(app, snapshot_of(&open));
        }
    }
}

/// Opens a joystick unless it's already open. Returns whether the list changed.
fn open_joystick(
    joysticks: &JoystickSubsystem,
    open: &mut HashMap<u32, Joystick>,
    id: JoystickId,
) -> bool {
    let key = u32::from(id);
    if open.contains_key(&key) {
        return false;
    }
    match joysticks.open(id) {
        Ok(joystick) => {
            open.insert(key, joystick);
            true
        }
        Err(error) => {
            eprintln!("failed to open joystick {key}: {error}");
            false
        }
    }
}

fn snapshot_of(open: &HashMap<u32, Joystick>) -> DevicesSnapshot {
    let mut devices: Vec<DeviceInfo> = open
        .iter()
        .map(|(&id, joystick)| {
            let guid = joystick.guid().string();
            let usb = usb_ids_from_guid(&guid);
            DeviceInfo {
                id,
                name: joystick.name(),
                vendor_id: usb.map(|(vendor, _)| vendor),
                product_id: usb.map(|(_, product)| product),
                guid,
                axis_count: joystick.num_axes(),
                button_count: joystick.num_buttons(),
                hat_count: joystick.num_hats(),
            }
        })
        .collect();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    DevicesSnapshot {
        devices,
        error: None,
    }
}
