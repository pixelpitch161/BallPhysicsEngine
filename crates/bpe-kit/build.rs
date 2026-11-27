//! Prints the sources a Water take depends on (an FNV-1a hash of every file under them, in path
//! order, line ends made alike), so a program whose water changed in any way never takes an older
//! take for its own. The look's files (`look.rs`, `looks.rs`) are left out: what a scene looks like
//! never changes what a take holds, so a look change keeps every final (Water mode part 6).

use std::path::{Path, PathBuf};

/// The folders printed.
const ROOTS: &[&str] = &["src/things/water", "src/modules/water"];
/// The files under them that only say how the water looks, never printed.
const LOOKS: &[&str] = &["look.rs", "looks.rs"];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, out);
        } else if !LOOKS.iter().any(|name| path.file_name().is_some_and(|f| f == *name)) {
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
