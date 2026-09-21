# ci/

Staging area for GitHub Actions workflow changes.

This directory exists because session credentials cannot write
`.github/workflows/*` — see admin `DECISIONS.md` ADR-8. To change CI:

1. Put the intended workflow file in `workflows/`.
2. A maintainer promotes it with the admin `rollout/apply-ci-folders.sh`
   script.

## Pending

- **`workflows/docs.yml`** — the prose gate: Vale over the reader-facing
  pages at the levels set in `.vale.ini`, on the file list
  `ts/scripts/gated-docs.cjs` produces. See `docs/STYLE-GUIDE.md`.

  It needs no sibling checkouts and no secrets, and pins its own Vale
  version. Errors fail the job; warnings go to the run summary as a
  report. `make prose` runs the identical check locally, and the test
  suite already runs the other half of the gate
  (`ts/test/docs.test.js`), so promoting this adds the spelling and
  Google-convention arm rather than the whole gate.

- **`workflows/rust.yml`**, the Rust gate: `ci/rust/run.sh` over the
  crate in `rs/`, on the MSRV pinned in `rs/Cargo.toml`. The engine and
  the test-support crate are unpublished path dependencies on sibling
  checkouts (`../parser/rs`, `../support/rs`), so the workflow checks
  this repository out into a named directory and clones both siblings
  beside it before running the script. The script is the whole gate
  (`cargo fmt --check`, build, tests, doctests, clippy with warnings
  denied, and a lockfile check that exempts the two siblings' recorded
  versions), so a local run of it says exactly what CI would.
