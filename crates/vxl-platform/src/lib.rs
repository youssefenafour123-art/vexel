//! # vxl-platform
//!
//! The only place, apart from `vxl-telemetry`, that is allowed to call OS APIs directly.
//! v1 targets Windows. Other targets compile, but return `None` or do nothing.

pub mod memory;
