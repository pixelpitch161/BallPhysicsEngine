# Plan: run only the checks a pull request can affect

Asked by Aedan on 2026-11-21: "Is it really necessary to run the full suite for every change?" Agreed: (1) a pull
request tests only the crates it touches plus every crate that depends on them, (2) a change to notes and docs alone
runs no build, lint or tests. Not chosen: a nightly full run.

## Design

- New job **What changed** (`changes` in `.github/workflows/ci.yml`) runs `.github/scripts/affected.py HEAD^1` on the
  pull request's merge commit (checkout `fetch-depth: 2`, `git diff --no-renames` so a moved file counts at both ends; the first parent is the branch it merges into, so the diff
  is exactly what the merge adds). A push to main and a manual run pass `--all`.
- `affected.py` (Python 3 standard library, `tomllib`; no Rust toolchain needed) sorts each changed path:
  - notes and docs: `docs/*`, `*.md` outside `crates/`, a crate's `.md` outside `src/`, `LICENSE*`, `.gitignore`: nothing
    (`.gitattributes` can change line endings of files built in, so it runs everything).
  - `crates/<folder>/...` of an existing crate: that crate.
  - `crates/*/assets/*`: everything (bpe-kit builds bpe-studio's fonts in with `include_bytes!`).
  - anything else (`Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `.github/`, `.config/`,
    `assets/`, a removed crate): everything. Unknown means run all, so a skipped test is never a surprise.
  - The changed crates grow to every workspace crate that depends on them through `[dependencies]`,
    `[dev-dependencies]`, `[build-dependencies]` or a `[target.*]` table, repeated until nothing new joins.
- Outputs: `code`, `full`, `packages`, `filter` (nextest filterset `package(a) | ...`, or `all()`), `goldens_kit`,
  `goldens_sound`, `windows`.
- Jobs:
  - Format: always (under a minute).
  - Clippy: when `code`. Still the whole workspace (1-2 minutes, and lint of one crate can depend on another).
  - Tests 1-4/4: when `code`; still build `--workspace` (one feature set, the same cache), then
    `-E '<filter>' --no-tests=pass`. nextest applies the filter before the slice partition (checked on a small
    workspace with nextest 0.9.152: 8 filtered tests split 2/2/2/2), so the four machines share the picked tests.
  - Tests gate: green when "What changed" succeeded and the shards passed, or were skipped because `code` is false.
    A failed "What changed" makes the gate fail, so a broken picker can't pass as "nothing to test".
  - Goldens (release): when bpe-kit or bpe-sound is picked; each of its two steps runs only for its own crate.
  - Windows cross-build: when bpe-studio or bpe-cli is picked.
- Skipped jobs report as skipped, which GitHub counts as passing for required checks.

## What it saves

The crates sit in a chain (settings-derive, core, runtime, gfx, dsp, kit, sound, cli, studio), so a change low down
still tests nearly everything. The savings are for the top of the chain and for docs:

| Change in | Tests | Goldens | Windows |
| --- | --- | --- | --- |
| notes, docs | none | no | no |
| bpe-studio | bpe-studio | no | yes |
| bpe-cli | bpe-cli, bpe-studio | no | yes |
| bpe-sound | bpe-sound, bpe-cli, bpe-studio | sound only | yes |
| bpe-kit, bpe-dsp | from there up (skips core, runtime, gfx) | yes | yes |
| bpe-core, shared files | everything | yes | yes |

## Not done

- No nightly full run (not asked for). A push to main still checks everything, so a missed dependency shows up there.
- The test machines still compile the whole workspace; selecting `-p` crates would change feature unification and
  miss the shared cache.

## Verify

- `affected.py` on hand-made path lists (docs only, studio, sound, dsp, core, Cargo.lock, bpe-studio assets, a
  removed crate, ci.yml) gives the table above.
- The pull request itself changes `.github/`, so it runs everything; its "What changed" summary shows `full=true`.
