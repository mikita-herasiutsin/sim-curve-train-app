# SCT-010: SDL3 device detection on real hardware

- **Date:** 2026-10-06
- **Backend:** SDL 3.4.18, built from source and statically linked (`sdl3` crate 0.20, `build-from-source-static`)
- **Machine:** Windows 11 Pro (26300), the maintainer's sim rig
- **Ticket:** SCT-010 (main risk check of [ADR-0002](../decisions/0002-input-sdl3-directinput-fallback.md))

## Result

**SDL3 detects the VNM pedals.** Braking Lab, the browser trainer, did not detect them. The DirectInput fallback (SCT-016) is not needed for this rig and stays post-MVP.

The SDL joystick subsystem runs on its own thread with no SDL window. It needs the hint `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1`; without it, SDL treats the app as unfocused because the Tauri window isn't an SDL window.

## Devices seen

| Device (SDL name) | GUID | VID:PID | Axes | Buttons | Hats |
|---|---|---|---|---|---|
| Simulation VNM Pedal V1 | `0300fcf783040000dfa3000000000000` | `0483:a3df` | 4 | 0 | 0 |
| SIMAGIC P2000 Haptic | `0300a6a6703600000209000000000000` | `3670:0902` | 3 | 0 | 0 |
| Simulation., JSC VNM GT Steering Wheel V1 | `0300c37b83040000d9a3000000000000` | `0483:a3d9` | 3 | 64 | 0 |
| FANATEC Wheel | `0300bd14b70e00002000000000000000` | `0eb7:0020` | 8 | 108 | 1 |
| FANATEC Wheel | `0300bd14b70e00002000000000000000` | `0eb7:0020` | 12 | 63 | 4 |

## Findings

- **GUIDs are not unique per device.** The two "FANATEC Wheel" interfaces share one GUID but expose different axes and buttons. SCT-015 must not key profiles by GUID alone. Possible keys: GUID plus axis and button count, or GUID plus SDL device path. Pedal sets on this rig have unique GUIDs, so this doesn't block M1.
- The VNM pedals report **4 axes** for 3 pedals. SCT-011 (raw axis monitor) will show which axis is which, and whether the fourth is unused or a handbrake input.
- **Hot-plug works.** Unplugging and replugging the VNM pedals updated the Devices screen within about 1 s each way. SDL sends `JOYSTICK_ADDED`/`REMOVED` events, and the input thread waits on events with a 50 ms timeout.

## Follow-up: raw axes and stream (SCT-011, SCT-012)

VNM Pedal V1 axis mapping, from the raw axis monitor (each pedal moves exactly one bar):

| Axis | Pedal | Notes |
|---|---|---|
| 0 | none | Probably the handbrake input, which isn't connected on this set. Rests at -32768. |
| 1 | Throttle | |
| 2 | Brake | |
| 3 | Clutch | |

All axes rest at -32768 (0%) and rise when pressed, so none of them needs inverting.

Stream measured on this rig in `tauri dev`, with the raw monitor open:

- **Sample rate:** 1000 Hz. This is the poll rate of the input thread; the pedals' own USB report rate may be lower, so consecutive samples can repeat a value.
- **Batches:** age 7.1 ms when sent, 8.2 ms apart on average.
- **CPU:** the app process uses 0.3% of a 16-thread machine. All WebView2 processes together use 1.4%, which is an upper bound because other apps' WebViews are included. Both are well under the 3% target.

## Follow-up: live view (SCT-020, SCT-021)

Live page in `tauri dev` on a 165 Hz monitor, with the VNM profile restored automatically after an app restart:

- **Render:** 165 fps, frame time 6.1 ms avg / 6.2 ms max. The canvas keeps up with the refresh rate.
- **Stream:** 1000 Hz. Batches arrive in the webview 8.5 ms apart on average (10.2 ms max).
- Full brake and throttle presses reach the solid 100% line on the graph. Released pedals sit on 0% with the default 2% deadzones.
