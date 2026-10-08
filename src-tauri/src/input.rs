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

use crate::audio::AudioFeedback;
use sct_core::audio_map::{SILENT, ToneTracker};
use sct_core::axis_detect::{AxisDetector, Detection};
use sct_core::calibration::RangeCapture;
use sct_core::device::{DeviceInfo, DevicesSnapshot, usb_ids_from_guid};
use sct_core::input::{MAX_AXES, RawSample};
use sct_core::profile::{DeviceKey, DeviceProfile, Pedal, ProfileStore};
use sct_core::ring_buffer::RingBuffer;
use sct_core::stream::{PedalFrame, RateMeter, SampleBatch, StreamStats};

use sct_core::drill_engine::{DrillEvent, DrillRun, Phase};
use sct_core::preset::Drill;
use sct_core::scoring::ValueSample;

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
        token: u64,
        device_id: u32,
        channel: Channel<SampleBatch>,
    },
    /// Stops the stream only if it is still the one started with `token`, so a late stop from
    /// a previous page can't kill a newer stream.
    Stop {
        token: u64,
    },
    /// Starts a drill on the stream started with `token`. The outcome goes to `reply`.
    StartDrill {
        token: u64,
        drill: Drill,
        channel: Channel<DrillEvent>,
        reply: Sender<Result<(), String>>,
    },
    AbortDrill {
        token: u64,
    },
}

/// How long `start_drill` waits for the input thread to answer.
const DRILL_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// A drill running on the input thread.
///
/// Dropping it ends the UI's run: unless the engine already sent its own `SetFinished`, the
/// drop sends one with the summary of the reps scored so far. That covers every way a stream
/// can end, panics and unwinding included.
struct ActiveDrill {
    run: DrillRun,
    channel: Channel<DrillEvent>,
    /// Profile copied when the drill started, so the per-sample path takes no lock.
    profile: DeviceProfile,
    /// The UI already got the terminal `SetFinished`.
    finished: bool,
    /// Audio feedback output; `None` when the app runs without it (and in tests).
    audio: Option<AudioFeedback>,
    tone: ToneTracker,
}

impl Drop for ActiveDrill {
    fn drop(&mut self) {
        if let Some(audio) = &self.audio {
            audio.update(SILENT);
        }
        if !self.finished {
            let summary = self.run.abort();
            let _ = self.channel.send(DrillEvent::SetFinished { summary });
        }
    }
}

/// The stream the UI last asked for, shared with the input thread.
#[derive(Default)]
struct ActiveStream {
    token: u64,
    device_id: Option<u32>,
    /// Profile applied to this stream's samples.
    profile: Option<DeviceProfile>,
}

/// Shared handle to the input thread's state, managed by Tauri.
#[derive(Clone)]
pub struct InputService {
    snapshot: Arc<Mutex<DevicesSnapshot>>,
    recent: Arc<Mutex<RingBuffer<RawSample>>>,
    commands: Sender<Command>,
    /// Saved device profiles; `None` if the database couldn't be opened.
    store: Arc<Mutex<Option<ProfileStore>>>,
    active: Arc<Mutex<ActiveStream>>,
    /// Audio feedback handed to each drill.
    audio: Option<AudioFeedback>,
}

impl InputService {
    /// Spawns the input thread. Failures are reported through the snapshot's `error`.
    pub fn spawn(
        app: AppHandle,
        store: Option<ProfileStore>,
        audio: Option<AudioFeedback>,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let service = Self {
            snapshot: Arc::default(),
            recent: Arc::new(Mutex::new(RingBuffer::with_capacity(RECENT_CAPACITY))),
            commands,
            store: Arc::new(Mutex::new(store)),
            active: Arc::default(),
            audio,
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
    /// Returns a token that `stop_stream` needs, so a stale stop can't end a newer stream.
    pub fn start_stream(
        &self,
        device_id: u32,
        channel: Channel<SampleBatch>,
    ) -> Result<u64, String> {
        let profile = self.load_profile(device_id)?;
        let token = {
            let mut active = lock(&self.active);
            let token = active.token + 1;
            *active = ActiveStream {
                token,
                device_id: Some(device_id),
                profile,
            };
            token
        };
        self.send(Command::Start {
            token,
            device_id,
            channel,
        })?;
        Ok(token)
    }

    /// Loads the saved profile of a connected device.
    pub fn load_profile(&self, device_id: u32) -> Result<Option<DeviceProfile>, String> {
        let key = self.device_key(device_id)?;
        match lock(&self.store).as_ref() {
            Some(store) => store.load(&key).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    /// Saves a device's profile and applies it right away if that device is streaming.
    pub fn save_profile(&self, device_id: u32, profile: DeviceProfile) -> Result<(), String> {
        for pedal in Pedal::ALL {
            if let Some(axis) = profile.get(pedal) {
                axis.calibration
                    .validate()
                    .map_err(|e| format!("{pedal:?}: {e}"))?;
            }
        }
        let key = self.device_key(device_id)?;
        lock(&self.store)
            .as_ref()
            .ok_or("profile storage is unavailable")?
            .save(&key, &profile)
            .map_err(|e| e.to_string())?;
        self.activate_if_streaming(device_id, Some(profile));
        Ok(())
    }

    /// Deletes a device's saved profile. Returns whether one existed.
    pub fn reset_profile(&self, device_id: u32) -> Result<bool, String> {
        let key = self.device_key(device_id)?;
        let deleted = lock(&self.store)
            .as_ref()
            .ok_or("profile storage is unavailable")?
            .delete(&key)
            .map_err(|e| e.to_string())?;
        self.activate_if_streaming(device_id, None);
        Ok(deleted)
    }

    /// Ids of connected devices that have a saved profile, in device-list order.
    pub fn profiled_devices(&self) -> Vec<u32> {
        let store = lock(&self.store);
        let Some(store) = store.as_ref() else {
            return Vec::new();
        };
        self.snapshot()
            .devices
            .iter()
            .filter(|d| matches!(store.load(&key_of(d)), Ok(Some(_))))
            .map(|d| d.id)
            .collect()
    }

    fn device_key(&self, device_id: u32) -> Result<DeviceKey, String> {
        self.snapshot()
            .devices
            .iter()
            .find(|d| d.id == device_id)
            .map(key_of)
            .ok_or_else(|| format!("device {device_id} is not connected"))
    }

    fn activate_if_streaming(&self, device_id: u32, profile: Option<DeviceProfile>) {
        let mut active = lock(&self.active);
        if active.device_id == Some(device_id) {
            active.profile = profile;
        }
    }

    /// Stops the stream started with `token`, if it is still the active one.
    pub fn stop_stream(&self, token: u64) -> Result<(), String> {
        {
            let mut active = lock(&self.active);
            if active.token == token {
                active.device_id = None;
                active.profile = None;
            }
        }
        self.send(Command::Stop { token })
    }

    /// Starts `drill` on the stream started with `token`, feeding events to `channel`.
    /// Fails if that stream isn't active or its device has no profile.
    pub fn start_drill(
        &self,
        token: u64,
        drill: Drill,
        channel: Channel<DrillEvent>,
    ) -> Result<(), String> {
        let (reply, answer) = mpsc::channel();
        self.send(Command::StartDrill {
            token,
            drill,
            channel,
            reply,
        })?;
        answer
            .recv_timeout(DRILL_REPLY_TIMEOUT)
            .map_err(|_| "input thread did not answer".to_owned())?
    }

    /// Aborts the drill running on the stream started with `token`, if any.
    pub fn abort_drill(&self, token: u64) -> Result<(), String> {
        self.send(Command::AbortDrill { token })
    }

    /// Runs axis detection over the streamed samples taken at or after `since_us`.
    ///
    /// The first of those samples is the baseline, so the pedals must be at rest at `since_us`.
    pub fn detect_axis(&self, since_us: u64, exclude: &[usize]) -> Detection {
        let recent = lock(&self.recent);
        let mut samples = recent.iter().filter(|s| s.t_us >= since_us).peekable();
        let Some(first) = samples.peek() else {
            return Detection::NoMovement;
        };
        let mut detector = AxisDetector::new(usize::from(first.axis_count));
        for sample in samples {
            detector.observe(sample.axes());
        }
        detector.result(exclude)
    }

    /// Returns the `(min, max)` raw range of `axis` over the samples taken at or after `since_us`.
    pub fn capture_range(&self, axis: usize, since_us: u64) -> Option<(i16, i16)> {
        let mut capture = RangeCapture::new();
        for sample in lock(&self.recent).iter().filter(|s| s.t_us >= since_us) {
            if let Some(&raw) = sample.axes().get(axis) {
                capture.observe(raw);
            }
        }
        capture.range()
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
    token: u64,
    device_id: u32,
    channel: Channel<SampleBatch>,
    pending: Vec<RawSample>,
    last_send: Instant,
    rate: RateMeter,
    active_drill: Option<ActiveDrill>,
}

impl Stream {
    fn new(token: u64, device_id: u32, channel: Channel<SampleBatch>) -> Self {
        Self {
            token,
            device_id,
            channel,
            pending: Vec::with_capacity(32),
            last_send: Instant::now(),
            rate: RateMeter::new(1_000_000),
            active_drill: None,
        }
    }

    /// Ends the running drill, if any. Dropping it tells the UI with a final `SetFinished`.
    fn finish_drill(&mut self) {
        self.active_drill = None;
    }

    /// Feeds one sample to the running drill and forwards its events.
    fn step_drill(&mut self, sample: &RawSample) {
        let Some(drill) = self.active_drill.as_mut() else {
            return;
        };
        let frame = PedalFrame::from_sample(&drill.profile, sample);
        let value = pedal_value(&frame, drill.run.drill().pedal);
        for event in drill.run.push(ValueSample::new(sample.t_us, value)) {
            let _ = drill.channel.send(event);
        }
        if let Some(audio) = &drill.audio {
            // Tone only while a rep is active: silent in the countdown and the rest pause.
            let target = matches!(drill.run.phase(), Phase::Active { .. })
                .then(|| drill.run.target_at(sample.t_us))
                .flatten();
            let spec = drill.run.drill();
            let step = drill
                .tone
                .step(target, value, spec.tolerance_fraction(), &spec.kind);
            audio.update(step.tone);
            if step.chime {
                audio.chime();
            }
        }
        if matches!(drill.run.phase(), Phase::Finished) {
            // The engine sent `SetFinished` itself.
            drill.finished = true;
            self.active_drill = None;
        }
    }
}

/// Ends `stream`, first sending the UI the terminal event of a running drill.
fn end_stream(stream: &mut Option<Stream>) {
    if let Some(mut ended) = stream.take() {
        ended.finish_drill();
    }
}

/// The calibrated value (0 to 1) of `pedal` in `frame`.
fn pedal_value(frame: &PedalFrame, pedal: Pedal) -> f32 {
    match pedal {
        Pedal::Brake => frame.brake,
        Pedal::Throttle => frame.throttle,
        Pedal::Clutch => frame.clutch,
    }
}

/// Why a drill can't start on `stream` with `profile`, if it can't.
fn check_start(
    stream: Option<&mut Stream>,
    profile: Option<DeviceProfile>,
    pedal: Pedal,
) -> Result<(&mut Stream, DeviceProfile), String> {
    let stream = stream.ok_or("no active pedal stream; connect the pedals first")?;
    let profile = profile.ok_or("the device has no saved profile; calibrate it first")?;
    if profile.get(pedal).is_none() {
        let name = format!("{pedal:?}").to_lowercase();
        return Err(format!(
            "the {name} pedal isn't assigned; set it up on the Devices page"
        ));
    }
    Ok((stream, profile))
}

/// Starts `drill` on `stream`, replacing a running one, and answers on `reply`.
///
/// The answer goes out before any event. If nobody is waiting for it any more (the caller
/// timed out), the drill is not installed and no event is sent.
fn start_drill(
    stream: Option<&mut Stream>,
    profile: Option<DeviceProfile>,
    t_us: u64,
    drill: Drill,
    channel: Channel<DrillEvent>,
    audio: Option<AudioFeedback>,
    reply: &Sender<Result<(), String>>,
) {
    let (stream, profile) = match check_start(stream, profile, drill.pedal) {
        Ok(checked) => checked,
        Err(error) => {
            let _ = reply.send(Err(error));
            return;
        }
    };
    if reply.send(Ok(())).is_err() {
        return;
    }
    stream.finish_drill();
    let mut run = DrillRun::new(drill, sct_core::drill_engine::DEFAULT_REST_MS);
    for event in run.start(t_us) {
        let _ = channel.send(event);
    }
    stream.active_drill = Some(ActiveDrill {
        run,
        channel,
        profile,
        finished: false,
        audio,
        tone: ToneTracker::default(),
    });
}

/// The profile of the stream started with `token`, if it is still the active one.
fn stream_profile(service: &InputService, token: u64) -> Option<DeviceProfile> {
    let active = lock(&service.active);
    if active.token == token {
        active.profile.clone()
    } else {
        None
    }
}

/// Handles all queued commands. Returns `false` once the service is gone.
fn drain_commands(
    commands: &Receiver<Command>,
    service: &InputService,
    epoch: Instant,
    stream: &mut Option<Stream>,
    next_tick: &mut Instant,
) -> bool {
    loop {
        match commands.try_recv() {
            Ok(Command::Start {
                token,
                device_id,
                channel,
            }) => {
                end_stream(stream);
                // Cleared here, not in `start_stream`, so no sample of the previous
                // stream can land after the clear.
                lock(&service.recent).clear();
                *stream = Some(Stream::new(token, device_id, channel));
                *next_tick = Instant::now();
            }
            Ok(Command::Stop { token }) => {
                if stream.as_ref().is_some_and(|s| s.token == token) {
                    end_stream(stream);
                }
            }
            Ok(Command::StartDrill {
                token,
                drill,
                channel,
                reply,
            }) => {
                let t_us = u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
                start_drill(
                    stream.as_mut().filter(|s| s.token == token),
                    stream_profile(service, token),
                    t_us,
                    drill,
                    channel,
                    service.audio.clone(),
                    &reply,
                );
            }
            Ok(Command::AbortDrill { token }) => {
                if let Some(active) = stream.as_mut().filter(|s| s.token == token) {
                    active.finish_drill();
                }
            }
            Err(TryRecvError::Empty) => return true,
            // The service (and with it the app) is gone.
            Err(TryRecvError::Disconnected) => {
                end_stream(stream);
                return false;
            }
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
        if !drain_commands(commands, service, epoch, &mut stream, &mut next_tick) {
            return Ok(());
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
            end_stream(&mut stream);
            continue;
        };
        let t_us = u64::try_from(epoch.elapsed().as_micros()).unwrap_or(u64::MAX);
        let raw_sample = read_sample(joystick, t_us);
        active.pending.push(raw_sample);
        active.rate.record(t_us);
        active.step_drill(&raw_sample);

        if active.last_send.elapsed() >= BATCH_INTERVAL && !send_batch(active, service, epoch) {
            // The webview dropped the channel (e.g. a page reload).
            end_stream(&mut stream);
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
    let frames = {
        let active = lock(&service.active);
        match &active.profile {
            Some(profile) if active.token == stream.token => stream
                .pending
                .iter()
                .map(|s| PedalFrame::from_sample(profile, s))
                .collect(),
            _ => Vec::new(),
        }
    };
    if let Some(drill) = &stream.active_drill {
        send_audio_feedback(drill, &frames);
    }
    let batch = SampleBatch {
        samples: std::mem::take(&mut stream.pending),
        frames,
        stats: StreamStats {
            sample_rate_hz: stream.rate.rate_hz(),
            batch_age_ms,
        },
    };
    stream.channel.send(batch).is_ok()
}

/// Flipped on by SCT-038 once the audio hook does something.
const AUDIO_FEEDBACK_ENABLED: bool = false;

/// Passes the signed error and in-band flag of each frame to the audio hook.
fn send_audio_feedback(drill: &ActiveDrill, frames: &[PedalFrame]) {
    // TODO(SCT-038): the hook is a stub, so compute nothing until audio is wired.
    if !AUDIO_FEEDBACK_ENABLED {
        return;
    }
    let tolerance = drill.run.drill().tolerance_fraction();
    let pedal = drill.run.drill().pedal;
    let audio_data: Vec<(f32, bool)> = frames
        .iter()
        .filter_map(|frame| {
            let err = pedal_value(frame, pedal) - drill.run.target_at(frame.t_us)?;
            Some((err, err.abs() <= tolerance))
        })
        .collect();
    if !audio_data.is_empty() {
        audio_feedback_hook(&audio_data);
    }
}

/// TODO(SCT-038): Audio feedback hook.
/// Called with the signed error and in-band flag for each sample batch during a drill.
fn audio_feedback_hook(_samples: &[(f32, bool)]) {
    // Intentionally blank for SCT-038
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

fn key_of(device: &DeviceInfo) -> DeviceKey {
    DeviceKey::new(device.guid.clone(), device.axis_count, device.button_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sct_core::calibration::AxisCalibration;
    use sct_core::preset::DrillKind;
    use sct_core::profile::PedalAxis;
    use tauri::ipc::InvokeResponseBody;

    type Log = Arc<Mutex<Vec<String>>>;

    fn event_channel() -> (Channel<DrillEvent>, Log) {
        let log: Log = Arc::default();
        let sink = Arc::clone(&log);
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                sink.lock().unwrap().push(json);
            }
            Ok(())
        });
        (channel, log)
    }

    fn finished_count(log: &Log) -> usize {
        log.lock()
            .unwrap()
            .iter()
            .filter(|e| e.contains("setFinished"))
            .count()
    }

    fn stream() -> Stream {
        Stream::new(1, 1, Channel::new(|_| Ok(())))
    }

    fn brake_profile() -> DeviceProfile {
        let mut profile = DeviceProfile::default();
        profile.set(
            Pedal::Brake,
            Some(PedalAxis {
                axis: 0,
                calibration: AxisCalibration::default(),
            }),
        );
        profile
    }

    fn drill(pedal: Pedal) -> Drill {
        Drill {
            id: "d".into(),
            name: "D".into(),
            pedal,
            reps: 2,
            lead_in_ms: 1000,
            tolerance: 5.0,
            kind: DrillKind::Hold {
                target: 70.0,
                hold_ms: 1000,
            },
        }
    }

    /// Starts a drill and returns its event log and whether the caller got `Ok`.
    fn start(stream: &mut Stream, pedal: Pedal) -> (Log, Result<(), String>) {
        let (channel, log) = event_channel();
        let (reply, answer) = mpsc::channel();
        start_drill(
            Some(stream),
            Some(brake_profile()),
            0,
            drill(pedal),
            channel,
            None,
            &reply,
        );
        (log, answer.recv().unwrap())
    }

    #[test]
    fn abort_sends_one_terminal_event() {
        let mut stream = stream();
        let (log, result) = start(&mut stream, Pedal::Brake);
        assert_eq!(result, Ok(()));
        assert!(log.lock().unwrap()[0].contains("countdownStarted"));
        stream.finish_drill();
        stream.finish_drill();
        drop(stream);
        assert_eq!(finished_count(&log), 1);
    }

    #[test]
    fn replacing_start_ends_the_old_drill_once() {
        let mut stream = stream();
        let (first, _) = start(&mut stream, Pedal::Brake);
        let (second, result) = start(&mut stream, Pedal::Brake);
        assert_eq!(result, Ok(()));
        assert_eq!(finished_count(&first), 1);
        assert_eq!(finished_count(&second), 0);
        drop(stream);
        assert_eq!(finished_count(&first), 1);
        assert_eq!(finished_count(&second), 1);
    }

    #[test]
    fn end_stream_sends_one_terminal_event() {
        let mut slot = Some(stream());
        let (log, _) = start(slot.as_mut().unwrap(), Pedal::Brake);
        end_stream(&mut slot);
        assert!(slot.is_none());
        assert_eq!(finished_count(&log), 1);
    }

    #[test]
    fn unmapped_pedal_is_rejected_and_keeps_the_running_drill() {
        let mut stream = stream();
        let (running, _) = start(&mut stream, Pedal::Brake);
        let (rejected, result) = start(&mut stream, Pedal::Throttle);
        assert_eq!(
            result,
            Err("the throttle pedal isn't assigned; set it up on the Devices page".to_owned())
        );
        assert!(rejected.lock().unwrap().is_empty());
        assert_eq!(finished_count(&running), 0);
    }

    #[test]
    fn missing_stream_or_profile_is_an_error() {
        let (channel, log) = event_channel();
        let (reply, answer) = mpsc::channel();
        start_drill(None, None, 0, drill(Pedal::Brake), channel, None, &reply);
        assert!(answer.recv().unwrap().is_err());

        let mut stream = stream();
        let (channel, _) = event_channel();
        start_drill(
            Some(&mut stream),
            None,
            0,
            drill(Pedal::Brake),
            channel,
            None,
            &reply,
        );
        assert!(answer.recv().unwrap().is_err());
        assert!(log.lock().unwrap().is_empty());
        assert!(stream.active_drill.is_none());
    }

    #[test]
    fn caller_that_gave_up_gets_no_drill_and_no_events() {
        let mut stream = stream();
        let (channel, log) = event_channel();
        let (reply, answer) = mpsc::channel();
        drop(answer);
        start_drill(
            Some(&mut stream),
            Some(brake_profile()),
            0,
            drill(Pedal::Brake),
            channel,
            None,
            &reply,
        );
        assert!(stream.active_drill.is_none());
        assert!(log.lock().unwrap().is_empty());
    }
}
