# telemetry-extract

Developer CLI in `crates/telemetry-extract` for the sim-curve-train-app repo. It is not shipped in the app. It turns Garage 61 60 Hz CSV lap exports into draft drill presets and per-car pedal statistics.

## Build & run

```bash
cargo run -p telemetry-extract -- <subcommand>
```

## extract

Detect pedal zones and export validated preset JSON drills.

```bash
cargo run -p telemetry-extract -- extract \
  --out preset.json \
  --car-filter "Car Name" \
  --preset-id my-preset \
  --preset-name "My Preset" \
  --tolerance 10.0 \
  --max-drills 12 \
  "Garage 61 - Driver - Car - Track - 01.23.456 - 123.csv"
```

If `--out` is omitted, JSON prints to stdout.

## stats

Aggregate and display pedal control metrics per car.

```bash
cargo run -p telemetry-extract -- stats \
  --car-filter "Car Name" \
  --json \
  "Garage 61 - Driver - Car - Track - 01.23.456 - 123.csv"
```

## Input file naming

Expected Garage 61 filename:

```text
Garage 61 - <Driver> - <Car> - <Track> - <mm.ss.mmm> - <ID>.csv
```

`--car-filter` keeps laps whose car name contains the given text, ignoring case (`--car-filter mx-5` matches "Global Mazda MX-5 Cup"). Mixed cars or tracks are rejected unless `--allow-mixed` is used.

## Output

Drill IDs look like:

```text
<track-slug>-c<NN>-<kind>
```

`kind` is one of: `brake`, `brake-hold`, `lift`, `lift-hold`, `throttle`, `throttle-hold`. Corners are numbered by lap position. If `--tolerance` is omitted, the app default of ±10 % applies. By default, at most 12 drills are output, highest-priority corners first.

## Notes

- Never commit telemetry CSVs.
- `--car-filter` and `--allow-mixed` work for `stats` too.
- Output is a draft to hand-tune before it becomes a bundled preset.
