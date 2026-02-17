# Scripts

## NFR Harness

`nfr-harness.ps1` compares `femtors` against a Go `picoclaw` baseline binary.

What it does:

1. Builds `femtors` (`cargo build --release`).
2. Builds `picoclaw` from `references/picoclaw` (`go build ./cmd/picoclaw`).
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
- Use `-SkipBuild` only if `target/release/femtors.exe` and `target/nfr/picoclaw.exe` already exist.

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
