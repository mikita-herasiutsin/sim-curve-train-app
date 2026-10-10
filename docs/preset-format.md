# Preset File Format

Preset files define collections of sim-racing pedal drills (such as threshold braking, trail-off traces, or throttle modulation) with configurable repetitions, preparation countdowns, and tolerances.

Files use JSON. The app ships its presets in the bundled `presets/` folder (importing your own files is planned, see SCT-081). All keys use `camelCase`. Values are expressed in human-readable units: pedal travel is given in percent (`0` to `100`) and durations in milliseconds (`ms`). Unknown keys are strictly rejected.

## Top-Level Preset Fields

| Field | Type | Required / Default | Allowed Range | Meaning |
|---|---|---|---|---|
| `schemaVersion` | integer | Required | `1` | Schema version of the file format. |
| `id` | string | Required | Non-empty, `[a-z0-9-]+` | Unique machine identifier for the preset. |
| `name` | string | Required | Non-empty string | Human-readable title shown in the UI. |
| `description` | string | Optional (default: `""`) | Any string | Human-readable description of the preset purpose. |
| `drills` | array of objects | Required | At least 1 drill | Ordered list of drills included in this preset. |

## Drill Fields

Every drill shares a set of common fields, plus specific fields determined by the drill `type`.

### Common Fields

| Field | Type | Required / Default | Allowed Range | Meaning |
|---|---|---|---|---|
| `id` | string | Required | Non-empty, `[a-z0-9-]+` | Unique drill identifier within the preset. |
| `name` | string | Required | Non-empty string | Human-readable name displayed during practice. |
| `type` | string | Required | `"hold"` or `"trace"` | Kind of practice drill. |
| `pedal` | string | Required | `"throttle"`, `"brake"`, `"clutch"` | Target pedal hardware axis to monitor. |
| `reps` | integer | Optional (default: `5`) | `1` to `50` | Number of repetitions to complete the drill. |
| `leadInMs` | integer | Optional (default: `3000`) | `1000` to `10000` | Lead-in preparation countdown before each repetition in milliseconds. GO shows for its last 1000 ms. |
| `tolerance` | number | Optional (default: `10`, D-17) | `0.5` to `50.0` | Half-width of the tolerance band in percentage points: `5` means the target ±5%. |
| `decimals` | integer | Optional (default: `0`) | `0` or `1` | Digits after the decimal point when the UI shows percentages for this drill. |
| `throttleLeadIn` | object | Optional | `level`: `10` to `100`, `holdMs`: `500` to `5000` | Requires holding the throttle at a set level before starting a brake repetition (see [Throttle lead-in](#throttle-lead-in)). |

### Hold Drill Fields (`"type": "hold"`)

Hold drills require pressing and maintaining the target pedal travel for a fixed duration.

| Field | Type | Required / Default | Allowed Range | Meaning |
|---|---|---|---|---|
| `target` | number | Required | `0.0` to `100.0` | Target pedal position in percent. |
| `holdMs` | integer | Required | `200` to `60000` | Time to maintain the target pedal position in milliseconds. |

### Trace Drill Fields (`"type": "trace"`)

Trace drills require following a dynamic target curve over time (for example, threshold braking into trail braking).

| Field | Type | Required / Default | Allowed Range | Meaning |
|---|---|---|---|---|
| `points` | array of `[t, value]` pairs | Required | 2 to 64 points, last `t` from `500` to `15000` | Control points defining the pedal curve over time. |

## How Trace Points Work

A trace drill specifies an array of two-element arrays `[t, value]`, where:
- `t` is the timestamp in milliseconds from the start of the attempt (`u32`).
- `value` is the target pedal position in percent (`0.0` to `100.0`).

### Interpolation and Clamping

- **Rounded curve:** Between adjacent points the target follows a monotone cubic (Fritsch-Carlson) curve with zero slope at the first and last point ([D-21](decisions/README.md)). The curve passes through every point and never goes above or below the two points around it, so it has no overshoot. The screen, the audio cue and scoring all use this curve.
- **Band:** A sample counts as in the band when it is within the tolerance of any target value within ±150 ms of that moment (D-21). On a ramp the band is wider than ±tolerance; on a flat part it is exactly ±tolerance.
- **Before the first point:** Timestamps before `0 ms` clamp to the first point value.
- **After the final point:** Timestamps beyond the final timestamp clamp to the final point value.
- **Normalization:** In internal scoring and evaluation, percentage values `0.0..=100.0` are converted into normalized fractions `0.0..=1.0`.

## Throttle lead-in

The optional `throttleLeadIn` object configures a throttle hold that must be satisfied before a brake repetition begins. It is allowed only on drills with `"pedal": "brake"`, supporting both hold and trace drills. Starting such a drill needs the throttle pedal assigned on the Devices page.

The object accepts two properties: `level` (target throttle percentage to hold, from `10` to `100`) and `holdMs` (required hold duration in milliseconds, from `500` to `5000`). Unknown keys inside `throttleLeadIn` are rejected.

After the `leadInMs` preparation countdown completes, the drill waits until the throttle is at `level - 10` percentage points or above. There is no timeout while waiting. The user then holds the throttle for `holdMs`. If the throttle drops below that during the hold, the drill waits again and the hold restarts.

When the hold completes, the UI shows LIFT and the brake rep starts, with hold or trace timing counting from that moment. While the drill waits and during the hold, the audio tone beeps when the throttle is below the accepted range; during the rep it beeps against the brake target as usual.

After each rep of such a drill the app shows overlap: the time both pedals were above 5 % at once (during the hold and the rep), and the peak throttle while the brake was above 5 %. The score does not change.

Here is an example of a brake trace drill configured with a throttle lead-in:

```json
{
  "id": "heavy-stop-from-throttle",
  "name": "Heavy stop from full throttle",
  "type": "trace",
  "pedal": "brake",
  "tolerance": 7,
  "reps": 5,
  "leadInMs": 2000,
  "throttleLeadIn": {
    "level": 100,
    "holdMs": 1500
  },
  "points": [
    [0, 0],
    [200, 78],
    [900, 76],
    [1700, 58],
    [2400, 25],
    [2900, 12],
    [3300, 5],
    [3600, 0]
  ]
}
```

## Validation Rules

The parser validates all presets strictly upon loading:
1. `schemaVersion` must equal `1`.
2. `id` for both presets and drills must be non-empty and contain only lowercase ASCII letters, digits, and hyphens (`[a-z0-9-]+`).
3. Drill IDs must be unique within a preset.
4. Names (`name`) for both presets and drills must be non-empty.
5. Presets must contain at least one drill.
6. `reps` must be between `1` and `50`.
7. `leadInMs` must be between `1000` and `10000`.
8. `tolerance` (if present) must be finite and between `0.5` and `50.0`. Omitted tolerances default to `10.0` (D-17). `decimals` (if present) must be `0` or `1`.
9. Hold drills: `target` must be finite and between `0.0` and `100.0`; `holdMs` must be between `200` and `60000`.
10. Trace drills:
    - Must contain at least `2` and at most `64` points.
    - The first point timestamp must be `0`.
    - Timestamps must be strictly increasing (`t[n] > t[n - 1]`).
    - Every value must be finite and between `0.0` and `100.0`.
    - The total duration (the final point timestamp) must be between `500` and `15000` ms. The ±150 ms band needs a trace well over 300 ms, and 15 s plus the 1 s GO fits the UI's 20 s pedal history.
11. `throttleLeadIn` (if present) is allowed only on drills with `pedal` set to `"brake"`. The `level` value must be finite and between `10` and `100`. The `holdMs` value must be an integer between `500` and `5000`. Unknown keys inside `throttleLeadIn` are rejected.
12. Unknown fields are rejected at both the preset and drill level.
13. Directories loaded via the directory loader must not contain duplicate preset IDs across different files.

## Sample Preset

Here is the complete `presets/sample.json` file included with the repository:

```json
{
  "schemaVersion": 1,
  "id": "sample",
  "name": "Sample",
  "description": "Sample threshold braking and throttle control drills.",
  "drills": [
    {
      "id": "brake-hold-70",
      "name": "Brake hold 70%",
      "type": "hold",
      "pedal": "brake",
      "target": 70,
      "tolerance": 5,
      "holdMs": 2000,
      "reps": 5,
      "leadInMs": 3000
    },
    {
      "id": "throttle-hold-50",
      "name": "Throttle hold 50%",
      "type": "hold",
      "pedal": "throttle",
      "target": 50,
      "tolerance": 5,
      "holdMs": 2000,
      "reps": 5,
      "leadInMs": 3000
    },
    {
      "id": "hairpin",
      "name": "Hairpin trace",
      "type": "trace",
      "pedal": "brake",
      "tolerance": 6,
      "reps": 5,
      "leadInMs": 2000,
      "points": [
        [0, 0],
        [150, 100],
        [300, 95],
        [600, 70],
        [1000, 40],
        [1500, 0]
      ]
    }
  ]
}
```
