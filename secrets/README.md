# secrets/

Real secret values here are **gitignored**; only templates (`*.example`), `.gitkeep`, and this
README are tracked.

```
secrets/
  r2.env                 # Cloudflare R2 credentials (copy from r2.env.example)
  ymir/ipl/*.bin         # Saturn IPL ROMs (512 KiB each); worker auto-picks by region -> /config/ipl
  iris/bios/ps2.bin      # PS2 BIOS (for Iris, once it ships a screenshot CLI) -> /config/bios/ps2.bin
```

The orchestrator mounts `secret_root/<emulator>` into each screenshotter at `/config` (ro), and
passes `r2.env` to the uploader via `--env-file`.

**docker-compose:** `secret_root` must be a real **host** path identity-mapped into the
container (default `/opt/emu-shot/secrets`). For local `cargo run`, `config.toml` uses
`./secrets`.
