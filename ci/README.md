# ci/

The scripts CI runs, and the staging area for workflow changes.

The staging half exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`, creating the
   directory if a previous promotion emptied it.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

Nothing is staged today. Both workflows that used to sit here have been
promoted, and what remains under `ci/` is the Rust gate script.

## Scripts

- **`rust/run.sh`** — the Rust gate, in full: `cargo fmt --check`, build,
  `cargo test --all-targets`, `cargo test --doc` (which `--all-targets`
  excludes), clippy with warnings denied, and a lockfile check that
  exempts the two siblings' recorded versions. `.github/workflows/rust.yml`
  runs this file and nothing else, so a local run of it says exactly what
  CI would. It lives here rather than in `.github/` so that a promotion
  never has to move it.

  It is not what `make test-rs` runs. That target is tests, doctests and
  clippy, and skips the fmt and lockfile arms, so a tree that satisfies
  it can still fail this script.

## Promoted

- **`.github/workflows/docs.yml`** — the prose gate: Vale over the
  reader-facing pages at the levels set in `.vale.ini`, on the file list
  `ts/scripts/gated-docs.cjs` produces, plus `ts/scripts/vale-counts.cjs`
  to hold every demoted-rule count in `.vale.ini` to a live run. See
  `docs/STYLE-GUIDE.md`.

  It needs no sibling checkouts and no secrets, and pins its own Vale
  version. Errors fail the job; warnings go to the run summary as a
  report. `make prose` runs both steps locally, and the test suite runs
  the other half of the gate (`ts/test/docs.test.js`), so this workflow
  is the spelling and Google-convention arm rather than the whole gate.

- **`.github/workflows/rust.yml`**, the Rust gate: `ci/rust/run.sh` over
  the crate in `rs/`, on the MSRV pinned in `rs/Cargo.toml`. The engine
  and the test-support crate are unpublished path dependencies on
  sibling checkouts (`../parser/rs`, `../support/rs`), so the workflow
  checks this repository out into a named directory and clones both
  siblings beside it before running the script.
