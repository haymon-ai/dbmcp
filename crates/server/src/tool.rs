//! Declarative tool registry for per-backend MCP routers.
//!
//! A backend declares its tools as a `const` slice of [`ToolSpec`] rows, each
//! row carrying the gating flags for that tool. [`ToolRouterExt::from_specs`]
//! folds the slice into a [`ToolRouter`], skipping tools the current mode forbids.
//! This keeps the read-only / pinned gating matrix as data, not control flow.

use rmcp::handler::server::router::tool::{AsyncTool, ToolRouter};

/// Declarative registration entry for one MCP tool.
///
/// Pairs the tool's router-registration function with its mode gates.
pub struct ToolSpec<H: Send + Sync + 'static> {
    /// Registers the tool on a router, returning the extended router.
    register: fn(ToolRouter<H>) -> ToolRouter<H>,
    /// Whether the tool is hidden in read-only mode.
    read_only: bool,
    /// Whether the tool is hidden when a database name is pinned.
    pinned: bool,
}

impl<H: Send + Sync + 'static> std::fmt::Debug for ToolSpec<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSpec")
            .field("read_only", &self.read_only)
            .field("pinned", &self.pinned)
            .finish_non_exhaustive()
    }
}

impl<H: Send + Sync + 'static> ToolSpec<H> {
    /// Creates a spec for an async tool `T` with its read-only and pinned gates.
    #[must_use]
    pub const fn async_tool<T: AsyncTool<H> + 'static>(read_only: bool, pinned: bool) -> Self {
        Self {
            register: ToolRouter::with_async_tool::<T>,
            read_only,
            pinned,
        }
    }
}

/// Extends [`ToolRouter`] with declarative construction from a [`ToolSpec`] table.
pub trait ToolRouterExt<H: Send + Sync + 'static>: Sized {
    /// Builds a router from `specs`, skipping mode-gated tools.
    ///
    /// A spec is skipped when its `read_only` gate coincides with `read_only`
    /// mode, or its `pinned` gate with `pinned` mode.
    #[must_use]
    fn from_specs(specs: &[ToolSpec<H>], read_only: bool, pinned: bool) -> Self;
}

impl<H: Send + Sync + 'static> ToolRouterExt<H> for ToolRouter<H> {
    fn from_specs(specs: &[ToolSpec<H>], read_only: bool, pinned: bool) -> Self {
        specs
            .iter()
            .filter(|spec| (!spec.read_only || !read_only) && (!spec.pinned || !pinned))
            .fold(ToolRouter::new(), |router, spec| (spec.register)(router))
    }
}
