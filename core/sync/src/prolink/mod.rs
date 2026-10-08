//! Pro DJ Link (CDJ-3000 / XDJ / DJM): packet parsing, the virtual-device
//! announcement that lets player5 join as a device, and the network source.
//!
//! PLACEHOLDER: implemented in its own change, with every protocol fact
//! sourced in `docs/protocols/pro-dj-link.md`. Expected public surface:
//! packet types and `parse_*` functions (pure, fixture-tested), packet
//! builders for tests and the simulator, and
//! `start(config) -> io::Result<crate::net::SourceHandle>`.
