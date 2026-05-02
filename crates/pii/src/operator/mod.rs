//! Operators that rewrite a single matched span and the opserator-config map.

mod hash;
mod mask;
mod ops;

pub use ops::{ChunkCount, HashAlgorithm, Operator, OperatorKind};
