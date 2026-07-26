# 5. Debian packaging, Windows packaging, and CI/release automation

Date: 2026-07-26

## Status

Accepted

## Context

`ft991a` had no `packaging/` directory and no `.github/` directory at all
before this round. `radio-cat-rs` ADR 0008 defines a shared, reusable
`release-app.yml` GitHub Actions workflow (in that repo) with a fixed
consuming-repo contract: `packaging/build-deb.sh [--skip-build]`,
`packaging/build-windows-package.ps1` (new, no prior precedent in either
sibling app), and a `[[bin]]` matching the declared `main_binary`. `ts570d`
already satisfies the Debian half of this contract; this ADR brings
`ft991a` to parity and adds the Windows half, which neither app had before.

### What was read before deciding

- `ts570d/packaging/build-deb.sh` and `ts570d/packaging/DEBIAN/control` in
  full, as the literal template (per the task's own instruction).
- `ts570d/.github/workflows/{ci,release}.yml` in full, as the CI/release
  structure template.
- `radio-cat-rs/docs/adr/0006-windows-network-transport.md` §6 and
  `docs/adr/0008-shared-release-workflow.md` in full — the exact contract
  `build-deb.sh`/`build-windows-package.ps1` must satisfy, and where
  `pin-test` actually comes from.
- `ft991a`'s own `Cargo.toml` (`version = "0.0.1"`, workspace-wide) and
  `emulator/Cargo.toml` (`serialport = "4"`, which needs `libudev-dev` on
  Linux — confirmed by `ts570d`'s own CI installing the same package for
  the same reason).

## Decision

### 1. `packaging/DEBIAN/control` + `packaging/build-deb.sh` — a close mirror of `ts570d`'s, package name `ft991a-radio-control`

Same staging-tree approach, same three binaries
(`ft991a`→`/usr/bin/ft991a-control`, `emulator`→`/usr/bin/ft991a-emulator`,
`pin-test`→`/usr/bin/rs232c-pintest`), same DEP-5 copyright generation from
a new root `LICENSE.txt` (copied verbatim from `ts570d/LICENSE.txt` — the
Apache 2.0 boilerplate text itself has no project-specific content to
diverge on; confirmed by grepping it for `ts570d`/`Kenwood` and finding
none), same `--skip-build` contract.

**`pin-test` build step, verified rather than assumed:** `cat-transport-serial`
(where `pin-test` now lives, per ADR 0006 §6) is not a workspace member of
this repo — it's consumed as a (patched, per
[ADR 0003](0003-consume-radio-cat-rs-windows-network-transport.md)) git
dependency. `cargo build --release -p cat-transport-serial --bin pin-test`
was run directly from inside this repo and confirmed to work, landing the
binary at the same `target/release/pin-test` path `ts570d`'s identical
script already reads from — `cargo build -p <package>` selects by package
ID across the *whole resolved dependency graph*, not only workspace
members, exactly as ADR 0006 §6 states. `build-deb.sh` calls this as an
explicit second build step (unlike `ts570d`'s script, which only calls
plain `cargo build --release` and relies on `pin-test` already existing
from a stale local build step ADR 0008 flagged as `ts570d`'s own
follow-on) — `ft991a` never had a local `pin-test` source to begin with,
so there is no equivalent stale step to inherit.

Verified end-to-end: `./packaging/build-deb.sh --skip-build` (after
building all three binaries) produced
`ft991a-radio-control_0.0.1_amd64.deb` in the repo root;
`dpkg-deb --contents`/`--info` confirm all three binaries staged at their
documented paths and the control metadata is well-formed.

### 2. `packaging/build-windows-package.ps1` — new contract, no precedent in either sibling app

Per ADR 0008 §3's exact requirements (invoked via `pwsh`, no arguments,
after `cargo build --release` has produced `target/release/ft991a.exe`;
must produce package file(s) directly in the repo root): stages
`ft991a.exe`, `pin-test.exe` (optional — warns rather than fails if
absent, since it isn't built by a plain `cargo build --release`),
`README.md`, and `LICENSE.txt` into a versioned directory under
`target/windows-package/`, then `Compress-Archive`s it into
`ft991a-radio-control-<version>-windows-x86_64.zip` in the repo root —
satisfying the contract with a plain zip, exactly as ADR 0008 §3
describes as sufficient ("an NSIS/Inno Setup installer is an enhancement
either app can add later without changing this workflow at all").

**Not executed or verified by an actual run** — there is no
Windows/PowerShell (`pwsh`) available in this sandbox
(`which pwsh`/`which powershell` both report not found). Written and
reviewed by hand against ADR 0008 §3's contract and `build-deb.sh`'s own
staging-tree structure (same version-extraction idea, same "binaries +
docs into a staging dir, then archive" shape), not validated by execution.
This is a real, disclosed gap, not silently assumed away.

### 3. `.github/workflows/ci.yml` — `ts570d`'s structure, plus a genuinely new Windows cross-check job

Same `fmt --check`/`clippy`/unit-tests/integration-tests shape and
toolchain pin (`dtolnay/rust-toolchain@stable`, `toolchain: "1.93"`) as
`ts570d/.github/workflows/ci.yml`, adapted to this repo's own commands
(`cargo test --workspace --lib`, `cargo test --test integration` — this
repo's actual test binary name, per `tests/integration.rs`) and its own
`libudev-dev` apt dependency (`emulator`'s `serialport` crate). A second
job, `windows-check`, runs `cargo check --target x86_64-pc-windows-gnu -p
ft991a` — the exact command `CLAUDE.md` already documents as this repo's
Windows verification method — as CI, not just a manually-run local
command. `server` is deliberately excluded from that command (both here
and in `CLAUDE.md`), since it is genuinely not Windows-buildable yet (see
ADR 0003) and the root `Cargo.toml` never pulls it in for that target
anyway.

### 4. `.github/workflows/release.yml` — a thin caller, per ADR 0008 §4's exact template, plus `apt_packages`

Follows ADR 0008 §4's template exactly (`app_name: "ft991a-radio-control"`,
`main_binary: "ft991a"`, `extra_binaries: "emulator
cat-transport-serial:pin-test"`), with one addition beyond the task's own
literal example: `apt_packages: "libudev-dev"`, matching ADR 0008 §4's own
worked *`ts570d`* example (which includes exactly this input, for the
identical `serialport`/`libudev-dev` reason) rather than the task
description's own trimmed-down template that omitted it. Kept because the
underlying need is real — `radio-cat-rs`'s shared `linux` job installs
`apt_packages` before building, and `emulator` will fail to build without
`libudev-dev` present on a stock `ubuntu-latest` runner (confirmed by this
repo's own `ci.yml` needing the identical package for the identical
reason).

**This will not resolve or run until a human pushes `radio-cat-rs`'s
`main` branch** — `uses: kf0uwv/radio-cat-rs/.github/workflows/
release-app.yml@main` resolves against the *remote* GitHub repository, not
this local checkout. Per ADR 0008's own explicit callout, this is expected
and not a bug to chase.

## Consequences

- New root-level `LICENSE.txt` (full Apache 2.0 text) — previously this
  repo only mentioned Apache 2.0 in its README, with no actual license
  file, which `build-deb.sh`'s DEP-5 copyright generation needs verbatim.
- New `packaging/DEBIAN/control`, `packaging/build-deb.sh`,
  `packaging/build-windows-package.ps1`, `.github/workflows/ci.yml`,
  `.github/workflows/release.yml`.
- The Debian packaging path (build-deb.sh) is verified by an actual local
  run producing a real, inspected `.deb`. The Windows packaging path
  (build-windows-package.ps1) and both GitHub Actions workflows are
  verified by careful hand-review and (for the workflows) YAML syntax
  validation only — no GitHub Actions runner or Windows machine is
  available in this sandbox to exercise them for real. The release
  workflow additionally cannot resolve at all yet, for the separate,
  expected reason in Decision §4.
