//! The bind groups the lighting keeps between frames.

/// Bind groups kept between frames (review S24): [`Lighting::run`] binds the same buffers and
/// pictures frame after frame, so each group is made once and found again by what it binds. Each
/// frame starts by dropping the groups the last frame didn't use ([`BindCache::sweep`]: a buffer
/// that grew is a new buffer, and the groups binding the old one would keep it alive); also emptied
/// when the pictures are made again and when it holds [`MOST_KEPT_BINDS`].
#[derive(Default)]
pub(super) struct BindCache {
    /// Each group and whether it was used since the last sweep.
    pub(super) groups: std::sync::Mutex<std::collections::HashMap<BindKey, (wgpu::BindGroup, bool)>>,
    /// How many groups it has made (for tests).
    pub(super) made: std::sync::atomic::AtomicUsize,
}

pub(super) const MOST_KEPT_BINDS: usize = 256;

/// What a bind group binds: its layout and each entry's binding and resource.
#[derive(PartialEq, Eq, Hash)]
pub(super) struct BindKey {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) entries: Vec<(u32, Bound)>,
}

#[derive(PartialEq, Eq, Hash)]
pub(super) enum Bound {
    Buffer(wgpu::Buffer, u64, Option<u64>),
    View(wgpu::TextureView),
    Sampler(wgpu::Sampler),
    /// Anything else (arrays): never kept.
    Other,
}

impl BindCache {
    /// The bind group `desc` describes, made the first time.
    pub(super) fn get(&self, device: &wgpu::Device, desc: &wgpu::BindGroupDescriptor) -> wgpu::BindGroup {
        let entries: Vec<(u32, Bound)> = desc
            .entries
            .iter()
            .map(|e| {
                let bound = match &e.resource {
                    wgpu::BindingResource::Buffer(b) => {
                        Bound::Buffer(b.buffer.clone(), b.offset, b.size.map(std::num::NonZeroU64::get))
                    }
                    wgpu::BindingResource::TextureView(v) => Bound::View((*v).clone()),
                    wgpu::BindingResource::Sampler(s) => Bound::Sampler((*s).clone()),
                    _ => Bound::Other,
                };
                (e.binding, bound)
            })
            .collect();
        if entries.iter().any(|(_, b)| *b == Bound::Other) {
            return device.create_bind_group(desc);
        }
        let key = BindKey { layout: desc.layout.clone(), entries };
        let mut groups = self.groups.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((g, used)) = groups.get_mut(&key) {
            *used = true;
            return g.clone();
        }
        if groups.len() >= MOST_KEPT_BINDS {
            groups.clear();
        }
        let g = device.create_bind_group(desc);
        self.made.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        groups.insert(key, (g.clone(), true));
        g
    }

    /// Drops the groups not used since the last sweep (and what only they kept alive).
    pub(super) fn sweep(&self) {
        let mut groups = self.groups.lock().unwrap_or_else(|e| e.into_inner());
        groups.retain(|_, (_, used)| std::mem::replace(used, false));
    }

    pub(super) fn clear(&self) {
        self.groups.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    pub(super) fn made(&self) -> usize {
        self.made.load(std::sync::atomic::Ordering::Relaxed)
    }
}
