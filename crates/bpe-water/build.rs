//! Prints the sources a Water take depends on (an FNV-1a hash of every file under them, in path
//! order, line ends made alike), so a program whose water changed in any way never takes an older
//! take for its own. Only the physics and the take are printed: the drawing (`src/draw`,
//! `src/shaders`) and the test helpers never change what a take holds, so a look change keeps every
//! final (Water mode part 6).

use std::path::{Path, PathBuf};

/// The folders and files printed.
const ROOTS: &[&str] = &["src/drops", "src/take", "src/drop.rs", "src/solid.rs", "src/lib.rs"];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn main() {
    let mut all = Vec::new();
    for root in ROOTS {
        println!("cargo:rerun-if-changed={root}");
        files(Path::new(root), &mut all);
    }
    all.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for path in &all {
        let name = path.to_string_lossy().replace('\\', "/");
        let text = std::fs::read(path).unwrap_or_default();
        for b in name.bytes().chain(text.into_iter().filter(|b| *b != b'\r')) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("sources_print.rs");
    std::fs::write(out, format!("{h}u64")).expect("OUT_DIR can be written");
}
