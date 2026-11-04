//! The bus effects: stereo blocks run once per bus. Each processes a block in place and gives
//! its wet sound only; the bus's chain mixes dry and wet. Every effect allocates in `new` and
//! never while it processes, smooths the settings it is given once a block, and takes no locks.

pub mod analyser;
pub mod compressor;
pub mod delay;
pub mod eq;
pub mod modfx;
pub mod reverb_hall;
pub mod reverb_plate;
pub mod reverb_spring;
pub mod width;
