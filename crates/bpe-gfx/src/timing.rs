//! How long the GPU spends lighting a frame, for the editor's status line: two timestamps round
//! the light passes, read back a frame or so later without ever waiting. Only on a device made
//! with [`LightTimer::FEATURES`]; without them nothing is timed.

use std::sync::{Arc, Mutex};

/// What a device needs for the timer.
pub const FEATURES: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

pub struct LightTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Nanoseconds per tick.
    period: f32,
    /// The read buffer is being mapped or holds a result not yet taken.
    busy: Arc<Mutex<Busy>>,
    /// The last time measured, in milliseconds.
    pub last_ms: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Busy {
    Free,
    Mapping,
    Mapped,
}

impl LightTimer {
    /// A timer, when `device` can time.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<LightTimer> {
        if !device.features().contains(FEATURES) {
            return None;
        }
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("light timer"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buffer = |usage, label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage,
                mapped_at_creation: false,
            })
        };
        let resolve = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, "light timer resolve");
        let read = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, "light timer read");
        Some(LightTimer {
            set,
            resolve,
            read,
            period: queue.get_timestamp_period(),
            busy: Arc::new(Mutex::new(Busy::Free)),
            last_ms: None,
        })
    }

    /// Notes the start of the light passes.
    pub fn start(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.write_timestamp(&self.set, 0);
    }

    /// Notes their end and, when the last reading has been taken, copies this one out.
    pub fn end(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        encoder.write_timestamp(&self.set, 1);
        self.take();
        let free = *self.busy.lock().expect("timer lock") == Busy::Free;
        if free {
            encoder.resolve_query_set(&self.set, 0..2, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, 16);
        }
        free
    }

    /// After the frame is submitted: starts reading the copy made by [`LightTimer::end`].
    pub fn read_after_submit(&self) {
        *self.busy.lock().expect("timer lock") = Busy::Mapping;
        let busy = self.busy.clone();
        self.read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            *busy.lock().expect("timer lock") = if r.is_ok() { Busy::Mapped } else { Busy::Free };
        });
    }

    /// Takes a finished reading.
    fn take(&mut self) {
        if *self.busy.lock().expect("timer lock") != Busy::Mapped {
            return;
        }
        if let Ok(data) = self.read.slice(..).get_mapped_range() {
            let tick = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"));
            let span = tick(8).wrapping_sub(tick(0));
            // A span of more than a second is a reset counter, not a frame.
            let ms = span as f64 * self.period as f64 / 1e6;
            if ms > 0.0 && ms < 1000.0 {
                self.last_ms = Some(ms as f32);
            }
        }
        self.read.unmap();
        *self.busy.lock().expect("timer lock") = Busy::Free;
    }
}
