//! Brush strokes on the GPU: each frame's brush tiles (see [`Frame::mask_tiles`]) in one 2D
//! array texture, tile `i` in layer `i`, bound with the layer records (group 2, bindings 1 and
//! 2). A tile is re-sent only when the tile at its place changed.

use std::sync::Arc;

/// A brush tile's width and height in pixels; it covers a ball's square, -1 to 1 both ways.
pub const MASK_TILE: usize = 128;

/// The most brush tiles one frame can hold; records naming later ones draw no strokes.
pub const MAX_MASK_TILES: usize = 256;

/// One frame's brush tiles on the GPU.
pub(crate) struct MaskAtlas {
    texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    /// The tile hashes in the texture's layers now.
    uploaded: Vec<u64>,
}

impl MaskAtlas {
    pub(crate) fn new(device: &wgpu::Device, layers: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("brush tiles"),
            size: wgpu::Extent3d {
                width: MASK_TILE as u32,
                height: MASK_TILE as u32,
                depth_or_array_layers: layers.max(1),
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { texture, view, uploaded: Vec::new() }
    }

    fn layers(&self) -> usize {
        self.texture.depth_or_array_layers() as usize
    }

    /// Puts `tiles` in the texture, growing it in powers of two; true when the texture was
    /// remade, so what binds it must be remade too.
    pub(crate) fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tiles: &[(u64, Arc<[u8]>)]) -> bool {
        let tiles = &tiles[..tiles.len().min(MAX_MASK_TILES)];
        let mut remade = false;
        if tiles.len() > self.layers() {
            *self = Self::new(device, tiles.len().next_power_of_two() as u32);
            remade = true;
        }
        for (i, (hash, bytes)) in tiles.iter().enumerate() {
            if self.uploaded.get(i) == Some(hash) || bytes.len() != MASK_TILE * MASK_TILE {
                continue;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: i as u32 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(MASK_TILE as u32),
                    rows_per_image: Some(MASK_TILE as u32),
                },
                wgpu::Extent3d { width: MASK_TILE as u32, height: MASK_TILE as u32, depth_or_array_layers: 1 },
            );
            if self.uploaded.len() <= i {
                self.uploaded.resize(i + 1, u64::MAX);
            }
            self.uploaded[i] = *hash;
        }
        remade
    }
}
