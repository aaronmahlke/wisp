//! Borrow Checker for Wisp
//!
//! Implements ownership tracking, move analysis, and borrow checking.
//! Uses a simplified NLL (Non-Lexical Lifetimes) approach.

mod state;
mod check;

pub use check::{BorrowChecker, BorrowError};
pub use state::*;

