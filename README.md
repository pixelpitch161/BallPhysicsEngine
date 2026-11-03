# Ball Physics Engine

Design, preview and render satisfying bouncing-ball videos for TikTok, Instagram Reels and YouTube Shorts.
Written in Rust with wgpu. The output is a vertical 1080x1920 mp4. Developed for Windows 11, and also builds on
Linux and macOS.

This is the stripped-down core after Version 1 (kept on the `version-1` branch). It has balls, arenas, a camera,
folders, colours and looks, trails, particles, shader effects, keyframes and pulses on hits, the scene settings, and
mp4 export. The video's sound track is silent for now.

## Programs

- **Ball Studio** (`bpe-studio`, binary `ball-studio-next`): the editor. The tree of things is on the left, the
  selected thing's settings and one card per slot (Look, Trail, Motion, Bounce, Particles, Shader) on the right, the
  preview in the middle and the timeline of keyframes at the bottom.
- **bpe-render-next** (`bpe-cli`): renders a scene file to an mp4 from the command line.

```sh
cargo run --release -p bpe-studio                                     # the editor
cargo run --release -p bpe-studio -- scenes/next/reel-block.ron        # open a scene
cargo run --release -p bpe-cli -- scenes/next/reel-block.ron out.mp4   # export from the command line
cargo run --release -p bpe-cli -- scenes/next/reel-block.ron out.mp4 --scale 0.5 --frames 300 --encoder cpu
```

Export needs ffmpeg. The editor and the renderer find it by themselves, including a winget install that is not on
`PATH` yet.

## Scenes

The built-in scenes are saved in `scenes/next/`: one ball, reel block, box drop, spinning hexagon, shrinking ring,
ball pit, riders, lava ball, confetti drop, spark block, kaleido hexagon, neon ring, colour paints, blend modes,
graded sunset and long drop.

## How it fits together

| Crate | Job |
| --- | --- |
| `bpe-settings-derive` | `#[derive(Settings, Choice)]`: a plain struct becomes the settings the inspector, dice and keyframes use. |
| `bpe-core` | The scene model: the tree of things, the module library, values, colours, keyframes and pulses, save and load, and resolve. |
| `bpe-runtime` | The deterministic world at a fixed 240 steps per second: collisions, the camera and the event log. |
| `bpe-gfx` | The wgpu renderer: gradients, blend modes, the grade, shader effects, particles and trails. |
| `bpe-kit` | Every thing kind (arena, balls, camera, folder, scene) and module kind (look, trail, motion, bounce, particles, shader), and the built-in scenes. |
| `bpe-cli` | Headless export. |
| `bpe-studio` | The egui editor. |

The physics never uses the screen's frame time, so the preview and the export always agree, and running a scene
twice gives identical frames. World coordinates are pixels of the output video, with the origin at the centre of
the frame and y pointing down.

## Tests

```sh
cargo test --workspace
```

The picture tests need a GPU or a software Vulkan driver (`mesa-vulkan-drivers` on Linux).
