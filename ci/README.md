# ci/

The scripts CI runs.

To change CI, edit `.github/workflows/` in a reviewed pull request.
Session credentials push workflow files (admin `DECISIONS.md` ADR-8, as
amended 2026-09-24), so staging a workflow under `ci/workflows/` first
for a maintainer to promote is optional. Sessions still cannot push
tags, so a maintainer pushes any tag that a tag-triggered workflow
needs.

The amendment also asks for the same change in `tabnas/admin` wherever
admin keeps a copy of the workflow:

- If admin's `rollout/workflows/` holds a `markdown__<file>`
  template for the workflow you changed, make the same edit there.
  Admin `scripts/verify.sh` compares each template with its deployed
  copy, and a maintainer's `rollout/apply-workflows.sh --apply` would
  push the older text back over yours.
- `clib.yml` and `clib-release.yml` are stamped from admin
  `tasks/clib-template/` and carry a `tabnas-clib-template` marker.
  Change the template, then restamp with admin `tasks/adopt-clib.sh`,
  which writes both workflows straight into `.github/workflows/`. The
  new stamp lands in this repository's own reviewed pull request. Never
  edit the copies here.

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
