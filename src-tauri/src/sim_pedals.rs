//! Dev-only simulated pedals: an SDL3 virtual joystick that goes through the real device path.
//!
//! The virtual device shows up in the device list like hardware, so the wizard, profiles,
//! streaming and drills can be tried without pedals. It is created on the input thread (SDL
//! joysticks aren't `Send`); the UI only writes [`SimControl`], which the thread reads once per
//! loop pass.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI16, Ordering};

use sdl3::JoystickSubsystem;
use sdl3::gamepad::Axis;
use sdl3::joystick::{
    Joystick, JoystickType, VirtualJoystickConnection, VirtualJoystickDescription,
};

/// Device name. It feeds the SDL GUID, so renaming it orphans saved profiles of the simulator.
pub const SIM_NAME: &str = "SCT Simulated Pedals";

/// Axis order of the virtual device.
const AXES: [Axis; 3] = [Axis::LeftX, Axis::LeftY, Axis::RightX];
const THROTTLE: usize = 0;
const BRAKE: usize = 1;
const CLUTCH: usize = 2;

/// Released pedal.
const REST: i16 = i16::MIN;

/// Length of the automatic cycle, as in `src/lib/pedals/mockSource.ts`.
const CYCLE_MS: u16 = 2650;
const BRAKE_RAMP_MS: u16 = 150;
const BRAKE_TRAIL_MS: u16 = 1500;
const THROTTLE_RAMP_MS: u16 = 400;
/// The clutch is pressed and released once per cycle, while the brake rests.
const CLUTCH_START_MS: u16 = 1750;
const CLUTCH_PULSE_MS: u16 = 400;

/// What the UI wants the simulated pedals to do. Written by Tauri commands, read by the input
/// thread.
pub struct SimControl {
    targets: [AtomicI16; 3],
    auto: AtomicBool,
}

impl SimControl {
    #[must_use]
    pub fn new() -> Self {
        Self {
            targets: [REST; 3].map(AtomicI16::new),
            auto: AtomicBool::new(false),
        }
    }

    /// Sets the manual targets (0 to 1 per pedal) and whether the automatic cycle drives them.
    pub fn set(&self, fractions: [f32; 3], auto: bool) {
        for (target, fraction) in self.targets.iter().zip(fractions) {
            target.store(fraction_to_raw(fraction), Ordering::Relaxed);
        }
        self.auto.store(auto, Ordering::Relaxed);
    }

    /// Returns the manual targets and the auto flag.
    #[must_use]
    pub fn read(&self) -> ([i16; 3], bool) {
        (
            [0, 1, 2].map(|i| self.targets[i].load(Ordering::Relaxed)),
            self.auto.load(Ordering::Relaxed),
        )
    }
}

impl Default for SimControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `SCT_SIM_PEDALS=1` asks for the simulated device.
#[must_use]
pub fn enabled_by_env() -> bool {
    std::env::var("SCT_SIM_PEDALS").is_ok_and(|v| v == "1")
}

/// Maps a pedal fraction to a raw axis value. `NaN` counts as released; the rest is clamped to
/// 0 to 1.
#[must_use]
pub fn fraction_to_raw(fraction: f32) -> i16 {
    if fraction.is_nan() {
        return REST;
    }
    let steps = (fraction.clamp(0.0, 1.0) * 65535.0).round();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped to 0..=65535, which fits an i32"
    )]
    let steps = steps as i32;
    i16::try_from(steps - 32768).unwrap_or(REST)
}

/// Raw axis values `[throttle, brake, clutch]` of the automatic cycle at `t_us` microseconds.
///
/// Throttle and brake follow `generatePedalSample` in `mockSource.ts`. The clutch starts at rest
/// and gets one short triangular pulse per cycle.
#[must_use]
pub fn waveform(t_us: u64) -> [i16; 3] {
    let ms = u16::try_from(t_us / 1000 % u64::from(CYCLE_MS)).unwrap_or(0);
    let ms_f = f32::from(ms);
    let (brake, throttle) = if ms < BRAKE_RAMP_MS {
        (
            0.9 * ms_f / f32::from(BRAKE_RAMP_MS),
            (1.0 - ms_f / 50.0).max(0.0),
        )
    } else if ms < BRAKE_RAMP_MS + BRAKE_TRAIL_MS {
        let p = f32::from(ms - BRAKE_RAMP_MS) / f32::from(BRAKE_TRAIL_MS);
        (0.9 * (1.0 - p), 0.0)
    } else {
        let rest = f32::from(ms - (BRAKE_RAMP_MS + BRAKE_TRAIL_MS));
        (0.0, (rest / f32::from(THROTTLE_RAMP_MS)).min(1.0))
    };
    let clutch = match ms.checked_sub(CLUTCH_START_MS) {
        Some(into) if into < CLUTCH_PULSE_MS => {
            let half = f32::from(CLUTCH_PULSE_MS) / 2.0;
            1.0 - (f32::from(into) - half).abs() / half
        }
        _ => 0.0,
    };
    let mut raw = [REST; 3];
    raw[THROTTLE] = fraction_to_raw(throttle);
    raw[BRAKE] = fraction_to_raw(brake);
    raw[CLUTCH] = fraction_to_raw(clutch);
    raw
}

/// The virtual device, owned by the input thread.
pub struct SimPedals {
    // Field order matters: the handle is dropped before the connection detaches the device.
    joystick: Joystick,
    _connection: VirtualJoystickConnection,
    control: Arc<SimControl>,
    current: [i16; 3],
}

impl SimPedals {
    /// Attaches the virtual device with all pedals released.
    pub fn attach(joysticks: &JoystickSubsystem, control: Arc<SimControl>) -> Result<Self, String> {
        let description = VirtualJoystickDescription::new()
            .name(SIM_NAME)
            .joystick_type(JoystickType::Wheel)
            .with_axes(AXES);
        let connection = joysticks
            .attach_virtual_joystick(description)
            .map_err(|e| format!("failed to attach virtual joystick: {e}"))?;
        let joystick = joysticks
            .open(connection.id())
            .map_err(|e| format!("failed to open virtual joystick: {e}"))?;
        for axis in (0_u32..).take(AXES.len()) {
            joystick
                .set_virtual_axis(axis, REST)
                .map_err(|e| format!("failed to rest virtual axis {axis}: {e}"))?;
        }
        Ok(Self {
            joystick,
            _connection: connection,
            control,
            current: [REST; 3],
        })
    }

    /// Moves the axes to their target at `t_us`, touching only those that changed.
    /// Runs every loop pass, so it neither allocates nor fails loudly: an axis that SDL
    /// refused is simply retried on the next pass.
    pub fn tick(&mut self, t_us: u64) {
        let (manual, auto) = self.control.read();
        let target = if auto { waveform(t_us) } else { manual };
        for (axis, (current, wanted)) in (0_u32..).zip(self.current.iter_mut().zip(target)) {
            if *current != wanted && self.joystick.set_virtual_axis(axis, wanted).is_ok() {
                *current = wanted;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sct_core::device::usb_ids_from_guid;

    #[test]
    fn fraction_to_raw_edges() {
        assert_eq!(fraction_to_raw(0.0), i16::MIN);
        assert_eq!(fraction_to_raw(1.0), i16::MAX);
        assert_eq!(fraction_to_raw(-3.0), i16::MIN);
        assert_eq!(fraction_to_raw(7.0), i16::MAX);
        assert_eq!(fraction_to_raw(f32::NAN), REST);
        assert_eq!(fraction_to_raw(f32::INFINITY), i16::MAX);
        assert_eq!(fraction_to_raw(f32::NEG_INFINITY), i16::MIN);
        let mid = fraction_to_raw(0.5);
        assert!((-1..=0).contains(&mid), "mid was {mid}");
    }

    #[test]
    fn waveform_starts_at_rest_and_repeats() {
        let start = waveform(0);
        assert_eq!(start[CLUTCH], REST);
        assert_eq!(start[BRAKE], REST);
        assert_eq!(start[THROTTLE], i16::MAX);
        for t_ms in (0..2650).step_by(50) {
            let t_us = t_ms * 1000;
            assert_eq!(waveform(t_us), waveform(t_us + 2_650_000));
        }
    }

    #[test]
    fn waveform_covers_the_cycle() {
        let (mut brake_max, mut clutch_max) = (i16::MIN, i16::MIN);
        for t_ms in 0..2650 {
            let raw = waveform(t_ms * 1000);
            brake_max = brake_max.max(raw[BRAKE]);
            clutch_max = clutch_max.max(raw[CLUTCH]);
        }
        // Brake peaks at 90 %, the clutch pulse reaches full travel.
        assert!(brake_max > fraction_to_raw(0.85) && brake_max < fraction_to_raw(0.95));
        assert!(clutch_max > fraction_to_raw(0.95));
    }

    #[test]
    fn control_round_trip() {
        let control = SimControl::new();
        assert_eq!(control.read(), ([REST; 3], false));
        control.set([1.0, 0.0, f32::NAN], true);
        assert_eq!(control.read(), ([i16::MAX, i16::MIN, REST], true));
    }

    #[test]
    fn virtual_device_round_trip() {
        let sdl = sdl3::init().unwrap();
        let joysticks = sdl.joystick().unwrap();
        let control = Arc::new(SimControl::new());
        let mut sim = SimPedals::attach(&joysticks, Arc::clone(&control)).unwrap();
        assert_eq!(sim.joystick.num_axes(), 3);
        assert!(sim.joystick.is_virtual());
        let guid = sim.joystick.guid().string();
        assert!(guid.starts_with("ff00"), "guid was {guid}");
        assert_eq!(usb_ids_from_guid(&guid), None);

        control.set([1.0, 0.5, 0.0], false);
        sim.tick(0);
        joysticks.update();
        assert_eq!(sim.joystick.axis(0).unwrap(), i16::MAX);
        assert_eq!(sim.joystick.axis(1).unwrap(), fraction_to_raw(0.5));
        assert_eq!(sim.joystick.axis(2).unwrap(), i16::MIN);

        drop(sim);
        let again = SimPedals::attach(&joysticks, control).unwrap();
        assert_eq!(again.joystick.guid().string(), guid);
    }
}
