# ADR-0002: Pedal input via SDL3, DirectInput fallback

- **Status:** Accepted. The decision is final for now, but it still needs to be checked with real hardware (SCT-010 and SCT-012).
- **Date:** 2026-10-06

## Context

Braking Lab, a browser-based trainer, **did not detect the user's VNM pedals**. Browser trainers have three known problems:

- The Gamepad API only exposes certain HID usages, so some pedals never show up.
- It updates about once per frame (~60 Hz).
- A device only appears after a button press or axis movement.

## Decision

- **Primary:** the SDL3 joystick subsystem, through the [`sdl3`](https://docs.rs/sdl3) crate.
  - It needs no SDL window.
  - It combines RawInput, HIDAPI, DirectInput and WGI.
- **Fallback:** DirectInput8, through the `windows` crate.
  - iRacing itself reads pedals through DirectInput.
  - The fallback can be switched on in settings.
- **Input thread:**
  - Runs on a dedicated thread, polling at about 1 kHz.
  - Stamps every sample with a monotonic QPC timestamp.
  - Pushes samples into a ring buffer.

## Consequences

- SDL3 gets bundled. Either it is linked statically through the crate's `build-from-source` / `static-link` features, or the DLL ships alongside the app.
- The DirectInput backend is built only if the SCT-010 spike shows SDL3 misses devices. Otherwise it stays post-MVP (SCT-016).
- Device profiles are keyed by the SDL joystick GUID, or the DirectInput instance GUID when the fallback is in use.
