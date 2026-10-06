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
| `leadInMs` | integer | Optional (default: `3000`) | `0` to `10000` | Lead-in preparation countdown before each repetition in milliseconds. |
| `tolerance` | number | Required | `0.5` to `50.0` | Half-width of the tolerance band in percentage points: `5` means the target ±5%. |

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
| `points` | array of `[t, value]` pairs | Required | At least 2 points | Control points defining the pedal curve over time. |

## How Trace Points Work

A trace drill specifies an array of two-element arrays `[t, value]`, where:
- `t` is the timestamp in milliseconds from the start of the attempt (`u32`).
- `value` is the target pedal position in percent (`0.0` to `100.0`).

### Interpolation and Clamping

- **Linear interpolation:** Between adjacent points `(t0, v0)` and `(t1, v1)`, the target value at time `t` is calculated linearly:
  ```text
  target = v0 + ((t - t0) / (t1 - t0)) * (v1 - v0)
  ```
- **Before the first point:** Timestamps before `0 ms` clamp to the first point value.
- **After the final point:** Timestamps beyond the final timestamp clamp to the final point value.
- **Normalization:** In internal scoring and evaluation, percentage values `0.0..=100.0` are converted into normalized fractions `0.0..=1.0`.

## Validation Rules

The parser validates all presets strictly upon loading:
1. `schemaVersion` must equal `1`.
2. `id` for both presets and drills must be non-empty and contain only lowercase ASCII letters, digits, and hyphens (`[a-z0-9-]+`).
3. Drill IDs must be unique within a preset.
4. Names (`name`) for both presets and drills must be non-empty.
5. Presets must contain at least one drill.
6. `reps` must be between `1` and `50`.
7. `leadInMs` must be between `0` and `10000`.
8. `tolerance` must be finite and between `0.5` and `50.0`.
9. Hold drills: `target` must be finite and between `0.0` and `100.0`; `holdMs` must be between `200` and `60000`.
10. Trace drills:
    - Must contain at least `2` points.
    - The first point timestamp must be `0`.
    - Timestamps must be strictly increasing (`t[n] > t[n - 1]`).
    - Every value must be finite and between `0.0` and `100.0`.
    - The total duration (the final point timestamp) must not exceed `60000` ms (60 seconds).
11. Unknown fields are rejected at both the preset and drill level.
12. Directories loaded via the directory loader must not contain duplicate preset IDs across different files.

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
