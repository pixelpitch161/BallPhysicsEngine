//! Trapped air (part 2c): what the card finds under the water each frame and fills with air drops
//! (`shaders/air.wgsl`). The cells' and the blocks' buffers live here; the passes are encoded into
//! the frame's last step by [`super::Water`].

use super::BLOCK;
use super::buffers::storage;
use crate::solid::Layout;

/// A body's buffers for trapped air.
pub(super) struct Air {
    /// Per cell: its spots, the running sum within its block, the liquid's speed round it.
    pub(super) cells: wgpu::Buffer,
    /// Per block of cells, its running sum; then every spot found. Cleared each frame.
    pub(super) blocks: wgpu::Buffer,
}

impl Air {
    pub(super) fn new(device: &wgpu::Device, layout: &Layout) -> Air {
        let nodes = layout.nodes() as u64;
        Air {
            cells: storage(device, "water air cells", nodes * 16),
            blocks: storage(device, "water air blocks", (nodes.div_ceil(u64::from(BLOCK)) + 1) * 4),
        }
    }
}

/// The slots a frame keeps for air drops after its drops, when the report two frames back found
/// `found` spots of trapped air and `deep` cells deep: as many and a quarter more, and room for the
/// pockets that close since (a quarter of the deep cells, from 64 to 2048), none when it found none,
/// at most `most` and `room` (the drops the body still has room for). The slots no air takes are
/// let go two frames on.
pub(super) fn slots_for(found: u32, deep: u32, room: u32, most: u32) -> u32 {
    if found == 0 {
        return 0;
    }
    let new = (deep / 4).clamp(64, 2048);
    found.saturating_add(found / 4).saturating_add(new).min(most).min(room)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_air_found() {
        assert_eq!(slots_for(0, 5000, 1000, 65_536), 0, "no air, no slots");
        assert_eq!(slots_for(100, 0, 1 << 20, 65_536), 189);
        assert_eq!(slots_for(100, 4000, 1 << 20, 65_536), 1125, "room for new pockets in deep water");
        assert_eq!(slots_for(100, 1 << 20, 1 << 20, 65_536), 2173, "at most 2048 of it");
        assert_eq!(slots_for(1 << 20, 0, 1 << 22, 65_536), 65_536, "at most the most a frame");
        assert_eq!(slots_for(100, 0, 50, 65_536), 50, "at most the room left");
        assert_eq!(slots_for(u32::MAX, u32::MAX, u32::MAX, u32::MAX), u32::MAX);
    }
}
