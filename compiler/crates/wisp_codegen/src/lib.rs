//! Code generation for Wisp using Cranelift
//!
//! This module compiles MIR to native machine code.

mod codegen;

pub use codegen::{Codegen, CodegenError};

