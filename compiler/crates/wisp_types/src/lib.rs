//! Type checking for Wisp
//!
//! This module performs type inference and checking on HIR.

mod types;
mod check;

pub use types::*;
pub use check::{
    TypeChecker, TypeError,
    TypedProgram, TypedImpl, TypedFunction, TypedExternFunction, TypedExternStatic, TypedParam, TypedBlock, TypedStmt,
    TypedExpr, TypedExprKind, TypedElse, TypedMatchArm, TypedPattern, TypedLambdaParam,
    GenericInstantiation,
};

