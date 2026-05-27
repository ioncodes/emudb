# iris-screenshotter

Emulator-specific screenshotter for [Iris](https://github.com/allkern/iris) (PS2), driven by
[emu-shot-orchestrator](../../). Produces **one screenshot per game**: `/output/<UNIQUE_ID>/0.png`.

This is a thin, manifest-driven container. The orchestrator clones/checks out the exact Iris
commit and supplies it as the Docker build context `emulator`; this repo only builds Iris and
runs it once per game.

## Files

| File                | Purpose                                                        |
|---------------------|----------------------------------------------------------------|
| `Dockerfile`        | BuildKit; builds Iris from the `emulator` build context        |
| `run.py`            | Entrypoint: reads `/job/manifest.json`, runs Iris per game     |
| `screenshotter.toml`| Runner settings + Iris CLI flag mapping                        |
| `gamelist.txt`      | Fixed game set (exact ROM filenames)                           |
| `titlemap.txt`      | `UNIQUE_ID=stem` map (passed to the uploader as `--title-map`) |

## Contract

The orchestrator runs:

```
docker run --rm --gpus all --network none \
  -v <job>/input:/input:ro -v <job>/output:/output \
  -v <job>/manifest.json:/job/manifest.json:ro -v /secrets/iris:/config:ro \
  emu-shot/iris:<tag> --job /job/manifest.json --input /input --output /output
```

For each manifest game, `run.py` runs:

```
iris --bios /config/bios/ps2.bin --disc <input> --screenshot-on-exit /output/<id>/0.png
```

and validates the PNG. Exit 0 if all games succeed, 1 otherwise. Frame dedupe / single-color
removal is **not** done here — the orchestrator owns post-processing.

## Status

Scaffold. Iris does not yet ship the `--screenshot-on-exit` CLI mode; `screenshotter.toml`
makes the flag names configurable so this needs no code change once the real flags land.
`gamelist.txt`/`titlemap.txt` contain placeholder samples.
