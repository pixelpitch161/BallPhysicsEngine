//! Pictures for picture splats: PNG and JPG files read once, when a canvas's layers start,
//! fitted into a 512 by 512 square and kept as layers of one texture array with every mip level.
//! A file that cannot be read gets a pink and grey checker in its place, never a panic.

use std::path::{Path, PathBuf};

/// The side of every picture layer in texels.
pub const PICTURE_SIDE: u32 = 512;
/// Mip levels of a layer: 512 down to 1.
pub const PICTURE_MIPS: u32 = 10;
/// The checker's two colours, sRGB.
pub const CHECKER: [[u8; 4]; 2] = [[0xff, 0x4f, 0xd8, 0xff], [0x20, 0x20, 0x20, 0xff]];

const SIDE: usize = PICTURE_SIDE as usize;

/// The file `path` names: as it is when absolute, else inside `folder` (or relative to the
/// working folder with no folder). Backslashes are taken as separators off Windows.
pub fn picture_path(path: &str, folder: Option<&Path>) -> PathBuf {
    let text = if cfg!(windows) { path.to_string() } else { path.replace('\\', "/") };
    let p = Path::new(&text);
    match folder {
        Some(folder) if !p.is_absolute() => folder.join(p),
        _ => p.to_path_buf(),
    }
}

/// The checker layer: 32 px squares, `CHECKER[0]` where (x/32 + y/32) is even.
pub fn checker_layer() -> Vec<u8> {
    let mut out = Vec::with_capacity(SIDE * SIDE * 4);
    for y in 0..SIDE {
        for x in 0..SIDE {
            out.extend_from_slice(&CHECKER[(x / 32 + y / 32) % 2]);
        }
    }
    out
}

/// The 512 by 512 RGBA8 layer for raw RGBA `pixels` of `w` by `h`: fitted with its shape kept,
/// centred, and transparent around it.
pub fn fit_layer(pixels: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut layer = vec![0u8; SIDE * SIDE * 4];
    let Some(source) = image::RgbaImage::from_raw(w, h, pixels.to_vec()) else { return layer };
    if w == 0 || h == 0 {
        return layer;
    }
    let scale = PICTURE_SIDE as f32 / w.max(h) as f32;
    let nw = ((w as f32 * scale).round() as u32).clamp(1, PICTURE_SIDE);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, PICTURE_SIDE);
    let small = image::imageops::resize(&source, nw, nh, image::imageops::FilterType::Triangle);
    let (ox, oy) = ((PICTURE_SIDE - nw) / 2, (PICTURE_SIDE - nh) / 2);
    for y in 0..nh {
        for x in 0..nw {
            let i = (((oy + y) * PICTURE_SIDE + ox + x) * 4) as usize;
            layer[i..i + 4].copy_from_slice(&small.get_pixel(x, y).0);
        }
    }
    layer
}

/// The mip chain under `top` (512 squared), top level first: each level averages 2 by 2 blocks,
/// colour weighted by alpha, alpha averaged. Integer maths, so it never varies.
pub fn mips(top: Vec<u8>) -> Vec<Vec<u8>> {
    let mut levels = vec![top];
    let mut side = SIDE;
    while side > 1 {
        let from = levels.last().unwrap();
        let half = side / 2;
        let mut next = vec![0u8; half * half * 4];
        for y in 0..half {
            for x in 0..half {
                let (mut sum_a, mut sum) = (0u32, [0u32; 3]);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let i = ((2 * y + dy) * side + 2 * x + dx) * 4;
                    let a = from[i + 3] as u32;
                    sum_a += a;
                    for c in 0..3 {
                        sum[c] += from[i + c] as u32 * a;
                    }
                }
                let o = (y * half + x) * 4;
                next[o + 3] = ((sum_a + 2) / 4) as u8;
                for c in 0..3 {
                    next[o + c] = (sum[c] + sum_a / 2).checked_div(sum_a).unwrap_or(0) as u8;
                }
            }
        }
        levels.push(next);
        side = half;
    }
    levels
}

/// A picture file read, fitted and mipped; or the checker (and `false`) when it can't be read.
pub fn picture_levels(path: &Path) -> (Vec<Vec<u8>>, bool) {
    let read = || -> Option<Vec<u8>> {
        let image = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
        let rgba = image.to_rgba8();
        Some(fit_layer(rgba.as_raw(), rgba.width(), rgba.height()))
    };
    match read() {
        Some(top) => (mips(top), true),
        None => (mips(checker_layer()), false),
    }
}

/// Whether `path` is a picture the atlas can read (the header only, so it is cheap).
pub fn picture_readable(path: &Path) -> bool {
    image::ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .is_some_and(|r| r.into_dimensions().is_ok())
}

/// What a loaded atlas was made from: the files named, where they were read from, and when each
/// was last written. The same key means the same atlas, so it is not decoded again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AtlasKey {
    paths: Vec<String>,
    folder: Option<PathBuf>,
    written: Vec<Option<std::time::SystemTime>>,
}

impl AtlasKey {
    pub(crate) fn new(paths: &[String], folder: Option<&Path>) -> Self {
        let written =
            paths.iter().map(|p| std::fs::metadata(picture_path(p, folder)).and_then(|m| m.modified()).ok()).collect();
        Self { paths: paths.to_vec(), folder: folder.map(Path::to_path_buf), written }
    }
}

/// Every picture of a scene as layers of one texture array, and how to sample them.
#[derive(Clone)]
pub(crate) struct PictureAtlas {
    pub(crate) view: wgpu::TextureView,
    pub(crate) sampler: wgpu::Sampler,
    /// Pictures held; 0 when the scene has none.
    pub(crate) layers: u32,
    /// The files that could not be read, as given.
    pub(crate) missing: Vec<String>,
}

impl PictureAtlas {
    /// One layer per path, or a single checker layer (never drawn) when `paths` is empty.
    pub(crate) fn load(device: &wgpu::Device, queue: &wgpu::Queue, paths: &[String], folder: Option<&Path>) -> Self {
        let mut missing = Vec::new();
        let mut layers: Vec<Vec<Vec<u8>>> = Vec::new();
        for given in paths {
            let (levels, ok) = picture_levels(&picture_path(given, folder));
            if !ok {
                missing.push(given.clone());
            }
            layers.push(levels);
        }
        if layers.is_empty() {
            layers.push(mips(checker_layer()));
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("canvas pictures"),
            size: wgpu::Extent3d {
                width: PICTURE_SIDE,
                height: PICTURE_SIDE,
                depth_or_array_layers: layers.len() as u32,
            },
            mip_level_count: PICTURE_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, levels) in layers.iter().enumerate() {
            for (mip, data) in levels.iter().enumerate() {
                let side = (PICTURE_SIDE >> mip).max(1);
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: mip as u32,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(side * 4),
                        rows_per_image: Some(side),
                    },
                    wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
                );
            }
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("canvas pictures"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self { view, sampler, layers: paths.len() as u32, missing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bpe-pic-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn red_png(path: &Path, w: u32, h: u32) {
        image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 255])).save(path).unwrap();
    }

    fn at(layer: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * SIDE + x) * 4;
        [layer[i], layer[i + 1], layer[i + 2], layer[i + 3]]
    }

    #[test]
    fn fit_layer_letterboxes_a_wide_picture() {
        let pixels = [255u8, 0, 0, 255].repeat(200 * 100);
        let layer = fit_layer(&pixels, 200, 100);
        assert_eq!(at(&layer, 256, 256), [255, 0, 0, 255]);
        assert_eq!(at(&layer, 256, 60)[3], 0);
        assert_eq!(at(&layer, 256, 130)[3], 255);
    }

    #[test]
    fn missing_picture_gets_the_checker() {
        let (levels, ok) = picture_levels(Path::new("/no/such/file.png"));
        assert!(!ok);
        assert_eq!(levels.len(), PICTURE_MIPS as usize);
        assert_eq!(levels[0], checker_layer());
    }

    #[test]
    fn broken_picture_gets_the_checker() {
        let dir = temp("broken");
        let bad = dir.join("bad.png");
        std::fs::write(&bad, [7u8, 200, 13, 99, 250, 1, 2, 3, 77, 140].repeat(10)).unwrap();
        let (levels, ok) = picture_levels(&bad);
        assert!(!ok);
        assert_eq!(levels[0], checker_layer());
        assert!(!picture_readable(&bad));
    }

    #[test]
    fn mips_keep_colour_at_alpha_edges() {
        let mut top = vec![0u8; SIDE * SIDE * 4];
        for y in 0..SIDE {
            for x in 0..SIDE / 2 + 1 {
                let i = (y * SIDE + x) * 4;
                let a = if x < SIDE / 2 { 255 } else { 0 };
                top[i..i + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
        // Column 256 is transparent white; make the next block half opaque by hand.
        let levels = mips(top);
        let i = (10 * (SIDE / 2) + 127) * 4;
        assert_eq!(&levels[1][i..i + 4], &[255, 255, 255, 255]);
        let j = (10 * (SIDE / 2) + 128) * 4;
        assert_eq!(levels[1][j + 3], 0);
        assert_eq!(levels.len(), PICTURE_MIPS as usize);
        assert_eq!(levels[9].len(), 4);
    }

    #[test]
    fn mips_blend_half_covered_blocks_by_alpha() {
        let mut top = vec![0u8; SIDE * SIDE * 4];
        // One 2x2 block: the left column opaque white, the right transparent black.
        for y in 0..2 {
            let i = y * SIDE * 4;
            top[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let levels = mips(top);
        assert_eq!(&levels[1][..4], &[255, 255, 255, 128]);
    }

    #[test]
    fn picture_readable_reads_only_good_files() {
        let dir = temp("readable");
        let good = dir.join("good.png");
        red_png(&good, 8, 8);
        assert!(picture_readable(&good));
        assert!(!picture_readable(&dir.join("gone.png")));
    }

    #[test]
    fn relative_paths_join_the_folder() {
        let dir = temp("relative").join("ünï cödé folder");
        std::fs::create_dir_all(&dir).unwrap();
        red_png(&dir.join("my pic.png"), 16, 8);
        let (_, ok) = picture_levels(&picture_path("my pic.png", Some(&dir)));
        assert!(ok);
        let (_, ok) =
            picture_levels(&picture_path(dir.join("my pic.png").to_str().unwrap(), Some(Path::new("/elsewhere"))));
        assert!(ok, "absolute paths ignore the folder");
    }
}
