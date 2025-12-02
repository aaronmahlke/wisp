//! Borrow checking pass

use crate::state::{BorrowConflict, BorrowState, Place};
use wisp_lexer::Span;
use wisp_types::{Type, TypedProgram, TypedFunction, TypedExpr, TypedExprKind, TypedStmt, TypedBlock, TypedElse};

/// A borrow error
#[derive(Debug, Clone)]
pub struct BorrowError {
    pub message: String,
    pub span: Span,
    pub notes: Vec<(String, Span)>,
}

impl BorrowError {
    pub fn new(message: String, span: Span) -> Self {
        Self { message, span, notes: Vec::new() }
    }

    pub fn with_note(mut self, message: String, span: Span) -> Self {
        self.notes.push((message, span));
        self
    }
}

/// The borrow checker
pub struct BorrowChecker<'a> {
    program: &'a TypedProgram,
    state: BorrowState,
    errors: Vec<BorrowError>,
    /// Current function being checked
    current_fn: Option<String>,
}

impl<'a> BorrowChecker<'a> {
    pub fn new(program: &'a TypedProgram) -> Self {
        Self {
            program,
            state: BorrowState::new(),
            errors: Vec::new(),
            current_fn: None,
        }
    }

    pub fn check(mut self) -> Result<(), Vec<BorrowError>> {
        // Check all functions
        for func in &self.program.functions {
            self.check_function(func);
        }
        
        // Check impl methods
        for imp in &self.program.impls {
            for method in &imp.methods {
                self.check_function(method);
            }
        }

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    fn check_function(&mut self, func: &TypedFunction) {
        self.current_fn = Some(func.name.clone());
        // Reset state for each function
        self.state = BorrowState::new();

        // Declare parameters
        for param in &func.params {
            self.state.declare_var(param.def_id, param.name.clone(), param.is_mut, true);
        }

        // Check body
        if let Some(body) = &func.body {
            self.check_block(body);
        }

        self.current_fn = None;
    }

    fn check_block(&mut self, block: &TypedBlock) {
        // Save state for block scope
        let saved_loans = self.state.active_loans.clone();

        for stmt in &block.stmts {
            self.check_stmt(stmt);
        }

        // End loans that were created in this block
        // In a real NLL implementation, we'd track loan lifetimes more precisely
        self.state.active_loans = saved_loans;
    }

    fn check_stmt(&mut self, stmt: &TypedStmt) {
        match stmt {
            TypedStmt::Let { def_id, name, is_mut, init, .. } => {
                // First check the initializer
                if let Some(init_expr) = init {
                    self.check_expr(init_expr);
                    // Check if initializer moves a value
                    self.check_move_or_copy(init_expr);
                }

                // Then declare the variable
                self.state.declare_var(*def_id, name.clone(), *is_mut, init.is_some());
            }
            TypedStmt::Expr(expr) => {
                self.check_expr(expr);
            }
        }
    }

    fn check_expr(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::IntLiteral(_) |
            TypedExprKind::FloatLiteral(_) |
            TypedExprKind::BoolLiteral(_) |
            TypedExprKind::StringLiteral(_) => {}

            TypedExprKind::Var { def_id, .. } => {
                // Check if variable is valid (not moved)
                let place = Place::var(*def_id);
                if let Err(conflict) = self.state.can_read(&place) {
                    self.report_conflict(conflict, expr.span);
                }
            }

            TypedExprKind::Binary { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }

            TypedExprKind::Unary { expr: inner, .. } => {
                self.check_expr(inner);
            }

            TypedExprKind::Call { callee, args } => {
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                    // Arguments may be moved
                    self.check_move_or_copy(arg);
                }
            }
            
            TypedExprKind::MethodCall { receiver, args, .. } => {
                // Check receiver - it's borrowed, not moved
                self.check_expr(receiver);
                // Create a borrow for the receiver (immutable for &self)
                if let Some(place) = self.expr_to_place(receiver) {
                    if let Err(conflict) = self.state.can_borrow(&place) {
                        self.report_conflict(conflict, expr.span);
                    } else {
                        self.state.create_loan(place, false, expr.span);
                    }
                }
                
                for arg in args {
                    self.check_expr(arg);
                    // Arguments may be moved
                    self.check_move_or_copy(arg);
                }
            }

            TypedExprKind::Field { expr: base, .. } => {
                self.check_expr(base);
            }

            TypedExprKind::Index { expr: base, index } => {
                self.check_expr(base);
                self.check_expr(index);
            }

            TypedExprKind::ArrayLit(elements) => {
                for elem in elements {
                    self.check_expr(elem);
                    self.check_move_or_copy(elem);
                }
            }

            TypedExprKind::Lambda { body, .. } => {
                // For now, just check the lambda body
                // TODO: proper capture analysis
                self.check_expr(body);
            }

            TypedExprKind::StructLit { fields, .. } => {
                for (_, field_expr) in fields {
                    self.check_expr(field_expr);
                    self.check_move_or_copy(field_expr);
                }
            }

            TypedExprKind::If { cond, then_block, else_block } => {
                self.check_expr(cond);
                self.check_block(then_block);
                if let Some(else_b) = else_block {
                    self.check_else(else_b);
                }
            }

            TypedExprKind::While { cond, body } => {
                self.check_expr(cond);
                self.check_block(body);
            }

            TypedExprKind::For { start, end, body, .. } => {
                self.check_expr(start);
                self.check_expr(end);
                self.check_block(body);
            }

            TypedExprKind::Match { scrutinee, arms } => {
                self.check_expr(scrutinee);
                for arm in arms {
                    self.check_expr(&arm.body);
                }
            }

            TypedExprKind::Block(block) => {
                self.check_block(block);
            }

            TypedExprKind::Assign { target, value } => {
                // Check the value first
                self.check_expr(value);

                // Then check we can write to target
                if let Some(place) = self.expr_to_place(target) {
                    // Check mutability
                    if !self.is_place_mutable(&place) {
                        self.errors.push(BorrowError::new(
                            format!("cannot assign to `{}`, as it is not declared as mutable",
                                place.display(&self.state.var_names)),
                            expr.span,
                        ));
                    }

                    // Check no borrows
                    if let Err(conflict) = self.state.can_write(&place) {
                        self.report_conflict(conflict, expr.span);
                    }
                }

                // Value may be moved
                self.check_move_or_copy(value);
            }

            TypedExprKind::Ref { expr: inner, is_mut } => {
                // First check the inner expression (but don't check move/copy since we're borrowing)
                // Note: we don't call check_expr here to avoid false positives
                // The borrow itself is what we need to validate
                
                if let Some(place) = self.expr_to_place(inner) {
                    if *is_mut {
                        // Mutable borrow - check we can take a mutable borrow
                        if let Err(conflict) = self.state.can_borrow_mut(&place) {
                            self.report_conflict(conflict, expr.span);
                        } else {
                            self.state.create_loan(place, true, expr.span);
                        }
                    } else {
                        // Immutable borrow
                        if let Err(conflict) = self.state.can_borrow(&place) {
                            self.report_conflict(conflict, expr.span);
                        } else {
                            self.state.create_loan(place, false, expr.span);
                        }
                    }
                }
            }

            TypedExprKind::Deref(inner) => {
                self.check_expr(inner);
            }
            
            TypedExprKind::GenericCall { args, .. } => {
                // Check arguments (similar to regular call)
                for arg in args {
                    self.check_expr(arg);
                    self.check_move_or_copy(arg);
                }
            }
            
            TypedExprKind::TraitMethodCall { receiver, args, .. } => {
                self.check_expr(receiver);
                for arg in args {
                    self.check_expr(arg);
                    self.check_move_or_copy(arg);
                }
            }

            TypedExprKind::AssociatedFunctionCall { args, .. } => {
                // Check all arguments
                for arg in args {
                    self.check_expr(arg);
                    self.check_move_or_copy(arg);
                }
            }

            TypedExprKind::PrimitiveMethodCall { receiver, args, .. } => {
                self.check_expr(receiver);
                for arg in args {
                    self.check_expr(arg);
                    self.check_move_or_copy(arg);
                }
            }

            TypedExprKind::Error => {}
        }
    }
    
    fn check_else(&mut self, else_branch: &TypedElse) {
        match else_branch {
            TypedElse::Block(block) => self.check_block(block),
            TypedElse::If(if_expr) => self.check_expr(if_expr),
        }
    }

    /// Check if an expression results in a move (for non-Copy types)
    fn check_move_or_copy(&mut self, expr: &TypedExpr) {
        // For now, we'll consider primitive types as Copy
        // and everything else as requiring a move
        if self.is_copy_type(expr) {
            return;
        }

        if let Some(place) = self.expr_to_place(expr) {
            // Check if already moved (error already reported in check_expr)
            if self.state.can_read(&place).is_err() {
                return;
            }
            // Mark as moved
            self.state.move_place(&place, expr.span);
        }
    }

    /// Check if an expression has a Copy type
    fn is_copy_type(&self, expr: &TypedExpr) -> bool {
        match &expr.ty {
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64 | Type::Bool | Type::Char => true,
            Type::Ref { .. } => true, // References are Copy
            _ => false,
        }
    }

    /// Convert an expression to a Place (if it represents a place in memory)
    fn expr_to_place(&self, expr: &TypedExpr) -> Option<Place> {
        match &expr.kind {
            TypedExprKind::Var { def_id, .. } => {
                Some(Place::var(*def_id))
            }
            TypedExprKind::Field { expr: base, field, .. } => {
                self.expr_to_place(base).map(|p| p.field(field.clone()))
            }
            TypedExprKind::Deref(inner) => {
                self.expr_to_place(inner).map(|p| p.deref())
            }
            TypedExprKind::Index { expr: base, .. } => {
                self.expr_to_place(base).map(|p| p.index())
            }
            _ => None,
        }
    }

    /// Check if a place is mutable
    fn is_place_mutable(&self, place: &Place) -> bool {
        // Check if the root variable is mutable
        let root_mutable = self.state.is_mutable(place.base);
        
        // Check projections for derefs through mutable references
        for proj in &place.projections {
            if let crate::state::Projection::Deref = proj {
                // Dereferencing a mutable reference allows mutation
                // We'd need type info to check this properly, but for now
                // assume dereferences through mutable refs are mutable
                return true;
            }
        }
        
        root_mutable
    }

    fn report_conflict(&mut self, conflict: BorrowConflict, span: Span) {
        let message = conflict.display(&self.state.var_names);
        let mut error = BorrowError::new(message, span);

        match &conflict {
            BorrowConflict::UseAfterMove { moved_at, .. } => {
                error = error.with_note("value moved here".to_string(), *moved_at);
            }
            BorrowConflict::UseWhileMutablyBorrowed { loan, .. } |
            BorrowConflict::WriteWhileBorrowed { loan, .. } |
            BorrowConflict::BorrowWhileMutablyBorrowed { loan, .. } |
            BorrowConflict::MutBorrowWhileBorrowed { loan, .. } => {
                let borrow_kind = if loan.is_mut { "mutable" } else { "immutable" };
                error = error.with_note(format!("{} borrow occurs here", borrow_kind), loan.span);
            }
            BorrowConflict::BorrowMutOfImmutable { .. } => {}
        }

        self.errors.push(error);
    }
}


