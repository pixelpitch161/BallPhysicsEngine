# Sound editor and ball limits (2026-11-05)

Aedan's list (thread "Improve sound editor and ball limits"), plus two follow-ups. Each item is
its own task with its own tests. The LFO editor's layout was picked from mockups
(`/mnt/project-files/ballphysics/lfo-editor/mockups.html`, layout **A, wide editor**).

Readings chosen where the ask could be read two ways:

- "Display below the mixer layers (replace the sends and pan settings)": the picked effect's
  editor (or the Master's) is drawn once, across the three bus columns, in the band where the
  strips' Sends and Pan rows are. The strip tops (name, Layers list) stay above it and the
  faders, meters and Mute/Solo stay below. A single strip column (~110 px) is too narrow for an
  EQ curve, so the editor spans the bus columns; the Master column keeps its own knobs.
- "Mixer opens to the same size as the Thing Tree": every time the left panel switches to the
  Mixer it opens at the width the tree has now; it can still be dragged wider while open.
- "Light up a note when it's played": both the keyboard key and the slot-grid cell of every note
  sounding on the shape light in the shape's colour, and stay lit at least 0.15 s so a short hit
  is seen.

## Task 1: No speed limit, No size limit (bpe-kit bounce)

- `crates/bpe-kit/src/modules/bounce/mod.rs`, `BasicBounce`: add
  `no_speed_limit: bool` (label "No speed limit", group "Bounce", default **true**, `simple`) just
  before `top_speed`, and `no_size_limit: bool` (label "No size limit", group "Growing", default
  **true**) before `max_size`. `top_speed` gets
  `when = "thing!=shape and !no_speed_limit or takes_over and !no_speed_limit"`, `max_size` the
  same with `no_size_limit`.
- `sim.rs`: `native` and `respond_pair`: the top is `limits.ceiling` when `no_speed_limit`
  (the world's own safety ceiling still applies), else `top_speed.min(ceiling)`. Size: the
  largest is `MAX_RADIUS` (new `pub const MAX_RADIUS: f32 = 100_000.0`, a safety cap) when
  `no_size_limit`, else `max_size.max(0.5)`. Factor both into `fn top_speed(b, limits)` and
  `fn biggest(b)`.
- Goldens must not move: `scenes.rs` gets `pub fn classic_bounce() -> BasicBounce` (both flags
  false) used by `Builder::bounce` and the `lively` card, and `classic_cards` turns any
  `bounce.basic` card whose params do not set the flags into flags-off (set
  `no_speed_limit`/`no_size_limit` false when the key is absent), so test scenes keep the old caps.
- `lib.rs` test `keys_in("Bounce")`/`keys_in("Growing")` lists gain the new keys.
- Tests (sim.rs): `top_speed_caps_a_lively_ball` sets `no_speed_limit` false; new
  `no_speed_limit_lets_a_ball_pass_top_speed` (default card, gain 1.5, speed above 3000 after
  hits); `grow_and_speed_gain_apply_on_each_hit` sets `no_size_limit` false; new
  `no_size_limit_grows_past_biggest_size`.
- Verify: `cargo test -p bpe-kit` (goldens unchanged, debug and release).

## Task 2: Remove two preview hints

- `crates/bpe-studio/src/preview.rs`: drop `draw_hint(ui, rect, "Click to pause")` while playing,
  and the "Drags snap to the grid · hold Alt to place freely" hint (no hint while the grid snaps
  in Things mode; the camera and no-snap hints stay). Fix any test that looks for them.

## Task 3: Phase line in the Osc wave view

- `crates/bpe-studio/src/sound_tab/wave_view.rs`: `show(.., phase: Option<f32>, spread: f32)`:
  `phase` in degrees (None for noise), `spread` the Random phase 0..1. Draws, under the wave, a
  band `spread` of a cycle wide centred on the line (faint) when spread > 0, and a 1 px vertical
  line in the shape's colour at `phase/360` of each of the two cycles drawn (x = (phase/360)/2
  and 0.5 + that). The levels test id stays; add `phase_x(phase) -> [f32; 2]` (0..1 across) as a
  pure helper with a unit test (0° at 0 and 0.5; 90° at 0.125 and 0.625).
- `osc.rs` passes `cx.number(&key("phase"))` and `random_phase / 100`.

## Task 4: Notes light up while they sound

- `crates/bpe-sound/src/meters.rs`: per shape place `notes: [AtomicU64; 2]` (a 128-bit set of MIDI
  notes). `write_notes(place, mask: u128)` ORs the notes sounding this block in (so a note shorter
  than a frame is still seen); `take_notes(thing) -> u128` swaps it to 0. Cleared with the
  shape's other meters in `clear_shapes_from` and when the place's thing changes.
- `crates/bpe-sound/src/voices/shape.rs`: at the end of a block, the notes of every voice that is
  sounding (gate on, or releasing and not yet silent) go into the mask; `ShapeVoice` writes it
  where it writes its slot (`write_slot`). Drone notes count. No allocation.
- Editor: `SoundTab.lit: [f64; 128]` (time each note stays lit until). Each frame
  `take_notes(thing)`; each note in the mask is lit until `now + 0.15`. `keys.rs` draws a lit key
  filled with the shape colour (black keys a darker mix); `notes/grid.rs` draws a lit cell of the
  playing slot (from `shape_slot`) brighter, with an outline. Repaint while anything is lit.
- Tests: meters unit test (write/take, OR of two writes, other thing reads 0); a bpe-sound test
  that a hit on a test patch lights note 60 in the mask; a kittest that a lit note paints
  (state check through a pure `lit_now(&lit, now)` helper).

## Task 5: EQ layer on the oscillators

- `crates/bpe-kit/src/sound/chain.rs`: `pub const EQ: &str = "chain.eq"`; `CHAIN_KINDS` gains it
  (4 kinds). `ChainEqSettings`: `band1..band8: EqBand` (prefixes `band1_`.. as `EqSettings`,
  same defaults: band 1 a Low cut that is on, the rest off bells at `BAND_FREQS`), `output`
  (-24..24 dB), `mix` (0..100 %), `solo`. `chain_kind!(EqLayerKind, EQ, "EQ", "An 8-band
  equaliser: cuts, shelves, bells and notches.", ChainEqSettings)`, registered in `lib.rs`
  beside the others. `layer_summary` gives "N bands" (bands on).
- `crates/bpe-dsp/src/fx/eq.rs`: `Eq8`, `Band`, `Target`, `Svf` derive `Clone`; add
  `tick(l, r) -> (f32, f32)` running one sample (for loop-back chains: per-sample mode) without
  breaking `process`'s block path; a test that 64 ticks equal one 64-block `process`.
- `crates/bpe-sound/src/voices/knobs.rs`: `CHAIN_KEYS` gains the 24 band numbers
  (`band1_freq`, `band1_gain`, `band1_q`, .. `band8_q`) after the 14 there; constants
  `BAND_FREQ0` etc. via `fn band_key(band, part)` index helper `EQ_BANDS_AT = 14`. `CHAIN_LEN`
  becomes 38 (layout moves, routes are by key so files are fine).
- `crates/bpe-sound/src/chain.rs`: `LayerType::Eq`; `LayerDsp::Eq(Box<Eq8>, [EqBand flags])`:
  at build the band on/shape/slope/place come from the settings (`band_params` in
  `fx_chain.rs`), numbers from the knobs each block; Output through `set_output_db`; per-sample
  mode calls `tick`, block mode runs `process` on the layer's buffer then mixes. Patch build
  (`patch.rs` where chain layer types are made) maps `EQ` to `LayerType::Eq` with the bands'
  fixed parts. `same_type` true for two EQs; `copy_from` clones the Eq8 state.
- Editor: `sound_tab/strip.rs` `Settings::Eq`; the open strip shows band chips 1..8 (lit when on)
  and the picked band's On / Shape / Freq / Gain / Q / Slope / Stereo, then Output and Mix; the
  64 px view (`strip_views.rs`) draws the response (`band_response_db` summed) with a dot per
  band that is on; drag a dot for freq and gain, wheel for Q, double-click the curve to turn on
  the nearest off band there (as the Mixer EQ). The folded row's mini view draws the curve.
  Band pick kept in `SoundTab.eq_band: HashMap<LayerId, usize>`.
- Tests: bpe-kit settings round trip; bpe-sound: a chain EQ bell at +12 dB at 1 kHz lifts a
  1 kHz sine by ~12 dB in a note; adopt keeps it ringing; render does not allocate with an EQ
  layer (extend the counting test's patch); studio: adding an EQ layer from "+ Add layer".

## Task 6: Mixer: effect editor under the layers, opens at the tree's width

- `crates/bpe-studio/src/mixer/strips.rs`: split `bus` into `bus_top` (on switch, name, Layers),
  `bus_sends` (Sends, Pan) and `bus_bottom` (fader, meter, dB, GR, M/S); `master` into
  `master_top` (title), `master_knobs` and `master_bottom` (meter, GR). `mixer::show` lays out
  three rows of columns: tops; then either each bus's sends + the Master knobs (nothing picked),
  or the picked editor across the three bus columns (`width*3 + 2*GAP`) beside the Master knobs;
  then the bottoms. Columns line up because every top is the same height (the Layers list
  already has a fixed height).
- `mixer/editors/mod.rs` `show` keeps its title row and Close button but draws without its own
  ScrollArea (the Mixer scrolls); editors use `ui.available_width()` (check `eq.rs` CURVE_H 300
  -> 200 when narrow: `CURVE_H.min(width * 0.6)`).
- `app.rs`: the inspector no longer shows Mixer editors (`mixing` goes; the inspector always
  shows the selected thing). The Mixer panel's id is `("mixer", self.mixer_opens)` with
  `default_size(self.tree_width)`, where `tree_width` is the tree panel's last drawn width and
  `mixer_opens` counts switches to the Mixer, so each open starts at the tree's width.
  `min_size` 180 (as the tree). Strip widths below ~70 px still draw (knobs shrink).
- Clicking the picked effect again, or Close, closes the editor.
- Tests: app tests for the Mixer width change to "opens at the tree's width" (drag the tree,
  switch, compare); a kittest that picking an EQ slot shows its curve in the left panel and not
  in the inspector, and that Sends knobs are gone while it is open and back after Close.

## Task 7: LFO editor like Serum 2 (layout A)

Data (bpe-core, bpe-kit):

- `bpe-core/src/sound_values.rs`: `CurvePoint { x: f32, y: f32, bend: f32 }` (x 0..1 sorted,
  first at 0, last at 1, equal x makes a jump; y 0..1; bend -1..1 the segment to the next
  point) with `is_valid`; `Value::Curve(Vec<CurvePoint>)`, `ValueType::Curve` (never keyframed),
  `SettingValue for Vec<CurvePoint>`, save/load round trip. `MAX_POINTS = 128`.
- `bpe-kit/src/sound/mods.rs`: `LfoShape` becomes `Drawn` (default), `Random`, `SmoothRandom`.
  `LfoSettings` gains `curve: Vec<CurvePoint>` (default the Sine preset), `once: bool` ("Once":
  plays the shape one time from its start and holds its end), `grid_x: i32` (1..32, default 8),
  `grid_y: i32` (1..16, default 4), `snap: bool` (default true). `LfoPreset` with `ALL`, `label`,
  `points()`: Sine, Triangle, Saw up, Saw down, Square, Pulse (25 %), Exp up, Exp down (Random
  and Smooth random are shapes, offered in the same menu).

Engine (bpe-dsp, bpe-sound):

- `bpe-dsp/src/lfo.rs`: `pub fn bend_at(t, bend) -> f32` (`t^(16^(-bend))`, so +1 bows up on a
  rise), `pub fn curve_at(points: &[CurvePoint-like (x,y,bend) slice], phase: f64) -> f32`
  (right-hand value at a jump; empty or one point reads its y or 0.5), `curve_end(points)`.
  `LfoWave::Curve`; `shape_at` keeps its signature for the others; new `wave_at(wave, curve,
  phase, seed)` dispatching. bpe-dsp gets its own `CurvePt` mirror? No: bpe-dsp does not depend
  on bpe-core, so `curve_at` takes `&[[f32; 3]]` and callers convert once at patch build.
- `bpe-sound/src/mods/lfo.rs`: `LfoParams` gains `once: bool`; `lfo_value(p, curve, cycles,
  since, seed)`: Once holds at cycle 1 (value at the curve's end, from the left, shifted by
  phase); `LfoMod` holds `curve: Arc<[[f32; 3]]>` set at build (`set_curve`), and its playhead
  `position()` (0..1 across the shape, Phase counted; 1 once held). `LfoSpec` gains
  `curve: Arc<[[f32; 3]]>`. `control.rs` (Follow sound) passes the spec's curve.
- Meters: `ModSink` gains the LFO's position: `write_mods` sink takes `(layer, value, pos)`
  (pos NaN for non-LFOs); `mod_position(thing, layer) -> Option<f32>`. Per-note LFOs report the
  newest note's.

Editor (`bpe-studio/src/sound_tab/mods/lfo.rs`, new `lfo_curve.rs`):

- Toolbar: Preset menu (presets + Random, Smooth random), Grid `x` × `y` drag numbers, Snap
  chip, Paint chips Points / Step / Ramp up / Ramp down, Flip ↕, Flip ↔, Clear.
- The curve: the whole tab width × 210 px, grid lines, fill under the curve, points (5 px),
  bend dots in each segment's middle, the playhead (line + dot) while playing.
  Points mode: drag a point (x between its neighbours, ends keep x), double-click empty to add,
  double-click a point to delete (not the ends), drag a bend dot up/down, Alt+click a bend dot
  to straighten, right-click a point: Delete point / Straighten both sides. Snap to the grid
  unless Shift. Paint modes: drag across cells; each cell under the pointer becomes the painted
  shape at the pointer's height (snapped to grid_y when Snap): Step flat, Ramp up 0→y, Ramp down
  y→0. One drag is one undo step.
  Random shapes show their wave (not editable) with a "Pick a drawn preset to draw" note.
- Under the curve: Hz | Tempo, note and feel, Free | Restart on hit | Per note, Once; the knobs
  Rate (Hz), Phase, Fade in, Delay, Smooth.
- Pure curve edits in `bpe-kit/src/sound/lfo_curve.rs` (`add_point`, `move_point`,
  `delete_point`, `set_bend`, `paint_cell`, `flip_y`, `flip_x`, `clear`, `tidy`), unit-tested.

Tests: dsp `curve_at` (linear segment, bend, jump, wrap), presets (Sine preset within 0.03 of a
sine), Once holds, engine vs Follow sound agree with a drawn LFO (`tests/follow.rs` pattern), edit
functions, a kittest that double-clicking the curve adds a point and that a preset replaces it.

## Order and verification

Tasks 1-4 first (small), then 5, 6, 7. Before the PR: `cargo test --workspace` (debug; goldens
untouched, re-run `--release` for bpe-kit goldens), `cargo clippy --workspace`, `cargo fmt`,
one Opus review of the whole diff, then the PR.
