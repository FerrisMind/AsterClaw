# Scripts

## NFR Harness

`nfr-harness.ps1` compares `AsterClaw` against a Go baseline binary.

What it does:

1. Builds `AsterClaw` (`cargo build --release`).
2. Builds the Go baseline from `references/picoclaw` (`go build`).
3. Uses a temporary HOME (`target/nfr/home`) for isolated onboarding/config.
4. Starts each gateway, waits for `/ready`, records:
   - `startup_ms` (`startup_to_ready`)
   - `rss_bytes` (working set)
5. Evaluates gates:
   - `rss <= 1.05`
   - `startup <= 1.10`
6. Writes `target/nfr/nfr-results.json`.

Usage:

```powershell
pwsh scripts/nfr-harness.ps1
```

Notes:

- Requires `go` and `cargo` in `PATH`.
- Use `-SkipBuild` only if `target/release/asterclaw.exe` and the Go baseline binary already exist.

## Optional: PGO Build (Manual)

For an extra runtime optimization pass, build with profile-guided optimization:

1. Instrumented build:

```powershell
$env:RUSTFLAGS="-Cprofile-generate=target/pgo-data"
cargo build --release
```

2. Run representative workload(s) to generate profile data (gateway startup, tool-calling, telegram path).

3. Optimized build from profile:

```powershell
$env:RUSTFLAGS="-Cprofile-use=target/pgo-data -Cllvm-args=-pgo-warn-missing-function"
cargo build --release
```

4. Clear `RUSTFLAGS` after build if needed:

```powershell
Remove-Item Env:RUSTFLAGS
```

## Simply Tool-Memory Profiling

`simply-profile.ps1` measures peak process memory while running a tool-heavy test (`exec_truncates_large_stdout`), useful for tracking memory spike regressions.

Usage:

```powershell
pwsh scripts/simply-profile.ps1
```

Output:

- `target/simply-profile/results.json`
- per-run logs in `target/simply-profile/logs/`
