# Plan: a boxed state is never mistaken for the state inside it

## The report

Particles part 2 found that `world.states.get(&id).and_then(|s| s.as_any().downcast_ref::<T>())` always gave
`None`, and suspected the same in `emitter/sim.rs::fire_now` (the rules' Fire block) and `shape/sim.rs::side_of`/
`side_on` (By side notes).

## Root cause (checked)

`bpe-runtime/src/plugins.rs` implements `ThingState` for every `T: Any + Clone + Send + Sync`. `Box<dyn ThingState>`
is `Clone` (its own impl) and `Arc<dyn ThingState>` is `Clone` anyway, so **the pointers are `ThingState`s too**, whose
`as_any` hands back the pointer, not what it points at. Which one a call gets depends on whether the trait is in scope:

- Trait **not** imported: `boxed.as_any()` can only reach the `dyn ThingState`'s own method through deref, and works.
  This is every place that reads `world.states` today (`emitter/sim.rs`, `shape/sim.rs`, `balls/rows.rs`,
  `balls/sim.rs`, `emitter/aim.rs`, `particles/sim.rs`, `bpe-studio/src/shape_handles.rs`), so the Fire block and
  side notes work on main: `rules::tests_part5::fire_a_round_fires_when_a_rule_asks` and
  `shape::sim::tests::side_of_follows_the_corners_and_the_turn` pass.
- Trait **imported** (`use bpe_runtime::ThingState`): method lookup finds the pointer's blanket impl first, and the
  downcast silently fails. Adding that one import to `shape/sim.rs` makes the side test fail ("side 1"). Particles part
  2 imported it, which is why its copy broke.
- Also, wherever the trait is imported or not: `&Box<dyn ThingState>` or `&Arc<dyn ThingState>` passed where a
  `&dyn ThingState` is wanted (`state::<T>(&boxed)`) unsize-coerces the **pointer** into the trait object, so `state`
  panics and `as_any` misses. No call site does this today (they use `.as_ref()`), but nothing stops it.

So the 14 call sites are correct only by luck of their imports. Fixing them one by one (`(**s).as_any()`) leaves the
trap for the next file.

## Fix (at the root, in `plugins.rs` only)

The blanket impl forwards when `Self` is itself a pointer to a state:

- `fn inner<T: Any>(s: &T) -> Option<&dyn ThingState>`: `Some(&**b)` when `T` is `Box<dyn ThingState>`, `Some(&**a)`
  when `T` is `Arc<dyn ThingState>`, else `None` (a `TypeId` test on `T`, folded away for every real state type).
- `as_any`: `inner(self).map_or(self, |s| s.as_any())`.
- `as_any_mut`: through the box for a `Box`; for an `Arc`, `Arc::get_mut` when it is the only holder, else a panic that
  says a shared state can't be changed (nothing does this: no call takes `&mut` through an `Arc`).
- `clone_box`: `inner(self).map_or_else(|| Box::new(self.clone()), |s| s.clone_box())`, so cloning a pointer gives the
  state itself, never a box holding a box. The `Clone for Box<dyn ThingState>` impl keeps calling `(**self).clone_box()`.

The call sites stay as they are: with the fix every form of the call reaches the state, imported or not. Not done:
changing the trait's shape, a marker trait on every state type, or a wrapper map type for `World.states` (each a wide
change for the same result).

## Tests (in `plugins.rs`'s tests, where the trait is in scope; written first, failing before the fix)

`a_pointer_to_a_state_is_read_as_the_state`: with `ThingState` imported, for a `Box` and an `Arc` holding a `u32`:
`p.as_any().downcast_ref::<u32>()`, the same through `Option::and_then` on a `BTreeMap::get` (the reported shape),
`state::<u32>(&p)` (the coercion), `as_any_mut` on a `Box` and on a lone `Arc`, and `clone_box` on each giving a state
whose `as_any` is the `u32` (not a pointer). Also a shared `Arc`'s `as_any_mut` panics with its message.

A check by hand, not kept: with the fix, adding `ThingState` to `shape/sim.rs`'s imports (the change that broke it
above) leaves `side_of_follows_the_corners_and_the_turn` passing.

## Verify

- New tests fail before the fix, pass after.
- `cargo test --workspace` (debug) passes; goldens unchanged (nothing moves: every current call already reached the
  state). `cargo clippy --workspace`, `cargo fmt --check`.
