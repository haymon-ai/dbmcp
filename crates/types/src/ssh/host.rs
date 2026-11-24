//! Host-key verification policy for the SSH tunnel.

/// Host-key verification policy for the SSH tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrictHostKeyChecking {
    /// Reject hosts missing from `known_hosts`.
    Yes,
    /// Trust unknown hosts on first use and record them.
    AcceptNew,
    /// Disable host-key verification (insecure).
    No,
}

impl StrictHostKeyChecking {
    /// Returns the setting value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::AcceptNew => "accept-new",
            Self::No => "no",
        }
    }
}

impl std::fmt::Display for StrictHostKeyChecking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
