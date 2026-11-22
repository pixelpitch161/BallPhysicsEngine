//! The sound path never allocates. Alone in its own test binary, since the counting allocator
//! sees every thread.

use bpe_core::{Settings, SlotId};
use bpe_kit::sound::{LIGHT_MOD, SubWave, VoiceSettings, add_modulator, starter_voice};
use bpe_kit::things::light::{LightSettings, LightType, add_light};
use bpe_kit::{kit, new_scene, resolve_scene};
use bpe_sound::{BLOCK, Engine};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Counts allocations while switched on, on any thread.
struct Counting;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn note(size: usize) {
    if COUNTING.swap(false, Ordering::SeqCst) {
        let bt = std::backtrace::Backtrace::force_capture();
        let name = std::thread::current().name().map(|s| s.to_string());
        SEEN.lock().unwrap().push(format!("size {size} thread {name:?}\n{bt}"));
        COUNTING.store(true, Ordering::SeqCst);
    }
}
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            note(layout.size());
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            note(new_size);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn render_does_not_allocate() {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    let shape = scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap();
    scene.things.get_mut(&shape).unwrap().tags.insert("test-tone".into());
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut e = Engine::new(&scene, &resolved, &kit);
    let mut block = [0.0f32; BLOCK * 2];
    for _ in 0..200 {
        e.render(&mut block);
    }
    // The physics steps between blocks (it may allocate as it grows its log); the sound path
    // itself never does.
    let mut heard = false;
    for _ in 0..1_000 {
        e.step_physics();
        ALLOCATIONS.store(0, Ordering::SeqCst);
        COUNTING.store(true, Ordering::SeqCst);
        e.render_prepared(&mut block);
        COUNTING.store(false, Ordering::SeqCst);
        if ALLOCATIONS.load(Ordering::SeqCst) != 0 { COUNTING.store(false, Ordering::SeqCst); for s in SEEN.lock().unwrap().iter() { eprintln!("ALLOC {s}"); } panic!("a block allocated"); }
        heard |= block.iter().any(|&v| v != 0.0);
    }
    assert!(heard, "hits were heard while counting");

}
