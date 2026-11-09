use super::*;
use bpe_core::{Value, resolve};
use bpe_runtime::Log;

use crate::modules::look::layers::particles::emit::hash;
use crate::testing::{BALLS, SHAPE, scene_with_balls};
use crate::things::particles::births::unit;
use crate::things::{light, particles, text};

fn world_of(scene: &Scene) -> World {
    let kit = crate::kit();
    World::build(scene, &resolve(scene, &kit.registry), &kit.sims)
}

fn ride(on: RidesOn) -> Ride {
    Ride { on, ball_number: 3, edge_at: 0.25 }
}

#[test]
fn every_rider_is_offered_what_fits_its_parent() {
    use RidesOn::*;
    let (l, p, t) = (light::LIGHT, particles::KIND, text::KIND);
    assert_eq!(offered(l, "balls"), [EachBall, TheMiddle, BallNumber, TheEdge]);
    assert_eq!(offered(p, "emitter"), [EachBall, TheMiddle, BallNumber, TheEdge, Inside]);
    assert_eq!(offered(t, "balls"), [EachBall, TheMiddle, BallNumber, TheEdge]);
    assert_eq!(offered(l, "shape"), [TheMiddle, TheEdge]);
    assert_eq!(offered(p, "shape"), [TheMiddle, TheEdge, Inside]);
    assert_eq!(offered(l, "text"), [TheMiddle, EachLetter, TheEdge, ThePen]);
    assert_eq!(offered(p, "text"), [TheMiddle, EachLetter, TheEdge, Inside, ThePen]);
    assert!(can_ride(p, "text") && can_ride(l, "text") && !can_ride(t, "text"), "a Text doesn't ride a Text");
    assert_eq!(offered(t, "text").len(), RidesOn::NAMES.len(), "riding nothing: every choice");
    assert_eq!(
        RidesOn::NAMES,
        ["each_ball", "the_middle", "ball_number", "the_edge", "inside", "each_letter", "the_pen"]
    );
}

#[test]
fn balls_give_each_ball_one_ball_their_middle_and_their_rims() {
    let (scene, _, _) = scene_with_balls("ring", |_| {});
    let world = world_of(&scene);
    let b = &world.bodies[&BALLS];
    let ridden = Ridden::of(&world, Some(BALLS), |_| None);
    let each = ridden.spots(&ride(RidesOn::EachBall)).unwrap();
    assert_eq!(each.len(), b.len());
    assert!(each.iter().enumerate().all(|(r, s)| s.at == b.position[r] && s.ball == Some((BALLS, r))));
    assert_eq!(each[0].radius, b.radius[0]);
    let third = ridden.spots(&ride(RidesOn::BallNumber)).unwrap();
    assert_eq!(third.iter().map(|s| s.ball).collect::<Vec<_>>(), [Some((BALLS, b.row_of(2).unwrap()))], "from 1");
    let middle = (0..b.len()).map(|r| b.position[r]).sum::<Vec2>() / b.len() as f32;
    for on in [RidesOn::TheMiddle, RidesOn::EachLetter, RidesOn::ThePen, RidesOn::Inside] {
        let m = ridden.spots(&ride(on)).unwrap();
        assert!(m.len() == 1 && m[0].at.distance(middle) < 1e-3 && m[0].ball.is_none(), "{on:?}: the middle");
    }
    // A quarter round each rim from its top, clockwise: its right.
    let rims = ridden.spots(&ride(RidesOn::TheEdge)).unwrap();
    assert_eq!(rims.len(), b.len());
    for (r, s) in rims.iter().enumerate() {
        assert!((s.at - (b.position[r] + Vec2::X * b.radius[r])).length() < 1e-3, "{:?}", s.at);
    }
    assert_eq!(ridden.count(&ride(RidesOn::TheEdge)), b.len());
    assert_eq!(ridden.count(&ride(RidesOn::TheMiddle)), 1);
    // Every ball gone: nowhere.
    let mut world = world;
    world.bodies.get_mut(&BALLS).unwrap().alive.iter_mut().for_each(|a| *a = false);
    let ridden = Ridden::of(&world, Some(BALLS), |_| None);
    assert_eq!(ridden.spots(&ride(RidesOn::TheMiddle)), Some(Vec::new()));
}

#[test]
fn a_shape_gives_its_middle_and_a_point_round_its_outline() {
    let (scene, _, _) = scene_with_balls("single", |_| {});
    let world = world_of(&scene);
    let wall = &world.walls[&SHAPE].outline;
    let ridden = Ridden::of(&world, Some(SHAPE), |_| None);
    for on in [RidesOn::EachBall, RidesOn::TheMiddle, RidesOn::BallNumber] {
        assert_eq!(ridden.spots(&ride(on)).unwrap()[0].at, outline::centre(wall), "{on:?}");
    }
    let edge = ridden.spots(&ride(RidesOn::TheEdge)).unwrap();
    assert_eq!(edge[0].at, outline::round_one(&outline::points(wall), 0.25).unwrap().0);
    // Nothing the world moves: the rider keeps its own place.
    assert!(Ridden::of(&world, Some(ThingId::ROOT), |_| None).spots(&ride(RidesOn::EachBall)).is_none());
    assert!(Ridden::of(&world, None, |_| None).spots(&ride(RidesOn::EachBall)).is_none());
}

/// Where a particle riding balls or a shape was born before W14, written out as it was.
fn old_place(
    world: &World,
    parent: ThingId,
    on: RidesOn,
    ball_number: u32,
    seed: u32,
) -> Option<(Vec2, Option<Vec2>, u32)> {
    let (u1, u2) = (unit(seed, 1), unit(seed, 2));
    if let Some(b) = world.bodies.get(&parent) {
        let balls: Vec<(usize, Vec2, Vec2, f32)> =
            (0..b.len()).filter(|&r| b.alive[r]).map(|r| (r, b.position[r], b.velocity[r], b.radius[r])).collect();
        if balls.is_empty() {
            return None;
        }
        let pick = balls[(hash(&[seed, 7]) as usize) % balls.len()];
        return Some(match on {
            RidesOn::EachBall => (pick.1, None, pick.0 as u32 + 1),
            RidesOn::TheMiddle => (balls.iter().map(|b| b.1).sum::<Vec2>() / balls.len() as f32, None, 0),
            RidesOn::BallNumber => {
                let row = b.row_of(ball_number.saturating_sub(1)).filter(|&r| b.alive[r])?;
                (b.position[row], None, row as u32 + 1)
            }
            RidesOn::TheEdge => {
                let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                (pick.1 + out * pick.3, Some(out), pick.0 as u32 + 1)
            }
            _ => {
                let out = Vec2::from_angle(u1 * std::f32::consts::TAU);
                (pick.1 + out * pick.3 * u2.sqrt(), Some(out), pick.0 as u32 + 1)
            }
        });
    }
    let wall = world.walls.get(&parent)?;
    let centre = wall.outline.center();
    let points = outline::points(&wall.outline);
    Some(match on {
        RidesOn::TheEdge => {
            let (at, out) = outline::round_one(&points, u1).unwrap();
            (at, Some(out), 0)
        }
        RidesOn::Inside => {
            let (lo, hi) =
                points.iter().fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
                    (lo.min(*p), hi.max(*p))
                });
            let tried = (0..8).find_map(|k| {
                let p = lo + (hi - lo) * Vec2::new(unit(seed, 20 + 2 * k), unit(seed, 21 + 2 * k));
                outline::inside(&points, p).then_some(p)
            });
            let at = tried.unwrap_or(centre);
            (at, (at - centre).try_normalize(), 0)
        }
        _ => (centre, None, 0),
    })
}

#[test]
fn particles_are_born_on_balls_and_shapes_where_they_always_were() {
    let (scene, _, _) = scene_with_balls("ring", |_| {});
    let mut world = world_of(&scene);
    // A ball gone, so the picks skip it as before.
    world.bodies.get_mut(&BALLS).unwrap().alive[4] = false;
    for parent in [BALLS, SHAPE] {
        let ridden = Ridden::of(&world, Some(parent), |_| None);
        for on in [RidesOn::EachBall, RidesOn::TheMiddle, RidesOn::BallNumber, RidesOn::TheEdge, RidesOn::Inside] {
            for ball_number in [3, 5] {
                for seed in 0..300u32 {
                    let seed = seed.wrapping_mul(0x9e37_79b9);
                    let u = |k: u32| unit(seed, k);
                    let dice = Dice { pick: hash(&[seed, 7]) as usize, unit: &u };
                    let r = Ride { on, ball_number, edge_at: 0.0 };
                    let new =
                        ridden.pick(&r, &dice).map(|s| (s.at, s.out, s.ball.map_or(0, |(_, row)| row as u32 + 1)));
                    assert_eq!(new, old_place(&world, parent, on, ball_number, seed), "{parent:?} {on:?} {seed}");
                }
            }
        }
    }
}

/// A Text saying `words` in the world at `at`, with what `f` changes.
fn a_text(words: &str, at: Vec2, f: impl FnOnce(&mut Scene, ThingId)) -> (Scene, ThingId) {
    let reg = crate::registry();
    let mut scene = Scene::empty(&reg);
    let id = crate::add_thing(&mut scene, &reg, text::KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.params.set("words", Value::Text(words.into()));
    t.transform.position = at;
    t.screen = Some(false);
    f(&mut scene, id);
    (scene, id)
}

fn run_to(scene: &Scene, time: f64) -> World {
    let kit = crate::kit();
    let mut world = World::build(scene, &resolve(scene, &kit.registry), &kit.sims);
    world.advance_to(time, &kit.sims, &mut Log::default());
    world
}

#[test]
fn a_text_gives_its_letters_their_outlines_and_its_middle() {
    let (scene, id) = a_text("HI", Vec2::new(200.0, -100.0), |_, _| {});
    let world = run_to(&scene, 0.0);
    let ridden = Ridden::of(&world, Some(id), |id| letters_in_world(&world, id).map(Rc::new));
    let Ridden::Text(t) = &ridden else { panic!("a Text") };
    assert_eq!(t.shown().len(), 2);
    let middle = ridden.spots(&ride(RidesOn::TheMiddle)).unwrap()[0].at;
    assert!(middle.distance(Vec2::new(200.0, -100.0)) < 100.0, "{middle}");
    for on in [RidesOn::EachBall, RidesOn::BallNumber, RidesOn::Inside] {
        assert_eq!(ridden.spots(&ride(on)).unwrap()[0].at, middle, "{on:?}: the middle");
    }
    let letters = ridden.spots(&ride(RidesOn::EachLetter)).unwrap();
    assert_eq!(letters.len(), 2);
    assert!(letters[0].at.x < middle.x && letters[1].at.x > middle.x, "H left, I right");
    assert_eq!(ridden.count(&ride(RidesOn::EachLetter)), 2);
    let near_an_outline = |p: Vec2| {
        let sides = t
            .shown()
            .iter()
            .flat_map(|(_, c)| c.iter())
            .flat_map(|c| (0..c.len()).map(|i| (c[i], c[(i + 1) % c.len()])));
        sides.into_iter().any(|(a, b)| bpe_core::shape_path::nearest_on_segment(a, b, p).2 < 0.01)
    };
    let edge = ridden.spots(&ride(RidesOn::TheEdge)).unwrap();
    assert!(edge.len() == 1 && near_an_outline(edge[0].at), "{:?}", edge[0].at);
    assert_eq!(ridden.spots(&ride(RidesOn::ThePen)), Some(Vec::new()), "no Draw on: no pen");
    // Particles: born on the letters' outlines, inside them, on a letter.
    let all: Vec<Vec<Vec2>> = t.shown().iter().flat_map(|(_, c)| c.iter().cloned()).collect();
    for seed in 0..200u32 {
        let u = |k: u32| unit(seed, k);
        let dice = Dice { pick: seed as usize, unit: &u };
        let on_edge = ridden.pick(&ride(RidesOn::TheEdge), &dice).unwrap();
        assert!(near_an_outline(on_edge.at) && on_edge.out.is_some());
        let inside = ridden.pick(&ride(RidesOn::Inside), &dice).unwrap();
        let on_a_letter = letters.iter().any(|l| l.at == inside.at);
        assert!(outline::inside_any(&all, inside.at) || on_a_letter, "{:?}", inside.at);
        let letter = ridden.pick(&ride(RidesOn::EachLetter), &dice).unwrap();
        assert_eq!(letter.at, letters[seed as usize % 2].at);
        assert!(ridden.pick(&ride(RidesOn::ThePen), &dice).is_none());
    }
}

#[test]
fn a_screen_text_is_ridden_where_the_camera_shows_it_and_its_pen_moves() {
    use crate::modules::look::shape::moves::DRAW_ON;
    let (scene, id) = a_text("ABCD", Vec2::new(0.0, 300.0), |scene, id| {
        scene.things.get_mut(&id).unwrap().screen = Some(true);
        crate::testing::set_layer(scene, id, DRAW_ON, "takes", Value::Number(4.0));
    });
    let world = run_to(&scene, 1.0);
    let ridden = Ridden::of(&world, Some(id), |id| letters_in_world(&world, id).map(Rc::new));
    let Ridden::Text(t) = &ridden else { panic!("a Text") };
    assert!(t.screen);
    let pen = ridden.spots(&ride(RidesOn::ThePen)).unwrap();
    assert_eq!(pen.len(), 1, "the sim sees the Look card's Draw on");
    let middle = ridden.spots(&ride(RidesOn::TheMiddle)).unwrap()[0].at;
    assert!(middle.distance(world.camera.to_world(Vec2::new(0.0, 300.0))) < 100.0, "{middle}");
    let later = run_to(&scene, 2.5);
    let ridden_later = Ridden::of(&later, Some(id), |id| letters_in_world(&later, id).map(Rc::new));
    let pen_later = ridden_later.spots(&ride(RidesOn::ThePen)).unwrap();
    assert!(pen_later[0].at.distance(pen[0].at) > 1.0, "the pen moves on");
    let done = run_to(&scene, 6.0);
    assert!(!done.walls.contains_key(&id), "a screen Text is no wall");
    let ridden_done = Ridden::of(&done, Some(id), |id| letters_in_world(&done, id).map(Rc::new));
    assert_eq!(ridden_done.spots(&ride(RidesOn::ThePen)), Some(Vec::new()), "done: no pen");
}

#[test]
fn a_thing_added_inside_a_shape_or_a_text_starts_on_the_middle() {
    let reg = crate::registry();
    let (mut scene, _, _) = scene_with_balls("single", |_| {});
    let txt = crate::add_thing(&mut scene, &reg, text::KIND, ThingId::ROOT).unwrap();
    let choice = |scene: &Scene, id: ThingId| scene.things[&id].params.get("rides_on").cloned();
    let middle = Some(Value::Choice("the_middle".into()));
    let lamp = light::add_light(&mut scene, &reg, light::LightType::Point, SHAPE).unwrap();
    assert_eq!(choice(&scene, lamp), middle, "a shape has no balls");
    let lamp = light::add_light(&mut scene, &reg, light::LightType::Point, txt).unwrap();
    assert_eq!(choice(&scene, lamp), middle);
    let specks = crate::add_thing(&mut scene, &reg, particles::KIND, txt).unwrap();
    assert_eq!(choice(&scene, specks), middle);
    let label = crate::add_thing(&mut scene, &reg, text::KIND, SHAPE).unwrap();
    assert_eq!(choice(&scene, label), middle);
    let on_balls = light::add_light(&mut scene, &reg, light::LightType::Point, BALLS).unwrap();
    assert_eq!(choice(&scene, on_balls), None, "Each ball fits balls");
    assert!(offers(&scene, on_balls, "each_ball") && !offers(&scene, lamp, "each_ball"));
    assert!(offers(&scene, lamp, "each_letter") && !offers(&scene, label, "the_pen"));
    // A light in a rig rides what the rig is in.
    let rig = crate::add_thing(&mut scene, &reg, crate::things::rig::RIG, SHAPE).unwrap();
    let in_rig = light::add_light(&mut scene, &reg, light::LightType::Point, rig).unwrap();
    assert!(!offers(&scene, in_rig, "ball_number") && offers(&scene, in_rig, "the_edge"));
    let kind = reg.thing(light::LIGHT).unwrap();
    assert!(!kind.offers(&scene, in_rig, "rides_on", "each_ball") && kind.offers(&scene, in_rig, "kind", "spot"));
}
