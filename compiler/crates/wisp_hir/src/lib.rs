//! HIR (High-level Intermediate Representation) and Name Resolution
//!
//! This module transforms AST into HIR by:
//! 1. Resolving all identifiers to unique DefIds
//! 2. Building scope tables
//! 3. Checking for undefined/duplicate names

mod resolve;
mod hir;

pub use resolve::{Resolver, ResolveError};
pub use hir::*;

