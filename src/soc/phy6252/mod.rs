//! PHY6252 SoC model.
//!
//! The existing implementation files are physically grouped in this directory.
//! During the migration they are still mounted at the crate root with `#[path]`
//! attributes so the move itself does not create a giant import-only diff.
//! New PHY6252 contracts should live under this namespace directly.

pub mod pins;
