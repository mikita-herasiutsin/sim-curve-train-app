//! Pedal input backend: a dedicated thread that owns the SDL3 joystick subsystem.
//!
//! SDL contexts aren't `Send`, so the thread initialises SDL, keeps every connected joystick
//! open and publishes the device list. The UI gets the current list from `list_devices` and
//! is told about hot-plug changes through the `devices-changed` event.
//!
//! While a stream is active the thread polls the selected device at about 1 kHz, stamps every
//! sample with a monotonic timestamp (`Instant`, which is QPC on Windows), keeps the last few
//! seconds in a ring buffer and sends batches to the UI over a Tauri `Channel`.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sct_core::device::{DeviceInfo, DevicesSnapshot, usb_ids_from_guid};
use sct_core::input::{MAX_AXES, RawSample};
use sct_core::ring_buffer::RingBuffer;
use sct_core::stream::{RateMeter, SampleBatch, StreamStats};
use sdl3::EventPump;
use sdl3::JoystickSubsystem;
use sdl3::event::Event;
use sdl3::joystick::{Joystick, JoystickId};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

/// Event emitted with a [`DevicesSnapshot`] whenever the device list changes.
pub const DEVICES_CHANGED_EVENT: &str = "devices-changed";

/// How long the idle thread blocks waiting for SDL events. Bounds hot-plug and command latency.
const EVENT_WAIT: Duration = Duration::from_millis(50);
/// Target polling period while streaming (about 1 kHz).
const POLL_INTERVAL: Duration = Duration::from_millis(1);
/// Maximum time between two batches sent to the UI.
const BATCH_INTERVAL: Duration = Duration::from_millis(8);
/// If the loop falls this far behind schedule, it resyncs instead of polling in a burst.
const MAX_LAG: Duration = Duration::from_millis(10);
/// Samples kept for scoring: 10 s at 1 kHz.
const RECENT_CAPACITY: usize = 10_000;

/// Requests from Tauri commands to the input thread.
enum Command {
    Start {
        device_id: u32,
        channel: Channel<SampleBatch>,
    },
    Stop,
}

/// Shared handle to the input thread's state, managed by Tauri.
#[derive(Clone)]
pub struct InputService {
    snapshot: Arc<Mutex<DevicesSnapshot>>,
    recent: Arc<Mutex<RingBuffer<RawSample>>>,
    commands: Sender<Command>,
}

impl InputService {
    /// Spawns the input thread. Failures are reported through the snapshot's `error`.
    pub fn spawn(app: AppHandle) -> Self {
        let (commands, receiver) = mpsc::channel();
        let service = Self {
            snapshot: Arc::default(),
            recent: Arc::new(Mutex::new(RingBuffer::with_capacity(RECENT_CAPACITY))),
            commands,
        };
        let shared = service.clone();
        let spawned = thread::Builder::new()
            .name("sct-input".into())
            .spawn(move || {
                let result =
                    panic::catch_unwind(AssertUnwindSafe(|| run(&app, &shared, &receiver)))
                        .unwrap_or_else(|payload| {
                            Err(format!(
                                "input thread panicked: {}",
                                panic_message(&*payload)
                            ))
                        });
                if let Err(error) = result {
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
            lock(&service.snapshot).error = Some(format!("failed to start input thread: {error}"));
        }
        service
    }

    /// Returns the latest device list.
    pub fn snapshot(&self) -> DevicesSnapshot {
        lock(&self.snapshot).clone()
    }

    /// Starts streaming samples of `device_id` to `channel`, replacing any active stream.
    pub fn start_stream(
        &self,
        device_id: u32,
        channel: Channel<SampleBatch>,
    ) -> Result<(), String> {
        if !self.snapshot().devices.iter().any(|d| d.id == device_id) {
            return Err(format!("device {device_id} is not connected"));
        }
        lock(&self.recent).clear();
        self.send(Command::Start { device_id, channel })
    }

    /// Stops the active stream, if any.
    pub fn stop_stream(&self) -> Result<(), String> {
        self.send(Command::Stop)
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "input thread is not running".to_owned())
    }

    fn publish(&self, app: &AppHandle, snapshot: DevicesSnapshot) {
        eprintln!("input: {} device(s)", snapshot.devices.len());
        for d in &snapshot.devices {
            eprintln!(
                "  [{}] {:?} guid={} axes={} buttons={} hats={}",
                d.id, d.name, d.guid, d.axis_count, d.button_count, d.hat_count
            );
        }
        *lock(&self.snapshot) = snapshot.clone();
        if let Err(error) = app.emit(DEVICES_CHANGED_EVENT, snapshot) {
            eprintln!("failed to emit {DEVICES_CHANGED_EVENT}: {error}");
        }
    }
}

/// Locks shared state. Values are replaced or appended wholesale, so a poisoned value is
/// still consistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

/// An active stream of one device's samples to the UI.
struct Stream {
    device_id: u32,
    channel: Channel<SampleBatch>,
    pending: Vec<RawSample>,
    last_send: Instant,
    rate: RateMeter,
}

impl Stream {
    fn new(device_id: u32, channel: Channel<SampleBatch>) -> Self {
        Self {
            device_id,
            channel,
            pending: Vec::with_capacity(32),
            last_send: Instant::now(),
            rate: RateMeter::new(1_000_000),
        }
    }
}

fn run(
    app: &AppHandle,
    service: &InputService,
    commands: &Receiver<Command>,
) -> Result<(), String> {
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

    let epoch = Instant::now();
    let mut stream: Option<Stream> = None;
    let mut next_tick = Instant::now();
    loop {
        loop {
            match commands.try_recv() {
                Ok(Command::Start { device_id, channel }) => {
                    stream = Some(Stream::new(device_id, channel));
                    next_tick = Instant::now();
                }
                Ok(Command::Stop) => stream = None,
                Err(TryRecvError::Empty) => break,
                // The service (and with it the app) is gone.
                Err(TryRecvError::Disconnected) => return Ok(()),
            }
        }

        // Idle: block on events. Streaming: pumping events also refreshes joystick state.
        let wait = if stream.is_some() {
            None
        } else {
            Some(EVENT_WAIT)
        };
        if handle_events(&mut events, &joysticks, &mut open, wait) {
            service.publish(app, snapshot_of(&open));
        }

        let Some(active) = stream.as_mut() else {
            continue;
        };
        let Some(joystick) = open.get(&active.device_id) else {
            // Unplugged; the UI learns about it from `devices-changed`.
            stream = None;
            continue;
        };
        let t_us = u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        active.pending.push(read_sample(joystick, t_us));
        active.rate.record(t_us);

        if active.last_send.elapsed() >= BATCH_INTERVAL && !send_batch(active, service, epoch) {
            // The webview dropped the channel (e.g. a page reload).
            stream = None;
            continue;
        }

        next_tick += POLL_INTERVAL;
        let now = Instant::now();
        if next_tick > now {
            thread::sleep(next_tick - now);
        } else if now - next_tick > MAX_LAG {
            next_tick = now;
        }
    }
}

/// Handles pending SDL events, first waiting up to `wait` for one. Returns whether the device
/// list changed.
fn handle_events(
    events: &mut EventPump,
    joysticks: &JoystickSubsystem,
    open: &mut HashMap<u32, Joystick>,
    wait: Option<Duration>,
) -> bool {
    let first = match wait {
        Some(timeout) => match events.wait_event_timeout(timeout) {
            Some(event) => Some(event),
            None => return false,
        },
        None => None,
    };
    let mut changed = false;
    for event in first.into_iter().chain(events.poll_iter()) {
        match event {
            Event::JoyDeviceAdded { which, .. } => {
                changed |= open_joystick(joysticks, open, which);
            }
            Event::JoyDeviceRemoved { which, .. } => {
                changed |= open.remove(&u32::from(which)).is_some();
            }
            _ => {}
        }
    }
    changed
}

fn read_sample(joystick: &Joystick, t_us: u64) -> RawSample {
    let mut axes = [0_i16; MAX_AXES];
    let count = joystick
        .num_axes()
        .min(u32::try_from(MAX_AXES).unwrap_or(u32::MAX));
    for (axis, value) in (0..count).zip(axes.iter_mut()) {
        *value = joystick.axis(axis).unwrap_or(0);
    }
    RawSample::new(t_us, &axes[..count as usize])
}

/// Sends the pending samples. Returns `false` if the UI side of the channel is gone.
fn send_batch(stream: &mut Stream, service: &InputService, epoch: Instant) -> bool {
    stream.last_send = Instant::now();
    let Some(oldest) = stream.pending.first() else {
        return true;
    };
    let now_us = u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
    #[expect(
        clippy::cast_precision_loss,
        reason = "batch ages are a few milliseconds; f32 is exact enough"
    )]
    let batch_age_ms = now_us.saturating_sub(oldest.t_us) as f32 / 1000.0;
    lock(&service.recent).extend_from_slice(&stream.pending);
    let batch = SampleBatch {
        samples: std::mem::take(&mut stream.pending),
        stats: StreamStats {
            sample_rate_hz: stream.rate.rate_hz(),
            batch_age_ms,
        },
    };
    stream.channel.send(batch).is_ok()
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
