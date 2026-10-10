//! A drill runs on its own thread, so scoring a rep never delays a poll of the input thread.
//!
//! The input thread queues each sample. While a rep is scored, samples wait in the queue.
//! `DrillRun` uses sample timestamps only, so handling them late gives the same events and
//! scores.

use std::io;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread;

use crate::audio::AudioFeedback;
use sct_core::audio_map::ToneTracker;
use sct_core::drill_engine::{DrillEvent, DrillRun, Phase};
use sct_core::scoring::ValueSample;
use tauri::ipc::Channel;

/// Samples the queue holds: about 4 s at 1 kHz. Scoring the longest trace (15 s) pauses the
/// drill thread for about 10 ms in a release build and 115 ms in a debug build.
const QUEUE_CAPACITY: usize = 4_096;

/// The input thread's handle to a drill running on its own thread.
///
/// Dropping it ends the drill: the drill thread handles the samples already queued, then sends
/// the UI the terminal `SetFinished` (unless the run already finished).
pub struct DrillThread {
    samples: SyncSender<ValueSample>,
}

impl DrillThread {
    /// Starts `run` at `t_us` on a new `sct-drill` thread that sends its events to `channel`
    /// and drives `audio`.
    pub fn spawn(
        run: DrillRun,
        t_us: u64,
        channel: Channel<DrillEvent>,
        audio: Option<AudioFeedback>,
    ) -> io::Result<Self> {
        let (samples, queue) = mpsc::sync_channel(QUEUE_CAPACITY);
        // If the spawn fails, std drops the closure and with it `drill`, whose drop sends the
        // UI its `SetFinished`.
        let mut drill = RunningDrill::new(run, channel, audio);
        thread::Builder::new()
            .name("sct-drill".into())
            .spawn(move || {
                drill.start(t_us);
                for sample in queue {
                    if drill.step(sample) {
                        return;
                    }
                }
            })?;
        Ok(Self { samples })
    }

    /// Queues `sample` without blocking. Returns `false` once the drill has ended or its queue
    /// is full; the caller then drops this handle.
    pub fn send(&self, sample: ValueSample) -> bool {
        match self.samples.try_send(sample) {
            Ok(()) => true,
            Err(TrySendError::Disconnected(_)) => false,
            Err(TrySendError::Full(_)) => {
                eprintln!(
                    "the drill thread fell {QUEUE_CAPACITY} samples behind; ending the drill"
                );
                false
            }
        }
    }
}

/// A drill run, owned by its drill thread.
///
/// Dropping it ends the UI's run: unless the engine already sent its own `SetFinished`, the
/// drop sends one with the summary of the reps scored so far. That covers every way a drill
/// can end, a panic on the drill thread included.
struct RunningDrill {
    run: DrillRun,
    channel: Channel<DrillEvent>,
    /// The UI already got the terminal `SetFinished`.
    finished: bool,
    /// Audio feedback output; `None` when the app runs without it (and in tests).
    audio: Option<AudioFeedback>,
    tone: ToneTracker,
}

impl RunningDrill {
    fn new(run: DrillRun, channel: Channel<DrillEvent>, audio: Option<AudioFeedback>) -> Self {
        Self {
            run,
            channel,
            finished: false,
            audio,
            tone: ToneTracker::default(),
        }
    }

    /// Starts the run at `t_us` and forwards its events.
    fn start(&mut self, t_us: u64) {
        for event in self.run.start(t_us) {
            let _ = self.channel.send(event);
        }
    }

    /// Feeds one sample to the run, forwards its events and updates the tone. Returns whether
    /// the run has finished.
    fn step(&mut self, sample: ValueSample) -> bool {
        let value = sample.value;
        for event in self.run.push(sample) {
            let _ = self.channel.send(event);
        }
        if let Some(audio) = &self.audio {
            // Beeps only while a rep is active: silent in the countdown and the rest pause.
            // The target is the nearest point of the band, so the tone measures the distance
            // outside the timing-window envelope. `max`/`min` never panic, unlike `clamp`.
            let target = matches!(self.run.phase(), Phase::Active { .. })
                .then(|| self.run.band_at(sample.t_us))
                .flatten()
                .map(|(lo, hi)| value.max(lo).min(hi));
            let rate = self
                .tone
                .step(target, value, self.run.drill().tolerance_fraction());
            audio.set_pulse_rate(rate);
        }
        if matches!(self.run.phase(), Phase::Finished) {
            // The engine sent `SetFinished` itself.
            self.finished = true;
        }
        self.finished
    }
}

impl Drop for RunningDrill {
    fn drop(&mut self) {
        if let Some(audio) = &self.audio {
            audio.set_pulse_rate(0.0);
        }
        if !self.finished {
            let summary = self.run.abort();
            let _ = self.channel.send(DrillEvent::SetFinished { summary });
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sct_core::drill_engine::DEFAULT_REST_MS;
    use sct_core::preset::{Drill, DrillKind};
    use sct_core::profile::Pedal;
    use std::sync::Mutex;
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::time::Duration;
    use tauri::ipc::InvokeResponseBody;

    /// A drill event channel and the receiving end of its JSON log. The log closes when the
    /// last clone of the channel is dropped, which for a started drill is when its thread exits.
    pub(crate) fn event_channel() -> (Channel<DrillEvent>, Receiver<String>) {
        let (sink, log) = mpsc::channel();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let _ = sink.send(json);
            }
            Ok(())
        });
        (channel, log)
    }

    /// Every event until the log closes. Panics if it stays open for 5 s.
    pub(crate) fn drain(log: &Receiver<String>) -> Vec<String> {
        let mut events = Vec::new();
        loop {
            match log.recv_timeout(Duration::from_secs(5)) {
                Ok(event) => events.push(event),
                Err(RecvTimeoutError::Disconnected) => return events,
                Err(RecvTimeoutError::Timeout) => panic!("the drill thread did not exit"),
            }
        }
    }

    /// The events logged so far.
    pub(crate) fn so_far(log: &Receiver<String>) -> Vec<String> {
        log.try_iter().collect()
    }

    /// The event type of each JSON event, e.g. `countdownStarted`.
    fn kinds(events: &[String]) -> Vec<String> {
        events
            .iter()
            .map(|e| {
                let (_, rest) = e.split_once("\"event\":\"").unwrap();
                rest.split('"').next().unwrap().to_owned()
            })
            .collect()
    }

    fn hold_drill() -> Drill {
        Drill {
            id: "d".into(),
            name: "D".into(),
            pedal: Pedal::Brake,
            reps: 2,
            lead_in_ms: 1000,
            tolerance: Some(5.0),
            decimals: None,
            kind: DrillKind::Hold {
                target: 70.0,
                hold_ms: 1000,
            },
        }
    }

    /// A 600 ms trace after a 1 s countdown.
    fn trace_drill() -> Drill {
        Drill {
            id: "t".into(),
            name: "T".into(),
            pedal: Pedal::Brake,
            reps: 1,
            lead_in_ms: 1000,
            tolerance: Some(6.0),
            decimals: None,
            kind: DrillKind::Trace {
                points: vec![(0, 0.0), (150, 90.0), (600, 0.0)],
            },
        }
    }

    /// Starts `drill` at 0 on a `RunningDrill` with a detached audio output.
    fn start_with_audio(drill: Drill) -> (RunningDrill, AudioFeedback) {
        let audio = AudioFeedback::detached();
        let (channel, _) = event_channel();
        let mut running = RunningDrill::new(
            DrillRun::new(drill, DEFAULT_REST_MS),
            channel,
            Some(audio.clone()),
        );
        running.start(0);
        (running, audio)
    }

    /// Feeds one sample per millisecond over `[from_ms, to_ms)` at `fraction` and returns the
    /// pulse rate after the last one.
    fn feed(
        drill: &mut RunningDrill,
        audio: &AudioFeedback,
        from_ms: u64,
        to_ms: u64,
        fraction: f32,
    ) -> f32 {
        for ms in from_ms..to_ms {
            drill.step(ValueSample::new(ms * 1000, fraction));
        }
        audio.pulse_rate()
    }

    #[test]
    fn beeps_while_off_target_and_silent_in_band() {
        let (mut drill, audio) = start_with_audio(hold_drill());
        // Active rep, too light: beeping.
        assert!(feed(&mut drill, &audio, 1_001, 1_010, 0.2) > 0.0);
        // Held in the band: silent.
        assert_eq!(feed(&mut drill, &audio, 1_010, 1_300, 0.70), 0.0);
        // Leaving the band: beeping again on the first sample.
        assert!(feed(&mut drill, &audio, 1_300, 1_301, 0.2) > 0.0);
    }

    #[test]
    fn beeps_faster_the_further_off() {
        let (mut drill, audio) = start_with_audio(hold_drill());
        let near = feed(&mut drill, &audio, 1_001, 1_010, 0.62);
        let far = feed(&mut drill, &audio, 1_010, 1_020, 0.20);
        assert!(near >= 3.0 && far > near, "near {near}, far {far}");
        assert!(far <= 11.0);
    }

    #[test]
    fn trace_scoring_phase_is_silent() {
        // A 600 ms trace after a 1 s countdown: active from 1.0 s to 1.6 s, then scoring until
        // 1.9 s (TRACE_LAG_MARGIN_MS after the active window).
        let (mut drill, audio) = start_with_audio(trace_drill());
        // At 80% in the first 100 ms of the ramp the pedal is 14 to 80 points above the
        // instant target (tolerance 6) but inside the ±150 ms band, which reaches the 90%
        // peak. Silent, so the cue follows the band and not the instant target.
        assert_eq!(feed(&mut drill, &audio, 1_001, 1_100, 0.80), 0.0);
        // Above the whole timing-window band during the active window: beeping.
        assert!(feed(&mut drill, &audio, 1_100, 1_500, 1.0) > 0.0);
        // Off target in the scoring phase, where the target is no longer shown: silent.
        assert_eq!(feed(&mut drill, &audio, 1_500, 1_700, 1.0), 0.0);
        let phase = drill.run.phase();
        assert!(matches!(phase, Phase::Scoring { .. }), "{phase:?}");
    }

    #[test]
    fn no_beeps_outside_the_active_phase() {
        let (mut drill, audio) = start_with_audio(hold_drill());
        // Countdown: far off target, still silent.
        assert_eq!(feed(&mut drill, &audio, 100, 900, 0.2), 0.0);
        assert_eq!(feed(&mut drill, &audio, 900, 1_000, 0.70), 0.0);
    }

    #[test]
    fn every_drill_end_silences_the_beeps() {
        // Abort, or replaced by a new drill: the run is dropped.
        let (mut drill, audio) = start_with_audio(hold_drill());
        assert!(feed(&mut drill, &audio, 1_001, 1_100, 0.2) > 0.0);
        drop(drill);
        assert_eq!(audio.pulse_rate(), 0.0);

        // Finish: run the whole drill off target (1 s countdown, 1 s hold).
        let (mut drill, audio) = start_with_audio(hold_drill());
        let finished = (1_001..10_000).any(|ms| drill.step(ValueSample::new(ms * 1000, 0.2)));
        assert!(finished, "drill should have finished");
        drop(drill);
        assert_eq!(audio.pulse_rate(), 0.0);
    }

    #[test]
    fn runs_a_trace_set_in_order_on_its_own_thread() {
        let (channel, log) = event_channel();
        let thread = DrillThread::spawn(
            DrillRun::new(trace_drill(), DEFAULT_REST_MS),
            0,
            channel,
            None,
        )
        .unwrap();
        for ms in 1..=2_000 {
            if !thread.send(ValueSample::new(ms * 1000, 0.5)) {
                break;
            }
        }
        let events = drain(&log);
        assert_eq!(
            kinds(&events),
            ["countdownStarted", "repStarted", "repScored", "setFinished"]
        );
        assert!(!events[3].contains("\"summary\":null"), "{}", events[3]);
        assert!(!thread.send(ValueSample::new(2_001_000, 0.5)));
    }

    #[test]
    fn a_full_queue_refuses_samples_without_blocking() {
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let (sink, log) = mpsc::channel();
        // Holds the drill thread inside `start` until the test releases it.
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                let blocks = json.contains("countdownStarted");
                let _ = sink.send(json);
                if blocks {
                    let _ = gate.lock().unwrap().recv();
                }
            }
            Ok(())
        });
        let thread = DrillThread::spawn(
            DrillRun::new(hold_drill(), DEFAULT_REST_MS),
            0,
            channel,
            None,
        )
        .unwrap();
        for ms in 1..=QUEUE_CAPACITY as u64 {
            assert!(thread.send(ValueSample::new(ms * 1000, 0.5)), "sample {ms}");
        }
        assert!(!thread.send(ValueSample::new((QUEUE_CAPACITY as u64 + 1) * 1000, 0.5)));
        // Later countdowns find the gate closed and don't block.
        release.send(()).unwrap();
        drop(release);
        drop(thread);
        let events = drain(&log);
        let finished = events.iter().filter(|e| e.contains("setFinished")).count();
        assert_eq!(finished, 1, "{events:?}");
    }
}
