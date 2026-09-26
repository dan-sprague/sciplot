//! Backends that consume a `DrawList`.

pub(crate) mod gpu;
#[cfg(feature = "cpu-png")]
pub(crate) mod cpu;
pub(crate) mod svg;
