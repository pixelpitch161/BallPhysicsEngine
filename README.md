# Ball Physics Engine

Design, preview and render satisfying bouncing-ball videos for TikTok, Instagram Reels and YouTube Shorts.
Written in Rust with wgpu. The output is a vertical 1080x1920 mp4 with sound. Developed for Windows 11, and also builds on macOS and Linux.

The default scene recreates a reference reel: a 28 x 28 grid of glowing balls falls into a ring, and every wall hit plays a note from a chord progression.

## Scene types

Mix and match these in the editor, or start from a preset of each:

| Scene type | How to make it | Preset |
| --- | --- | --- |
| Escape rings | Arena tab, shape **Escape rings**. Nested spinning rings each have a gap; a ball that gets out through a gap shatters that ring and plays a strum. Rings can be round or polygons. | Escape rings, Escape and multiply |
| Spinning shapes | Arena tab, shape **Polygon** (or any shape) and **Arena spin** under Movement. | Spinning hexagon |
| Multiplying balls | Bounces tab, **New balls per hit** or **per escape**, up to a ball limit. | Multiplying balls |
| Shrinking arena | Arena tab, **Shrink** (per second) or **Shrink per hit** under Movement. Negative shrink grows the arena. | Shrinking ring |
| String art | Look tab, **Lines per ball**: a line from each ball to its latest hit points. | String art |
| Pegs (Plinko) | Arena tab, **Pegs and walls**, plus Balls tab **Add a stream** to drop balls one by one. | Plinko |

## Requirements

### Windows 11

1. Install Rust from [rustup.rs](https://rustup.rs). When it asks, let it install the Visual Studio C++ Build Tools.
2. Install ffmpeg, then open a new terminal so it is on your `PATH`:
   ```powershell
   winget install --id Gyan.FFmpeg
   ffmpeg -version
   ```
3. Clone and run the editor:
   ```powershell
   git clone https://github.com/pixelpitch161/BallPhysicsEngine
   cd BallPhysicsEngine
   cargo run --release -p bpe-app
   ```
   The first build downloads and compiles dependencies and takes a few minutes. The editor uses DirectX 12 or Vulkan.

### macOS and Linux

- Rust 1.88 or newer from [rustup.rs](https://rustup.rs), and ffmpeg (`brew install ffmpeg` or `sudo apt install ffmpeg`).
- Linux also needs `pkg-config`, `libasound2-dev` and `libxkbcommon-x11-0` to build.

## Editor

```sh
cargo run --release -p bpe-app
# or open a scene file directly
cargo run --release -p bpe-app -- scenes/growing-ball.ron
```

- **Start:** the editor reopens the last scene you saved or opened. The first time, it shows the preset gallery.
- **Toolbar:** New and Presets open the preset gallery, each with a moving preview. Open, Save and Save as work with `.ron` scene files, which are plain text you can also edit by hand. Undo and Redo (Ctrl+Z, Ctrl+Y) cover every change. The editor asks before closing or opening another scene with unsaved changes.
- **Settings tabs:** Balls, Arena, Bounces, Look, Effects and Sound. Every setting has a plain name and a **?** that explains it, and right-clicking a setting resets it to its default. Rarely used settings are under **More settings** in each tab.
- **Preview:** while paused, drag a ball group to move it, drag the tip of its arrow to set launch speed and direction, and drag the arena's edge to resize it. Clicking the preview while it plays pauses it.
- **Bottom bar:** play and pause (Space), back to the start, scrub through time, the video's length, loop and mute.
- **Export video:** pick Best, Good or Quick draft, and the editor saves an mp4 with sound to your Videos folder, named after the scene. It finds ffmpeg by itself, including a winget install that is not on `PATH` yet. Video size, frame rate and the ffmpeg location are under Advanced. When it finishes, Play video and Show in folder open the result.

## Shader effects

The Effects tab stacks full-screen shader effects over the finished picture, applied top to bottom: Echo, Kaleidoscope, Shockwaves, Colour split, Colour cycle, Liquid, Mirror, Pixels, Film and Glitch, plus ready-made looks that combine them. Each has an Amount and its own sliders, and they are saved in the scene and rendered into exports.

An effect is one WGSL file that defines `fn effect(uv: vec2<f32>, p: Params) -> vec3<f32>`, with its sliders declared in comments at the top:

```wgsl
// name: Wobble
// about: Bends the picture in waves.
// param strength = 0.5 [0, 1] How far the picture bends
// param bands = 8 [1, 40] int Number of waves
// param vertical = 0 toggle Wave up and down instead of sideways
fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {
    let along = select(uv.y, uv.x, p.vertical > 0.5);
    let wave = sin(along * p.bands * TAU + fx.time * 3.0) * 0.02 * p.strength;
    let direction = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), p.vertical > 0.5);
    return sample(uv + direction * wave);
}
```

Put custom effects in an `effects` folder next to the scene file or next to `ball-studio`. They appear in Add effect, and the preview reloads them whenever the file is saved; a mistake shows its message and line in the tab. The helpers available to effects (`sample`, `prev` for feedback, `noise`, `hue_shift`, the list of recent bounces and more) are described in `crates/bpe-render/src/effects/prelude.wgsl`, and the built-in effects in `crates/bpe-render/src/effects/builtin/` are good starting points.

## Command line

```sh
cargo run --release -p bpe-export -- scenes/reference-reel.ron out.mp4
cargo run --release -p bpe-export -- scenes/reference-reel.ron out.mp4 --scale 0.5 --preset veryfast   # quick draft
cargo run --release -p bpe-export -- scenes/reference-reel.ron --still 3.5 frame.png                  # one frame
cargo run --release -p bpe-export -- scenes/reference-reel.ron --wav sound.wav                        # just the audio
cargo run --release -p bpe-export -- escape-rings out.mp4                                              # a built-in preset
cargo run --release -p bpe-export -- --presets                                                         # list the presets
```

## How it fits together

| Crate | Job |
| --- | --- |
| `bpe-scene` | The scene description: video size, container, balls, physics, rules, look and sound. Saved as RON. |
| `bpe-sim` | Deterministic physics at a fixed 240 steps per second: closed arenas that spin and shrink, rings with gaps, pegs and walls, balls that appear or split mid-clip. It records every hit and escape with its exact time. |
| `bpe-audio` | Turns wall hits into a soundtrack. Each hit plays a note from the current chord at the exact sample, then the mix is loudness-normalised and peak-limited. |
| `bpe-render` | wgpu renderer: background, walls and pegs (instanced shapes with distance functions), string art, ring shards, motion streaks, shaded balls, wall flashes, then the shader effect chain, HDR bloom and tone mapping. |
| `bpe-export` | Pipes rendered frames and the soundtrack into ffmpeg (H.264 and AAC in an mp4). Also the `bpe-render` command-line tool. |
| `bpe-app` | The egui editor (`ball-studio`). |

The physics never uses the screen's frame time, so the preview, the export and the sound always agree. Running a scene twice gives identical frames.

World coordinates are pixels of the output video, with the origin at the centre of the frame and y pointing down.

## Tests

```sh
cargo test --workspace
```

The renderer test needs a GPU or a software Vulkan driver (`mesa-vulkan-drivers` on Linux). It skips itself if none is found.
