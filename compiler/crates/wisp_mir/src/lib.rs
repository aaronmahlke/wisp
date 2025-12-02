//! Mid-level Intermediate Representation (MIR) for Wisp
//!
//! MIR is a lower-level representation that:
//! - Uses SSA (Static Single Assignment) form
//! - Has explicit basic blocks and control flow
//! - Makes all operations explicit (no implicit derefs, etc.)
//! - Is suitable for optimization and code generation

mod mir;
mod lower;

pub use mir::*;
pub use lower::lower_program;

