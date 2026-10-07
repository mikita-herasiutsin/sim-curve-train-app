# Latency test

## Why
Measure input-to-photon latency from brake pedal press to visible flash. Target is under 50 ms at 60 Hz. Target is under 20 ms at 144 Hz.

## What you need
- SimCurveTrainApp on a Windows desktop.
- Latency check mode on the Live page.
- A phone slow-motion camera.
- A pedal with a visible 50% travel mark.
- A monitor with known refresh rate.

## Setup
- Frame both the pedal and the monitor in one shot.
- Use side lighting so the pedal travel is easy to see.
- Put a tape mark on the pedal base at the 50% position.
- Remove any frame-rate cap for the app in the GPU control panel. The app draws through WebView2, which is always V-Synced; that delay is part of what this test measures.
- Run the app in a fullscreen window.
- Record the monitor refresh rate.

## Load-cell brakes

A load-cell brake (like the VNM brake) hardly moves, so you can't film it crossing a 50% mark. Use one of these instead:

- Stomp hard and fast, and count from the first frame where the foot visibly loads the pedal (the pedal face or the foot stops moving). A hard stomp reaches 50% force within a few milliseconds, so the error stays small.
- Or measure with the throttle, which is a position sensor, if the latency check is switched to the throttle.

Write down which method you used in the Notes column.

## Procedure
- Make 10 or more stomps.
- Use quick decisive presses.
- Press until the brake pedal crosses the 50% mark.
- The app flashes white for 100 ms when the pedal crosses 50% travel.

## Analysis
- Step through the video frame by frame.
- Count frames from the pedal visibly reaching the 50% mark to the first frame where the flash appears.
- Latency = frames x ms per frame.
- At 240 fps, use 4.17 ms per frame.
- At 120 fps, use 8.33 ms per frame.
- Take the median and note the min and max.
- Subtract nothing. The result includes display lag.

## Sources of error
- Camera frame quantisation adds or removes about 1 frame.
- Pedal travel versus the 50% mark can be ambiguous.
- Monitor scanout is top to bottom. Watch the top of the screen.

## Recording results
| Date | Pedals | Monitor Hz | Camera fps | Samples | Median ms | Min ms | Max ms | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
|  |  |  |  |  |  |  |  |  |
