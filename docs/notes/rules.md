# Rules (spec `docs/superpowers/specs/2026-11-06-rules-design.md`, plans `docs/superpowers/plans/rules/`)

Moved word for word from CLAUDE.md on 2026-11-08 (workstream W1). Add each finished part's "as built" summary here, not in CLAUDE.md.

Part 1 is in (plan `part1.md`, "As built" at its end): the headless engine core. `Scene.rules` (`bpe-core/src/rules/`:
`Rules`, `Rule { id, name, enabled, blocks }`, `Block { id, kind, inputs, params }`, `BlockSection` When / If / Logic /
Do, `Who` That / Other / Group / Thing{ball} / Tag / Kind / Anything as `Value::Who`; edits `add_rule` … `link`,
`set_block_param`; a number setting `k` may take its value from the firing via `k~from`/`~times`/`~plus`; format 12).
Every block kind is a plugin: core's `RuleBlockKind` (`Registry::add_block`) and the runtime's `BlockSim`
(`Sims::add_block`); the kit's 14 are in `bpe-kit/src/rules/` (`when.hit/time/beat`, `if.compare/is/chance/every/
cooldown/only/between`, `logic`, `do.motion/change/break`). The runtime (`bpe-runtime/src/rules/`) runs the **Rules
stage** at the end of every step (after the history record): each enabled rule without a problem, in order, Logic
ticks then Whens fire, firings flow depth first, Do blocks queue `Action`s (at most `MOST_ACTIONS` 4096 a rule a
step, else `busy_at`), and a `Rule` event (tag 10, `b.item` = `rule_item(rule, block)`) is logged per block that
fired. The actions apply at the next step's start (`World::apply_rule_actions`, before `animate`): motion changes,
freezes, **Break** (event tag 11, then Remove for a ball; a whole thing loses its wall and drops out of the order and
the frame) and setting **changes** (`changes.rs`: Set/Add/Multiply with glide, ease and lasts, folded over the keyed
value from a pristine copy each step so they never compound; physics keys re-prepare the cards, per-ball keys make
`Slotted.own` states, Size per ball through `World::ruled` in the Balls/Emitter sims, looks per ball in `Worn`,
whole looks in `build_frame_sensing`). Dice are `rules::dice` (seed, rule, block, step, subject). Nothing changes
for a scene without enabled rules (goldens, fingerprint); `tests/golden-rules*.txt` pin `scenes::rule_scenes`.

Part 2 is in (plan `part2.md`): the **editor**. The bottom panel switches Timeline / **Rules** (chips at the left of
the transport row, `settings::BottomView` saved in `bottom-view.txt`, `Studio.bottom`/`set_bottom`; the scrub bar
stays). `bpe-studio/src/rules_panel/`: `mod.rs` (`RulesPanel` in `Panels.rules`: `picked` block, rename, link and
rule drags, copied rule/block; `show` → `Outcome { separate, message }`, a structural edit is its own undo step through
`Studio.separate_edit` and `record_frame`), `layout.rs` (`lay_out`: column = longest path in, row = order added within
the column, links with their input index), `face.rs` (section, name, up to three simple settings or the kind's
`summary`; Change setting shows who, `setting_words`, how; who fields carry the thing's track-colour dot), `fired.rs`
(`Fired` from the log's Rule events: `count` of end-block moments, `recent` within `RING_SECONDS` 0.15, `firings`),
`menu.rs` (When / If / Logic / Do ▸ kinds), `setting_menu.rs` (thing ▸ card ▸ (layer ▸) setting, `cards_of`,
`picked_def`, "(whole group)" where one ball can't change alone) and `inspect.rs` (the inspector for a block, shown by
`Studio::picked_block` while no thing is selected: problems in `style::PROBLEM`, the settings through
`widgets::grid_row_with`, a "from" chip on each of the kind's `firing_keys` (`RuleBlockKind::firing_keys`, `values()` names
what a When's firings carry, both via `block_kind!`'s `values = [..], from = [..]` tail), Compare's Value menu, Change
setting's To shown as the picked setting's own type). `widgets::who_picker` draws every `Value::Who` row
(`TargetChoices.relative`, `ball_things`). Lanes: header (switch, name renamed in place, "fired N times", red problem
dot named "Problem in <rule>", "too busy at"), blocks named "<Section> <name>" ("Logic" alone), out-dots "Link from …",
"Add after …", "Add When to <rule>", right-click menus for headers, blocks and lines. The timeline has a Rules lane
under the Hits lane while the scene has rules (`rules_lane`).

Part 3 is in (plan `part3.md`): **break looks**, drawn only. `do.break`'s Look is Vanish / Fade out (Time) / Pop (Time,
Swell) / Shatter (Pieces, Spread, Spin, Gravity, Hit shapes, Bounce, Fade after, Fade over; `breaks.rs`: `LookSettings`
under the same keys, `show_for` gives a `BreakShow { look: Params, lasts }`, lasts 0 for Vanish and for kinds that
can't show a look (only shape, balls, emitter, light can)). `Action::Break.show` (an `Arc<BreakShow>`); applying it
keeps `RulesState.remains` (`bpe-runtime/src/rules/remains.rs`: `Remains` by (thing, row or `WHOLE`), with the hit,
the gravity, a ball's `BallAt` or a shape's taken wall; `remains_of`, `showing`, pruned when over). The physics never
reads them; the log and goldens are unchanged. `build_frame_sensing` (and the light probe) keep a broken thing while
`showing`. The kit's `rules/remains/` (`showing` per look, `shatter.rs`: `cut` = a seeded Voronoi of the square round
what is drawn, sites on the material (`Material`/`Edge`: inside, or a band along the outline), `piece_at` closed form or
1/120 s steps with `keep_clear` against the world's walls; `draw.rs`: `shatter_shape`, `ball_remains`). Each piece is the
thing's own instance again, moved and turned, with `PathInstance.clip`/`LayeredInstance.clip` = [first corner in
`path_points`, count, fade, 0] (vertex attribute 6 / 12; group 2 binding 4 is now seen by the vertex stage, which fits
a piece's quad to its cell; `clip_keep` in paths.wgsl, used by `shade_path`, `shade_shape_layers`' `keep`,
`shade_layers` and the object passes; all-zero clip draws as before). A broken light's strength follows its look
(`remains::whole_now`); a broken emitter's marker goes at once. `tests/break_pictures.rs` checks the pictures.

Part 5 is in (plan `part5.md`, "As built" at its end): **more events and actions**, 13 kit blocks. Whens
`when.enter` (Enters / Leaves / Either a zone, `rules/events.rs`: each (zone, thing, row) inside set worked out from the
world at prepare and diffed every stage; a catcher's `Enter` events too), `when.fired` (an emitter's balls, `Spawn`
events, or rounds, `EmitterState.fired`), `when.removed` (Removed / Broken), `when.count` (`counts.rs`: hits so far,
ball count or a counter rising/falling to an amount), `when.level` (`level.rs`: a ball's speed, size, light or shadow,
or a shape's/bus's sound crossing a level; `BlockSim::hears_sound`/`sees_light` put the sound/light makers'
fingerprints into the world's), `when.signal`; If `if.counter`; Do `do.split` (`balls.rs`: a ball becomes `into`
balls fanned over Spread, sizes shared by area or kept, each keeping the ball's number, own size, rule changes and
tags, `World::split`, `RulesState::copy_row`), `do.spawn` (new balls in a Balls or Emitter thing through
`ThingSim::add_ball(NewBall)`), `do.fire` (an emitter's round through `ThingSim::fire_now`; Fire when **By a rule**,
`FireWhen::ByARule`, `EmitterState.asked`), `do.tag` (a thing's tag in `world.tags`, a ball's own in
`RulesState::ball_tags`, which `who::matches(cx, ..)` reads; the physics sees only things' tags), `do.count`
(`RulesState::counters`, Add / Set / Reset), `do.signal`. Spawned balls count against `MAX_BALLS` live and
`MAX_ROWS` rows. Counters, tags, splits and spawns apply at the next step's start; a **signal** (`Action::Signal`,
`rules::Signal`) is heard in the same stage by `BlockSim::hear`, in up to `MOST_ROUNDS` 8 more rounds, the last
round's carried to the next step (`RulesState.carried`). A shape's **Balls pass through** (`ShapeSettings.passes`,
`Wall.passes`) keeps its wall for everything but the collision pass, so a shape can be a zone.

Part 4 is in (plan `part4.md`): **Health**. A **Health card** (`SlotId::Health`, last in `SlotId::ALL`; kind `health`,
`bpe-kit/src/modules/health/`: Starting health (varies across balls at launch), Hits take Each hit / By strength /
Nothing, Least strength, Only hits from (a `Target`), At zero Break / Nothing) goes on balls, emitters and shapes from
"+ Add slot". The world keeps health in `RulesState.health` (`bpe-runtime/src/rules/health.rs`: `Health`, `Hp` per body
row, one for a thing without balls; the kit's `HealthSim` turns the card into a `HealthCard`); a scene with a Health card
and no rule still gets a `RulesState`. The Rules stage first `fill`s new balls' starting health and works out the
step's hits (`damage`, at zero the card's `at_zero` template `aimed` into `pending`), then runs the rules with
`StageCx.health`; each change is a `HealthMoment` (cleared after the stage). `Action::Health { thing, row, change }`
(`HealthChange` Damage/Heal/Set/Refill) applies at the next step's start, a zero Break in the same pass. A ball split off another (part 5's Split) keeps its health (`Health::copy_row`). Blocks
`when.health` (Drops / Reaches zero / Goes up / Changes), `if.health` (Below/Above/Between/At zero/Full, Points or
Percent), `do.health` (`bpe-kit/src/rules/health.rs`); Hit firings and Compare carry `health`/`share`/`start`. **Follow
health** is `PulseSource::Health { thing }` read through `Inputs.health` (`HealthLevels`, `World::health_levels`: a
thing's share, a group's mean over live balls); the spread **`VaryBy::Health`** reads `BallSpread.hurt` (1 − share,
`World::hurt`/`hurts`; Balls/Emitter Size through `own_sizes`). The fingerprint hashes Health cards. Editor: Follow health
▸ (`follow::health_menu`), the Health spread (`VaryCx::healthy`). Goldens `tests/golden-rules-health*.txt`
(`scenes::health_scenes`). At zero Break takes part 3's Break looks (the card's `look` … `fade_over`, `look_settings`, the template's `show`).

Part 6 is in (plan `part6.md`): the **spectacle actions** `do.camera` (Shake / Zoom punch: `Action::Kick`, kept in
`RulesState.kicks` (`Kick`, `MOST_KICKS` 64, weight (1 − age/time)²), added by the camera sim after its own shake and
punch), `do.slow` (Slow motion: `Action::Pace`, part 1's `Change` folded on a pace of 1 (`RulesState::pace_at`, 0.01×
to 8×); each step `World.pace` scales the physics' dt (`World::sim_dt`, the ceiling ÷ pace, the resting threshold, the
shape's spin and shrink), the video clock, keys, camera, schedules and draws run on; `RulesState.lag`,
`World::sim_time`, and `SimCx.time` reads the simulated clock, exactly `step / rate` while `lag` is 0), `do.flash`
(`EventKind::Flash`, tag 20, one per ball, counted in `RulesState.flashes`: balls' `last_hits_but`, the Flash on hit
sprite scan and `GroupCx::hits`, shapes' `shape_hits` read it; nothing else does), `do.light_scene` (`SceneOp`,
`ShownScene` in `RulesState.scene`, resolved at apply by `next_scene`; `light_scenes::now` shows it over the card's own
Show, crossfading from what showed) and `do.note` (`EventKind::Note`, tag 21: `b` = (shape, pitch or `AS_IF_HIT`),
`normal.x` length, `speed` velocity; `HitCursor` places it, `ShapeVoice::rule_note` plays it past the hit filter (a
drone on its own voice), `catch_up` replays its picks, `ControlSampler` hears it, MIDI records it). Tags 12..19 are left
for parts 3 to 5. `tests/golden-rules*.txt` gain "Slow motion"; `bpe-sound/tests/rule_notes.rs`.

**Small links (W12, 2026-11-08).** Break looks show on a Particles thing too (`breaks::shows_looks`; the particles
fade by the look's alpha in `particles/draw.rs::draw_around`), and a Break naming a Particles thing by itself, kind,
tag or Anything keeps it stepping in the main world (`BlockSim::keeps`), so the editor and export agree. **Flash** on
a Light or Rig flares the light (see `lighting.md`). Tests in `spectacle_tests.rs`
(`a_flash_flares_a_light_and_every_light_of_a_rig`, `a_broken_particles_thing_fades_out`).
