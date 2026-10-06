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
