# emu-shot-orchestrator

A webhook-driven orchestrator that produces emulator screenshots on your own hardware and
uploads them to the [emu.layle.dev](https://github.com/ioncodes/emu.layle.dev) archive.

A `POST /webhook/run {emulator, commit}` triggers a deterministic pipeline: clone/checkout the
exact emulator commit, read the screenshotter's fixed game set, resolve & stage ROMs from a
read-only NAS mount, build & run an emulator-specific screenshotter container, post-process the
frames, validate, then invoke `emu.layle.dev/tools/submit-screenshots.py`.

The orchestrator owns *all* orchestration; screenshotter containers are thin and manifest-driven.

## Architecture

```
                 Cloudflare Tunnel
                        │
              POST /webhook/run (Bearer auth)
                        │
                 ┌──────▼──────┐      docker.sock
                 │ orchestrator │◄──────────────────► host Docker daemon
                 └──────┬──────┘                         (build + run shotters)
        /roms (ro)      │  /jobs  /repos  /secrets
   NAS SMB mount ───────┘
```

The orchestrator **never** mounts `/roms` into a screenshotter container — each container
receives only a staged, read-only `/input`, a writable `/output`, the generated
`/job/manifest.json`, and per-emulator `/config` secrets (BIOS/IPL).

### Pipeline stages

`prepare_repos → read_game_files → resolve_roms → stage_inputs → build_image →
run_screenshotter → postprocess_frames → validate_output → upload → done`

`status.json` is written after every stage. Per-stage logs live in `/jobs/<id>/logs/`
(`clone.log`, `build.log`, `roms.log`, `run.log`, `upload.log`).

## HTTP API

| Method | Path              | Notes                                            |
|--------|-------------------|--------------------------------------------------|
| POST   | `/webhook/run`    | `Authorization: Bearer <WEBHOOK_SECRET>` required |
| GET    | `/jobs/:id`       | Job status JSON                                  |
| GET    | `/jobs`           | All jobs, newest first                           |
| GET    | `/healthz`        | Liveness                                         |

Webhook payload (no branch, no repo URL, no games, no options):

```json
{ "emulator": "iris", "commit": "<40-hex-sha>", "force": false }
```

- `emulator` must exist in config.
- `commit` must match `^[a-fA-F0-9]{40}$`.
- `force` optional, defaults `false`.
- Unknown fields are rejected (400).

Responses:

```json
{ "job_id": "2026-05-27T16-10-00Z-iris-90dae6a", "state": "queued" }
{ "job_id": "...", "state": "already_completed",
  "message": "submission already exists for emulator+commit" }
```

## Detecting failures (per-emulator)

The webhook is asynchronous: `POST /webhook/run` returns `queued` before the pipeline runs, so
the build result is reported via the job status, not the POST response. Poll `GET /jobs/:id`:

```json
{
  "id": "2026-05-27T16-10-00Z-ymir-90dae6a",
  "state": "failed",
  "stage": "build_image",
  "failure_kind": "build",
  "error": "docker build failed: ...",
  ...
}
```

`failure_kind` is a stable, machine-readable category so an automated caller can react:

| `failure_kind`      | Meaning                                                              |
|---------------------|----------------------------------------------------------------------|
| `build`             | screenshotter/emulator image build broke — likely an upstream breaking change; **this screenshotter repo needs an update** |
| `emulator_repo`     | emulator clone/checkout/submodule failed (bad commit?)               |
| `game_set` / `rom` / `archive` | gamelist/titlemap or ROM resolution/extraction problem    |
| `run`               | the screenshotter container exited non-zero                          |
| `postprocess` / `output_validation` | no usable frames / invalid output                   |
| `upload`            | `submit-screenshots.py` failed                                       |

Failures are isolated per job, so one emulator's build breaking never affects another
emulator's runs. A caller wanting a non-2xx signal can simply poll until `state` is
`completed` / `failed` / `already_completed` and branch on `failure_kind`.

## Idempotency

Before heavy work the orchestrator pulls `emu.layle.dev` and looks for
`meta/submissions/<archive_slug>/*-<commit_short>.json`, parsing each candidate and comparing
the full `"commit"`. If found and `force=false` the job is marked `already_completed` and
nothing is rebuilt. `force=true` runs the full pipeline (R2 objects are content-addressed, so
re-uploads are cheap).

## Configuration

`config.toml` holds only orchestrator-level settings (server, paths, upload, postprocess).
There is **no per-emulator block** — each screenshotter is an in-tree directory and its
`screenshotter.toml` is the single source of truth for that emulator (slug, `emulator_repo`,
`archive_slug`, `requires_gpu`, `max_parallel_games`, `output_mode`, and the supported formats).
At startup the orchestrator scans `paths.screenshotter_root` for immediate subdirectories
containing a `screenshotter.toml` and builds its emulator registry from them. Screenshotters
are **not** cloned — they ship with the orchestrator; only the emulator *core* is cloned (the
webhook provides its commit).

Per-emulator archive handling is fully config-driven — there is no hard-coded zip behavior
(set in each `screenshotter.toml`):

- `zip` in `supported_archives` → orchestrator extracts and stages the anchor file.
- `zip` in `supported_direct` → passed through untouched (the emulator reads zips itself).
- `zip` in neither → the job is rejected with a clear error.

Frame post-processing (`[postprocess]` in `config.toml`) — dedupe and single-color removal — is
performed by the **orchestrator** so the rule is identical across every emulator. Tunables:
`remove_single_color`, `solid_tolerance`, `dedupe`.

> **Deployment invariant:** `job_root`, `repo_root`, and `secret_root` must be **identity-mapped**
> in `docker-compose.yml` (same path on host and inside the orchestrator container). The
> orchestrator drives the host Docker daemon over the socket, so the `-v` paths it passes for
> screenshotter containers are resolved on the *host*. For the same reason, staged ROMs are
> **hardlinked** (or copied across filesystems), never symlinked, so they appear as real files
> inside screenshotters. The orchestrator must also be able to read the screenshotter-written
> output (it runs as root in the compose image, which satisfies this).

See [`config.example.toml`](./config.example.toml).

## Setup

1. **Mount the NAS** (read-only) on the host at `/mnt/roms` (matching `docker-compose.yml`):

   ```bash
   sudo mount -t cifs //NAS/Roms /mnt/roms \
     -o credentials=/root/.smbcred,ro,uid=1000,gid=1000
   ```

   `docker-compose.yml` then bind-mounts `/mnt/roms:/roms:ro` into the orchestrator.

2. **Create config:**

   ```bash
   cp config.example.toml config.toml
   # edit paths/emulators as needed
   ```

3. **Create secrets:**

   ```
   secrets/r2.env              # R2 credentials for submit-screenshots.py
   secrets/iris/bios/ps2.bin   # per-emulator BIOS, mounted at /config in the container
   secrets/ymir/ipl/...        # Saturn IPL ROMs
   ```

4. **Start:**

   ```bash
   export WEBHOOK_SECRET=$(openssl rand -hex 32)
   export CLOUDFLARE_TUNNEL_TOKEN=...        # for the cloudflared service
   docker compose up -d --build
   ```

5. **Test:**

   ```bash
   curl -X POST http://127.0.0.1:8080/webhook/run \
     -H "Authorization: Bearer $WEBHOOK_SECRET" \
     -H "Content-Type: application/json" \
     -d '{"emulator":"ymir","commit":"90dae6afaa055444bdf89fa04f14cffafc271f9f"}'
   ```

   Then watch `jobs/<job_id>/status.json` and `jobs/<job_id>/logs/`.

## Local development (no Docker)

```bash
cp config.example.toml config.toml
# point rom_root/job_root/repo_root/secret_root at local ./ dirs and set
# screenshotter_root = "screenshotters"
WEBHOOK_SECRET=devsecret scripts/dev-run.sh
```

`cargo test` runs the parser/validation/post-processing unit tests.

## Security notes

- Mounting `/var/run/docker.sock` gives the orchestrator root-equivalent host control. This is
  acceptable on your own hardware, but **webhook auth must be strict** — keep `WEBHOOK_SECRET`
  secret and only expose the service through the Cloudflare Tunnel.
- All docker build/run and git commands use fixed shapes; nothing arbitrary comes from the
  webhook or screenshotter config.
- Screenshotter containers run with `--network none`, read-only `/input`, and never see `/roms`.
- Gamelist paths are validated: absolute paths and `../` / `..\` traversal segments are
  rejected (a `..` *inside* a filename is allowed). Archives are extracted safely.

## Screenshotter contract

Each in-tree screenshotter directory provides `gamelist.txt`, `titlemap.txt`,
`screenshotter.toml`, `Dockerfile`, and a runner. It receives `/job/manifest.json`:

```json
{
  "emulator": "iris",
  "commit": "<sha>",
  "games": [
    { "id": "SLUS_211_34", "title": "Resident Evil 4 (USA)",
      "input": "/input/SLUS_211_34/game.iso", "output_dir": "/output/SLUS_211_34" }
  ]
}
```

and writes `/output/<UNIQUE_ID>/<integer>.png`. The `id` is the titlemap `UNIQUE_ID` (the source
of truth for the site `game_id` and folder name) — **never** derived from a ROM header.

Reference screenshotters live in [`screenshotters/iris/`](./screenshotters/iris) and
[`screenshotters/ymir/`](./screenshotters/ymir).
