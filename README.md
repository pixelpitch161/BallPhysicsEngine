# Ball Physics Engine

Design, preview and render satisfying bouncing-ball videos for TikTok, Instagram Reels and YouTube Shorts.
Written in Rust with wgpu. The output is a vertical 1080x1920 mp4 with sound. Developed for Windows 11, and also builds on macOS and Linux.

The default scene recreates a reference reel: a 28 x 28 grid of glowing balls falls into a ring, and every wall hit plays a note from a chord progression.

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

- **Left panel:** every property of the scene, including container, balls, physics, wall-hit rules, look and sound. Changes show immediately in the preview.
- **Bottom bar:** play and pause (Space), restart, scrub through time, loop and mute.
- **File menu:** open and save scenes as `.ron` files, which are plain text you can also edit by hand.
- **Presets menu:** built-in starting points.
- **Export video:** renders every frame at full quality and encodes an mp4 with sound. *Half size* makes a quick draft.

## Command line

```sh
cargo run --release -p bpe-export -- scenes/reference-reel.ron out.mp4
cargo run --release -p bpe-export -- scenes/reference-reel.ron out.mp4 --scale 0.5 --preset veryfast   # quick draft
cargo run --release -p bpe-export -- scenes/reference-reel.ron --still 3.5 frame.png                  # one frame
cargo run --release -p bpe-export -- scenes/reference-reel.ron --wav sound.wav                        # just the audio
```

## How it fits together

| Crate | Job |
| --- | --- |
| `bpe-scene` | The scene description: video size, container, balls, physics, rules, look and sound. Saved as RON. |
| `bpe-sim` | Deterministic physics at a fixed 240 steps per second. It records every wall hit with its exact time. |
| `bpe-audio` | Turns wall hits into a soundtrack. Each hit plays a note from the current chord at the exact sample, then the mix is loudness-normalised and peak-limited. |
| `bpe-render` | wgpu renderer: background, container, motion streaks, shaded balls, wall flashes, then HDR bloom and tone mapping. |
| `bpe-export` | Pipes rendered frames and the soundtrack into ffmpeg (H.264 and AAC in an mp4). Also the `bpe-render` command-line tool. |
| `bpe-app` | The egui editor (`ball-studio`). |

The physics never uses the screen's frame time, so the preview, the export and the sound always agree. Running a scene twice gives identical frames.

World coordinates are pixels of the output video, with the origin at the centre of the frame and y pointing down.

## Tests

```sh
cargo test --workspace
```

The renderer test needs a GPU or a software Vulkan driver (`mesa-vulkan-drivers` on Linux). It skips itself if none is found.
