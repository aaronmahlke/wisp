//! Borrow state tracking

use wisp_hir::DefId;
use wisp_lexer::Span;
use std::collections::{HashMap, HashSet};

/// Unique identifier for a borrow/loan
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoanId(pub u32);

/// A place in memory (variable or path through fields/derefs)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Place {
    /// The root variable
    pub base: DefId,
    /// Path through the place (field names, derefs)
    pub projections: Vec<Projection>,
}

impl Place {
    pub fn var(def_id: DefId) -> Self {
        Self {
            base: def_id,
            projections: Vec::new(),
        }
    }

    pub fn field(mut self, name: String) -> Self {
        self.projections.push(Projection::Field(name));
        self
    }

    pub fn deref(mut self) -> Self {
        self.projections.push(Projection::Deref);
        self
    }

    pub fn index(mut self) -> Self {
        self.projections.push(Projection::Index);
        self
    }

    /// Check if this place is a prefix of another
    pub fn is_prefix_of(&self, other: &Place) -> bool {
        if self.base != other.base {
            return false;
        }
        if self.projections.len() > other.projections.len() {
            return false;
        }
        self.projections.iter().zip(&other.projections).all(|(a, b)| a == b)
    }

    /// Check if two places conflict (overlap)
    pub fn conflicts_with(&self, other: &Place) -> bool {
        self.is_prefix_of(other) || other.is_prefix_of(self)
    }

    pub fn display(&self, names: &HashMap<DefId, String>) -> String {
        let base = names.get(&self.base).cloned().unwrap_or_else(|| format!("_{}", self.base.0));
        let mut result = base;
        for proj in &self.projections {
            match proj {
                Projection::Field(name) => {
                    result.push('.');
                    result.push_str(name);
                }
                Projection::Deref => {
                    result = format!("(*{})", result);
                }
                Projection::Index => {
                    result.push_str("[_]");
                }
            }
        }
        result
    }
}

/// A projection in a place
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Projection {
    Field(String),
    Deref,
    Index,
}

/// A loan (borrow) of a place
#[derive(Debug, Clone)]
pub struct Loan {
    pub id: LoanId,
    /// The place being borrowed
    pub place: Place,
    /// Is this a mutable borrow?
    pub is_mut: bool,
    /// Where the borrow was created
    pub span: Span,
}

/// State of a variable
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarState {
    /// Variable is valid and owned
    Valid,
    /// Variable has been moved
    Moved { to: Span },
    /// Variable is partially moved (some fields moved)
    PartiallyMoved { fields: HashSet<String> },
    /// Variable is uninitialized
    Uninitialized,
}

/// Borrow state at a program point
#[derive(Debug, Clone)]
pub struct BorrowState {
    /// Variable states
    pub var_states: HashMap<DefId, VarState>,
    /// Active loans
    pub active_loans: HashMap<LoanId, Loan>,
    /// Next loan ID
    next_loan_id: u32,
    /// Variable names for error messages
    pub var_names: HashMap<DefId, String>,
    /// Is variable mutable?
    pub var_mutability: HashMap<DefId, bool>,
}

impl BorrowState {
    pub fn new() -> Self {
        Self {
            var_states: HashMap::new(),
            active_loans: HashMap::new(),
            next_loan_id: 0,
            var_names: HashMap::new(),
            var_mutability: HashMap::new(),
        }
    }

    /// Declare a new variable
    pub fn declare_var(&mut self, def_id: DefId, name: String, is_mut: bool, initialized: bool) {
        self.var_names.insert(def_id, name);
        self.var_mutability.insert(def_id, is_mut);
        if initialized {
            self.var_states.insert(def_id, VarState::Valid);
        } else {
            self.var_states.insert(def_id, VarState::Uninitialized);
        }
    }

    /// Initialize a variable
    pub fn initialize(&mut self, def_id: DefId) {
        self.var_states.insert(def_id, VarState::Valid);
    }

    /// Check if a variable is initialized
    pub fn is_initialized(&self, def_id: DefId) -> bool {
        matches!(self.var_states.get(&def_id), Some(VarState::Valid))
    }

    /// Check if a variable is mutable
    pub fn is_mutable(&self, def_id: DefId) -> bool {
        self.var_mutability.get(&def_id).copied().unwrap_or(false)
    }

    /// Get variable state
    pub fn get_state(&self, def_id: DefId) -> Option<&VarState> {
        self.var_states.get(&def_id)
    }

    /// Move a place (transfer ownership)
    pub fn move_place(&mut self, place: &Place, span: Span) {
        self.var_states.insert(place.base, VarState::Moved { to: span });
    }

    /// Create a new loan
    pub fn create_loan(&mut self, place: Place, is_mut: bool, span: Span) -> LoanId {
        let id = LoanId(self.next_loan_id);
        self.next_loan_id += 1;
        let loan = Loan { id, place, is_mut, span };
        self.active_loans.insert(id, loan);
        id
    }

    /// End a loan
    pub fn end_loan(&mut self, id: LoanId) {
        self.active_loans.remove(&id);
    }

    /// End all loans of a place
    pub fn end_loans_of(&mut self, place: &Place) {
        let to_remove: Vec<_> = self.active_loans.iter()
            .filter(|(_, loan)| loan.place.conflicts_with(place))
            .map(|(id, _)| *id)
            .collect();
        for id in to_remove {
            self.active_loans.remove(&id);
        }
    }

    /// Check for conflicting borrows
    pub fn check_borrow_conflicts(&self, place: &Place, is_mut: bool) -> Vec<&Loan> {
        self.active_loans.values()
            .filter(|loan| {
                if !loan.place.conflicts_with(place) {
                    return false;
                }
                // Conflict if either borrow is mutable
                is_mut || loan.is_mut
            })
            .collect()
    }

    /// Check if a place can be read (not moved, not mutably borrowed)
    pub fn can_read(&self, place: &Place) -> Result<(), BorrowConflict> {
        // Check if moved
        if let Some(VarState::Moved { to }) = self.var_states.get(&place.base) {
            return Err(BorrowConflict::UseAfterMove {
                place: place.clone(),
                moved_at: *to,
            });
        }

        // Check for mutable borrows
        for loan in self.active_loans.values() {
            if loan.is_mut && loan.place.conflicts_with(place) {
                return Err(BorrowConflict::UseWhileMutablyBorrowed {
                    place: place.clone(),
                    loan: loan.clone(),
                });
            }
        }

        Ok(())
    }

    /// Check if a place can be written (not borrowed at all, is mutable)
    pub fn can_write(&self, place: &Place) -> Result<(), BorrowConflict> {
        // Check if moved
        if let Some(VarState::Moved { to }) = self.var_states.get(&place.base) {
            return Err(BorrowConflict::UseAfterMove {
                place: place.clone(),
                moved_at: *to,
            });
        }

        // Check for any borrows
        for loan in self.active_loans.values() {
            if loan.place.conflicts_with(place) {
                return Err(BorrowConflict::WriteWhileBorrowed {
                    place: place.clone(),
                    loan: loan.clone(),
                });
            }
        }

        Ok(())
    }

    /// Check if a place can be mutably borrowed
    pub fn can_borrow_mut(&self, place: &Place) -> Result<(), BorrowConflict> {
        // First check if we can write
        self.can_write(place)?;

        // Check mutability
        if !self.is_mutable(place.base) && place.projections.is_empty() {
            return Err(BorrowConflict::BorrowMutOfImmutable {
                place: place.clone(),
            });
        }

        Ok(())
    }

    /// Check if a place can be immutably borrowed
    pub fn can_borrow(&self, place: &Place) -> Result<(), BorrowConflict> {
        // Check if moved
        if let Some(VarState::Moved { to }) = self.var_states.get(&place.base) {
            return Err(BorrowConflict::UseAfterMove {
                place: place.clone(),
                moved_at: *to,
            });
        }

        // Check for mutable borrows
        for loan in self.active_loans.values() {
            if loan.is_mut && loan.place.conflicts_with(place) {
                return Err(BorrowConflict::BorrowWhileMutablyBorrowed {
                    place: place.clone(),
                    loan: loan.clone(),
                });
            }
        }

        Ok(())
    }
}

impl Default for BorrowState {
    fn default() -> Self {
        Self::new()
    }
}

/// A borrow conflict
#[derive(Debug, Clone)]
pub enum BorrowConflict {
    UseAfterMove {
        place: Place,
        moved_at: Span,
    },
    UseWhileMutablyBorrowed {
        place: Place,
        loan: Loan,
    },
    WriteWhileBorrowed {
        place: Place,
        loan: Loan,
    },
    BorrowWhileMutablyBorrowed {
        place: Place,
        loan: Loan,
    },
    BorrowMutOfImmutable {
        place: Place,
    },
    MutBorrowWhileBorrowed {
        place: Place,
        loan: Loan,
    },
}

impl BorrowConflict {
    pub fn display(&self, names: &HashMap<DefId, String>) -> String {
        match self {
            BorrowConflict::UseAfterMove { place, .. } => {
                format!("use of moved value: `{}`", place.display(names))
            }
            BorrowConflict::UseWhileMutablyBorrowed { place, .. } => {
                format!("cannot use `{}` while mutably borrowed", place.display(names))
            }
            BorrowConflict::WriteWhileBorrowed { place, .. } => {
                format!("cannot assign to `{}` while borrowed", place.display(names))
            }
            BorrowConflict::BorrowWhileMutablyBorrowed { place, .. } => {
                format!("cannot borrow `{}` while mutably borrowed", place.display(names))
            }
            BorrowConflict::BorrowMutOfImmutable { place } => {
                format!("cannot borrow `{}` as mutable, as it is not declared as mutable", place.display(names))
            }
            BorrowConflict::MutBorrowWhileBorrowed { place, .. } => {
                format!("cannot borrow `{}` as mutable while also borrowed as immutable", place.display(names))
            }
        }
    }
}

