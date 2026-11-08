# W4: never lose work, exports and errors

Workstream W4 of the 2026-11-08 improvement review (report "Ball Studio code review: what to improve", findings E1 to
E6 and C4). Aedan picked it on 2026-11-08 ("everything besides Size"). Editor-only: no physics, picture or sound
changes, so goldens and picture hashes must not move. W5 (editor workflow) touches the same files and starts after
this merges.

## Goal

| Finding | What changes for Aedan |
| --- | --- |
| E1 | The editor autosaves every minute. After a crash or power cut the next start asks "Ball Studio closed unexpectedly. Restore your last work?" |
| E2 | Saving never leaves a half-written scene: it writes a temporary file and swaps it in, and keeps the previous version as `<name> (backup).ron`. |
| C4 | A graphics-driver complaint shows as a problem in the status line instead of closing the editor. A crash still saves a recovery copy first. |
| E3 | Closing while a video exports asks "A video is still exporting (40%). Stop it and close?". The status line shows the percentage, then "Video saved" or the failure. |
| E4 | Failures (save, open, speakers, graphics card, export) show in the problem red with a × to dismiss, and stay until dismissed, so "Undone." can't hide them. |
| E5 | A failed export says what to do in a plain sentence ("The video is open in another program. Close it and try again."), with ffmpeg's log behind a Details fold that scrolls and copies. |
| E6 | The export window remembers size, compression, encoder, stems, MIDI and the ffmpeg location between starts. |

## Decisions

### Status line and problems (E4), `app.rs`

- `Studio.status: String` stays the info line (grey, replaced freely by "Undone.", "Saved x." and so on).
- New `Studio.problem: Option<Problem>` with `Problem { text: String, kind: ProblemKind }`. `ProblemKind` is
  `Speakers`, `Graphics`, `File`, `Export`, `Recovery` so one source can clear only its own problem (the speakers
  coming back clears a `Speakers` problem, never a save failure).
- `Studio::report(kind, text)` sets it (newest problem wins); `Studio::clear_problem(kind)` clears it if it is of that
  kind. The status line draws the problem first: a painted warning dot and the text in `style::PROBLEM`, truncated
  with the full text on hover, then a small "×" button labelled "Dismiss" that sets it to `None`. The info line
  follows in `MUTED`.
- The shader-skipped readout uses `style::PROBLEM` too (it is a problem; it already clears itself).
- Callers moved to `report`: open failures (unreadable file, old format, bad file), save failures, autosave write
  failure (`Recovery`, reported once until a write succeeds), speakers stopped (`Speakers`, cleared by "The speakers
  are back."), sound thread stopped (`Speakers`), GPU errors and device lost (`Graphics`), export failures (`Export`),
  a recovery file that can't be read (`Recovery`). A successful save or open clears `File`.
- No new colours: `style::PROBLEM` already exists (the design-system test forbids colours outside `style/`).

### Safe saves (E2), new `files.rs`

- `pub fn write_safely(path: &Path, text: &str, backup: bool) -> io::Result<()>`:
  1. write `<file name>.tmp` beside `path` with `File::create`, `write_all`, `sync_all` (a power cut can't leave the
     swap pointing at unwritten blocks);
  2. if `backup` and `path` exists, `fs::copy(path, backup_path(path))` (copy, not rename, so the real file is never
     missing even for a moment);
  3. `fs::rename(tmp, path)` (replaces on Windows too: Rust uses `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`).
  On any error the `.tmp` file is removed and the original is untouched.
- `pub fn backup_path(path) -> PathBuf`: `My scene.ron` becomes `My scene (backup).ron`: it opens straight from the
  Open dialog (a `.bak` extension would need renaming, and Windows hides extensions). One backup, the version before
  the last save.
- `Studio::save` uses `write_safely(path, text, true)`. The settings files keep their plain writes (losing a width is
  harmless); the palette store already writes safely.

### Autosave and recovery (E1), new `recovery.rs`

- Folder `settings_dir()/recovery/`. Each running editor owns one session: `session-<pid>-<start millis>.ron` (the
  scene), `.path` (the scene's own file, or empty) and `.lock` (held with `File::try_lock`, stable since Rust 1.89,
  for the editor's whole life; the operating system lets go of it when the process dies, crash or not).
- `Recovery::start(dir) -> Option<Recovery>` makes the folder and takes the lock; `None` (no autosave) if either
  fails, reported once as a `Recovery` problem only when the folder exists but can't be written.
- Writing: a single worker thread fed by an `mpsc` channel of `Job::Write { scene: Box<Scene>, path }` and
  `Job::Clear`. The worker serialises with `bpe_core::save` and writes with `write_safely(.., false)`, so the UI thread
  only clones the scene. Failures come back on a second channel and become one problem.
- When: `Studio::tick` calls `autosave(now)`. Every `AUTOSAVE_EVERY = 60 s`, if the scene is unsaved and differs from
  the last autosaved copy, it is sent. When the scene is saved (or matches the file again), opened, or new, the session
  is cleared (files deleted). A clean exit (`on_exit`) clears it too, as does "Don't save" on close.
- Orphans: `recovery::orphans(dir) -> Vec<Orphan>`: every `session-*.ron` whose `.lock` can be locked by us (owner
  gone) or is missing, newest first (file modified time). `Orphan { scene_file, original: Option<PathBuf>, saved_at:
  SystemTime }`.
- On start (`Studio::new`, windowed editor only, also when a scene was opened from the command line: the prompt shows
  over whatever opened), the newest orphan is offered in a modal: heading "Ball Studio closed
  unexpectedly", text "Restore your last work? It was saved automatically <n minutes ago / at hh:mm>." plus the
  scene's name, buttons **Restore** (primary) and **Discard**. Restore loads it with `bpe_core::load`, calls
  `replace_scene(scene, original)`, then sets `saved` to the original file's scene when it loads (so the unsaved dot
  is honest) or to a blank studio scene otherwise, and autosaves into our own session at once. Discard deletes the
  orphan's files. Either way, older orphans stay on disk and are offered at the next start, one per start (nothing is
  ever deleted that wasn't offered).
- An orphan that won't load reports a `Recovery` problem "Couldn't restore the autosaved work: <why>" and is kept on
  disk (renamed `…ron.broken`) so it is never offered again but isn't lost.
- Tests use a temporary folder; a `Studio` without a window (`persist_settings == false`) never autosaves.

### Graphics errors and crashes (C4)

- `Studio::new`: with `cc.wgpu_render_state`, call `device.on_uncaptured_error(handler)`. The handler (it runs on any
  thread) pushes the error's text into an `Arc<Mutex<GpuErrors>>` (`first: Option<String>`, `count: u32`) and requests
  a repaint. `Studio::tick` reads it: the first error becomes a `Graphics` problem "The graphics card reported a
  problem: <first line>", later ones only bump the count ("(3 times)"). The full text of the first error goes into the
  problem's hover. wgpu no longer panics, so the editor keeps running.
- `device.set_device_lost_callback`: reports "The graphics card stopped responding. Your work was saved for recovery;
  close and reopen Ball Studio." and autosaves at once (the scene itself is fine; only drawing is gone). A lost reason
  of `Destroyed` (our own shutdown) is ignored.
- The export thread sets its own `on_uncaptured_error` on its headless device: an error is kept and the progress
  callback stops the export, which then fails with that error (explained by E5) instead of panicking the thread.
  A job whose channel disconnects (its thread panicked) ends as "The export stopped unexpectedly." rather than looking
  busy forever. Preview final frame gets the same disconnect handling.
- Crash hook: `recovery::install_crash_hook(snapshot)` wraps the default panic hook. The snapshot is an
  `Arc<Mutex<Option<CrashCopy>>>` (`scene`, `path`, session file paths) that `Studio` refreshes at most once a second
  while the scene is unsaved and has changed (a clone, no serialising). The hook `try_lock`s it (never blocks:
  the panic may have happened while it was held), serialises and writes the session file directly, writes
  `last-crash.txt` (message, location, time) in the settings folder, then runs the default hook. A panic on another
  thread that the editor survives (a caught one) leaves a session file that the clean exit deletes, so nothing is
  offered by mistake.

### Export window (E3, E5, E6), `export_window.rs`

- `ExportWindow::progress_percent() -> Option<u32>` from the job's done/total.
- `ExportWindow::take_news() -> Option<ExportNews>`, set once when a job ends: `Saved { name, seconds }`,
  `Failed { headline }`, `Cancelled`. `Studio` turns it into the info line "Video saved: <name> (<time>)." or a
  problem "The export failed: <headline>" (Cancelled: info "Export cancelled.").
- Status line: while busy, "· Exporting 40%" as a small link-styled button that opens the export window.
- Closing: the close request goes through `guard_close`: if an export is busy, a modal "A video is still exporting
  (40%). Stop it and close?" with **Stop and close** and **Keep exporting**; Stop runs `shutdown()` then continues
  to the usual unsaved-changes question. `Pending::Close` handling is otherwise unchanged.
- E5 `explain_export_error(raw: &str) -> Explained { headline: String, details: String }` (pure, tested), matching
  lower-cased text, first match wins:
  - "permission denied" / "access is denied" / "being used by another process" / "resource busy" → "Ball Studio
    couldn't write the video. If it's open in a player, close it and try again; otherwise choose another folder
    with Change…".
  - "no space left" / "disk full" / "not enough space" → "The disk is full. Free some space, or save the video on
    another drive with Change…".
  - "could not start ffmpeg" → "Ball Studio couldn't start ffmpeg. Point to ffmpeg.exe again with Choose…".
  - "unknown encoder" / "encoder not found" / "no such encoder" → "This copy of ffmpeg can't make H.264 videos.
    Install a full build of ffmpeg, then point to it with Choose…".
  - "out of memory" / "outofmemory" → "The graphics card ran out of memory. Try Half size, or Live lighting instead of
    Bake.".
  - "adapter" (no adapter found) → "Ball Studio couldn't find a graphics card to draw the video with.".
  - "did not come back from the graphics card" / "device lost" / "graphics card reported" → "The graphics card
    stopped while drawing the video. Try again; if it keeps happening, restart Ball Studio.".
  - otherwise → "Something went wrong while making the video.".
  The details are the raw text. The window shows the headline in `style::PROBLEM`, then a collapsed "Details" header
  holding a `ScrollArea` (max 160 px) of the raw text (selectable, small monospace) and a "Copy details" button.
- Before starting, if the target file exists and can't be opened for writing, the export fails at once with the
  "couldn't write" sentence instead of after the sound render.
- E6 `export-next.txt` in the settings folder, `key=value` lines like `grid-next.txt`: `scale`, `crf`, `encoder`
  (`Encoder::from_name` names: auto, cpu, nvidia, amd, intel), `stems`, `midi`, `ffmpeg`. Not the file (it belongs to
  the scene) and not the Bake choice (it overrides one scene's Lighting card). `ExportWindow::load_in(dir)` reads it,
  clamping crf to 14..=28 and scale to the three sizes; a remembered ffmpeg is used only if the file still exists,
  else `find_ffmpeg()`. `Studio::new` loads it (windowed editor only). The window saves when its remembered choices
  differ from what was last written (checked each frame the window is open, so a slider drag writes once it changes,
  a few small writes at most).
- An `Encoder::key()` (`"auto"`, `"cpu"`, `"nvidia"`, `"amd"`, `"intel"`) is added in bpe-cli beside `from_name`.

## Files

- New: `crates/bpe-studio/src/files.rs`, `crates/bpe-studio/src/recovery.rs`.
- Changed: `main.rs` (modules), `app.rs` (problem, autosave, restore modal, close guard, GPU handler, export news),
  `export_window.rs` (news, percent, explain, details, remember, disconnect, pre-check, export GPU handler),
  `final_frame.rs` (disconnect), `settings.rs` (nothing new beyond `settings_dir`), `timeline/mod.rs`, tree message
  site (unchanged: info), `bpe-cli/src/lib.rs` (`Encoder::key`), `docs/notes/editor.md` (as built).

## Not doing

- No autosave into the scene's own file (Aedan saves on purpose; the recovery copy is separate).
- No multiple backup generations, no recovery browser: one prompt per start is enough.
- No window-title change (E14 is W5).
- No change to what an export draws.

## Verification

- Unit tests:
  - `files`: a save over an existing file keeps the old text in `(backup).ron` and leaves no `.tmp`; a write into a
    missing folder fails and leaves the original untouched; `backup_path` names.
  - `recovery`: a session writes and clears; a session whose lock is held is not an orphan, and is one after it is
    dropped; orphans come newest first; a broken orphan is renamed `.broken`.
  - `app`: `report` / `clear_problem` by kind; the speakers coming back clears only a speakers problem; a failed open
    is a problem and keeps the scene; autosave sends only when unsaved and changed, and clears on save; restoring an
    orphan gives the recovered scene, its path and an honest unsaved state.
  - `export_window`: `explain_export_error` for each case; export choices round-trip through `export-next.txt`
    with garbage ignored; a missing remembered ffmpeg falls back; news after a finished job; a disconnected job ends.
  - kittest: the status line shows a problem with a Dismiss button that clears it; the close-while-exporting modal
    appears and "Keep exporting" keeps the job.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p bpe-studio -p bpe-cli
  -- --test-threads=2`, then CI's full run on the PR. No golden or picture output changes (editor only).
- One Opus review of the whole part before the PR (project rule).

## As built

Built as planned, with these differences:

- `recovery::tidy` deletes the lock files of editors that are gone and left no work (a kill with nothing unsaved
  left a stray `.lock`).
- The restore question is "Restore your last work on "<name>"? It was saved automatically <when>." and the info line
  after it "Restored your work (saved automatically <when>)."
- Preview final frame got the same device error handler and disconnect handling as the export.
- The Stop-export modal makes **Keep exporting** the primary button (the safe choice) and says the unfinished video
  will be deleted.
- Checked in the running editor under Xvfb: an added Balls group was autosaved at once, a `kill -9` left the copy,
  and the next start offered it back; Restore brought the Balls back under "Not saved yet".

Summary in `docs/notes/editor.md` ("Never lose work, exports and errors").
