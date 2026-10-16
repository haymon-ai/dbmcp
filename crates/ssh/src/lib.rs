//! Embedded SSH client that tunnels database connections through a bastion.
//!
//! [`Tunnel::start`] binds a loopback listener and returns a
//! [`TunnelHandle`]. Each accepted socket is forwarded to the database over
//! its own `direct-tcpip` channel on a single, lazily opened SSH session.
//! Database drivers connect to [`TunnelHandle::local_addr`] and stay
//! unaware of SSH.

mod error;
mod key;
mod known_hosts;
mod tunnel;

pub use error::SshError;
pub use tunnel::{Tunnel, TunnelHandle};
