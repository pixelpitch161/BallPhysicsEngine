# Faster checks (CI wall time)

Asked by the owner on 2026-11-21: "Is there any way to have tests go quicker? Multiple tests run in parallel?"

## Where the time goes now (measured)

From the CI runs of PR #198/#199 and their merges to main (runs 44210829196, 44232382514, 44248254355):

| Job | Wall time |
| --- | --- |
| Format | under 1 min |
| Clippy | 1-2 min |
| Goldens (release) | 10-15 min |
| Windows cross-build | 8-17 min (MinGW install alone took 5-15 min) |
| **Tests (debug, software GPU)** | **84-136 min**: the critical path |

Inside the Tests job (run 44232382514):

- System packages: **32 min** in one run (the Azure Ubuntu mirror served 123 MB at 64 kB/s), 27 s in another.
  The download is mostly ffmpeg's dependency tree.
- Compile with a warm cache: 5.6 min.
- Running the tests: **96 min**, because `cargo test` runs one test binary after another, two tests at a time.
  The big binaries: `bpe-kit` lib (1,091 tests) 36 min, `bpe-gfx` lib 17.5 min, `liquid_sound` 8.6 min,
  `golden_liquid` 6.6 min (one test), `ready_look_pictures` 5.8 min (one test), `liquid_look_pictures` 3.6 min.
- Doc-tests: every crate has none (0 passed in all 8).

So the run is long because one machine runs ~96 min of tests in a row. More threads on the same machine are not
an option (the picture tests on the software driver run out of memory above two at a time), so the work has to go
to more machines.

## Decisions

1. **cargo-nextest, split across 4 machines.** The Tests job becomes a matrix of 4 shards, each running
   `cargo nextest run --workspace --partition count:<k>/4` with 2 tests at a time (the same memory limit per
   machine as today). nextest mixes the tests of every binary into one queue, so the 36-min `bpe-kit` lib is spread
   across all 4 shards instead of sitting in one. Each shard compiles the test binaries itself (5-6 min with the
   cache); passing a nextest archive between jobs was ruled out because the debug test binaries with debug info are
   several GB.
   - nextest runs each test in its own process. Every picture test already makes its own device with
     `test_device()`, so nothing shared between tests is lost; process isolation only removes cross-test leaks.
   - nextest does not run doc-tests. There are none today; a `cargo test --workspace --doc` step in shard 1 keeps
     them checked if any are added (a few seconds).
   - `#[ignore]` tests are skipped exactly as `cargo test` skips them. No retries are configured: a flaky test must
     stay visible.
   - `fail-fast = false` (same as `--no-fail-fast` today) so one failure doesn't hide others.
   - nextest is installed as a prebuilt binary with `taiki-e/install-action` (seconds, no compile).
2. **A gate job keeps the old check name.** A job named exactly `Tests (debug, software GPU)` needs all 4 shards and
   fails unless every one passed, so anything that looks for that name (branch protection, the merge rule in
   CLAUDE.md) still works.
3. **System packages come from a cache.** The downloaded `.deb` files are kept with `actions/cache` (key includes the
   package list and the Ubuntu version), so a slow mirror day costs nothing after the first run. `apt-get install`
   uses `--no-install-recommends` (ffmpeg's recommends pull in VA drivers and more we never use) and man-db's
   rebuild trigger is switched off (it adds about a minute on hosted runners). The same applies to the MinGW install
   in the Windows job and the packages in Clippy and Goldens.
4. **Build cache.** Already in place (`Swatinem/rust-cache`). The shards share one cache key (same build), and only
   shard 1 saves it so four shards don't race to upload the same few GB. Its `prefix-key` moves to `v2` so the
   first run doesn't restore a cache with a different test profile layout.
5. **Nothing CI checks is weakened.** Same tests, same debug profile, same two-at-a-time, same goldens, same
   Windows build, same clippy and fmt. Only where and in what order the tests run changes.
6. **A note for focused local runs** goes in a new `docs/notes/checks.md` (one index line in CLAUDE.md, a new area),
   covering: running one crate, one test file, one test by name, nextest locally, and the two-at-a-time limit for
   the picture tests in the cloud container.

## Files

- `.github/workflows/ci.yml`: the changes above.
- `.config/nextest.toml`: a `ci` profile (`fail-fast = false`, `test-threads = 2`, `slow-timeout` to print slow
  tests, `final-status-level = "slow"`), and the default profile with `test-threads = 2` so a local
  `cargo nextest run` is memory-safe.
- `docs/notes/checks.md`: new note.
- `CLAUDE.md`: point the Checks row at `checks.md`; mention nextest beside the `cargo test` line.

## Things not to do

- Don't raise the per-machine thread count, add retries, skip or `#[ignore]` any test.
- Don't change test code to make sharding work; if a test fails only under nextest, fix the real shared-state bug.
- Don't touch the goldens or the release job's test list.

## Verification

- The PR's own CI run: every shard green, the gate green, every other job green.
- Count of tests run equals today's (cargo test ran 1091 + 361 + ... ; nextest prints a total per shard; the sum of
  the 4 shards must equal `cargo nextest list --workspace` total).
- Report wall time before (84-136 min) and after from the PR's run.
