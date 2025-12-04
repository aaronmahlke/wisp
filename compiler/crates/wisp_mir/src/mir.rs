//! MIR data structures

use wisp_hir::DefId;
use wisp_types::Type;
use std::collections::HashMap;

/// A MIR program
#[derive(Debug)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
    pub extern_functions: Vec<MirExternFunction>,
    pub extern_statics: Vec<MirExternStatic>,
    pub structs: HashMap<DefId, MirStruct>,
    pub enums: HashMap<DefId, MirEnum>,
}

/// A MIR extern function declaration
#[derive(Debug, Clone)]
pub struct MirExternFunction {
    pub def_id: DefId,
    pub name: String,
    pub params: Vec<Type>,
    pub return_type: Type,
}

/// A MIR extern static variable declaration
#[derive(Debug, Clone)]
pub struct MirExternStatic {
    pub def_id: DefId,
    pub name: String,
    pub ty: Type,
}

impl MirProgram {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            extern_functions: Vec::new(),
            extern_statics: Vec::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
        }
    }

    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        out.push_str("=== MIR Program ===\n\n");
        
        if !self.extern_functions.is_empty() {
            out.push_str("--- Extern Functions ---\n");
            for ext in &self.extern_functions {
                let params: Vec<_> = ext.params.iter()
                    .map(|t| format!("{:?}", t))
                    .collect();
                out.push_str(&format!("  extern fn {}({}) -> {:?}\n", 
                    ext.name, params.join(", "), ext.return_type));
            }
            out.push('\n');
        }

        if !self.extern_statics.is_empty() {
            out.push_str("--- Extern Statics ---\n");
            for ext in &self.extern_statics {
                out.push_str(&format!("  extern static {}: {:?}\n", ext.name, ext.ty));
            }
            out.push('\n');
        }

        for func in &self.functions {
            out.push_str(&func.pretty_print());
            out.push('\n');
        }

        out
    }
}

impl Default for MirProgram {
    fn default() -> Self {
        Self::new()
    }
}

/// A MIR struct definition
#[derive(Debug, Clone)]
pub struct MirStruct {
    pub def_id: DefId,
    pub name: String,
    pub fields: Vec<(String, Type)>,
}

/// A MIR enum definition
#[derive(Debug, Clone)]
pub struct MirEnum {
    pub def_id: DefId,
    pub name: String,
    /// Variants: (name, variant_def_id, field_types)
    pub variants: Vec<(String, DefId, Vec<Type>)>,
}

impl MirEnum {
    /// Get the size of the discriminant (always i64 for simplicity)
    pub fn discriminant_size(&self) -> u32 {
        8 // Use i64 for discriminant
    }
    
    /// Get the size of the largest variant's payload
    pub fn max_payload_size(&self) -> u32 {
        self.variants.iter()
            .map(|(_, _, fields)| {
                fields.iter().map(|ty| type_size(ty)).sum::<u32>()
            })
            .max()
            .unwrap_or(0)
    }
    
    /// Total enum size: discriminant + max payload (aligned)
    pub fn total_size(&self) -> u32 {
        let disc = self.discriminant_size();
        let payload = self.max_payload_size();
        // Align payload to 8 bytes
        disc + ((payload + 7) / 8) * 8
    }
    
    /// Get payload offset (after discriminant)
    pub fn payload_offset(&self) -> u32 {
        self.discriminant_size()
    }
}

/// Helper to get type size (basic approximation)
fn type_size(ty: &Type) -> u32 {
    match ty {
        Type::I8 | Type::U8 | Type::Bool => 1,
        Type::I16 | Type::U16 => 2,
        Type::I32 | Type::U32 | Type::Char => 4,
        Type::I64 | Type::U64 | Type::Str => 8,
        Type::F32 => 4,
        Type::F64 => 8,
        Type::Ref { .. } => 8, // Pointers are 8 bytes
        Type::Struct { .. } | Type::Enum { .. } => 8, // Passed as pointers
        Type::TypeParam(_, _) => 8, // Assume pointer-sized for generics
        _ => 8, // Default to 8
    }
}

/// A MIR function
#[derive(Debug)]
pub struct MirFunction {
    pub def_id: DefId,
    pub name: String,
    pub params: Vec<MirLocal>,
    pub return_type: Type,
    pub locals: Vec<MirLocal>,
    pub blocks: Vec<BasicBlock>,
}

impl MirFunction {
    pub fn pretty_print(&self) -> String {
        let mut out = String::new();

        // Function signature
        let params: Vec<_> = self.params.iter()
            .map(|p| format!("{}: {:?}", p.name, p.ty))
            .collect();
        out.push_str(&format!("fn {}({}) -> {:?} {{\n", 
            self.name, params.join(", "), self.return_type));

        // Locals
        if !self.locals.is_empty() {
            out.push_str("  locals:\n");
            for local in &self.locals {
                out.push_str(&format!("    _{}: {:?} // {}\n", 
                    local.id, local.ty, local.name));
            }
            out.push('\n');
        }

        // Basic blocks
        for block in &self.blocks {
            out.push_str(&format!("  bb{}:\n", block.id));
            for stmt in &block.statements {
                out.push_str(&format!("    {}\n", stmt.pretty_print()));
            }
            out.push_str(&format!("    {}\n", block.terminator.pretty_print()));
            out.push('\n');
        }

        out.push_str("}\n");
        out
    }
}

/// A local variable in MIR
#[derive(Debug, Clone)]
pub struct MirLocal {
    pub id: u32,
    pub name: String,
    pub ty: Type,
    pub is_arg: bool,
}

/// A basic block
#[derive(Debug)]
pub struct BasicBlock {
    pub id: u32,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

/// A MIR statement
#[derive(Debug, Clone)]
pub struct Statement {
    pub kind: StatementKind,
}

impl Statement {
    pub fn pretty_print(&self) -> String {
        match &self.kind {
            StatementKind::Assign { place, rvalue } => {
                format!("{} = {}", place.pretty_print(), rvalue.pretty_print())
            }
            StatementKind::StorageLive(local) => format!("StorageLive(_{local})"),
            StatementKind::StorageDead(local) => format!("StorageDead(_{local})"),
            StatementKind::Nop => "nop".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StatementKind {
    /// Assign a value to a place
    Assign { place: Place, rvalue: Rvalue },
    /// Mark a local as live (for stack allocation)
    StorageLive(u32),
    /// Mark a local as dead
    StorageDead(u32),
    /// No operation
    Nop,
}

/// A place in memory (lvalue)
#[derive(Debug, Clone)]
pub struct Place {
    pub local: u32,
    pub projections: Vec<PlaceProjection>,
}

impl Place {
    pub fn local(id: u32) -> Self {
        Self { local: id, projections: Vec::new() }
    }

    pub fn field(mut self, idx: usize, name: String) -> Self {
        self.projections.push(PlaceProjection::Field(idx, name));
        self
    }

    pub fn deref(mut self) -> Self {
        self.projections.push(PlaceProjection::Deref);
        self
    }

    pub fn index(mut self, idx: Operand) -> Self {
        self.projections.push(PlaceProjection::Index(Box::new(idx)));
        self
    }

    pub fn pretty_print(&self) -> String {
        let mut s = format!("_{}", self.local);
        for proj in &self.projections {
            match proj {
                PlaceProjection::Field(_, name) => {
                    s = format!("{}.{}", s, name);
                }
                PlaceProjection::Deref => {
                    s = format!("(*{})", s);
                }
                PlaceProjection::Index(idx) => {
                    s = format!("{}[{}]", s, idx.pretty_print());
                }
            }
        }
        s
    }
}

#[derive(Debug, Clone)]
pub enum PlaceProjection {
    /// Field access by index and name
    Field(usize, String),
    /// Dereference
    Deref,
    /// Array/slice index
    Index(Box<Operand>),
}

/// An rvalue (right-hand side of assignment)
#[derive(Debug, Clone)]
pub enum Rvalue {
    /// Use a value (copy or move)
    Use(Operand),
    /// Take a reference
    Ref { is_mut: bool, place: Place },
    /// Binary operation
    BinaryOp { op: BinOp, left: Operand, right: Operand },
    /// Unary operation
    UnaryOp { op: UnaryOp, operand: Operand },
    /// Create an aggregate (struct, tuple, array)
    Aggregate { kind: AggregateKind, operands: Vec<Operand> },
    /// Get discriminant of enum
    Discriminant(Place),
    /// Cast between types
    Cast { operand: Operand, ty: Type },
}

impl Rvalue {
    pub fn pretty_print(&self) -> String {
        match self {
            Rvalue::Use(op) => op.pretty_print(),
            Rvalue::Ref { is_mut, place } => {
                let m = if *is_mut { "mut " } else { "" };
                format!("&{}{}", m, place.pretty_print())
            }
            Rvalue::BinaryOp { op, left, right } => {
                format!("{:?}({}, {})", op, left.pretty_print(), right.pretty_print())
            }
            Rvalue::UnaryOp { op, operand } => {
                format!("{:?}({})", op, operand.pretty_print())
            }
            Rvalue::Aggregate { kind, operands } => {
                let ops: Vec<_> = operands.iter().map(|o| o.pretty_print()).collect();
                format!("{:?}({})", kind, ops.join(", "))
            }
            Rvalue::Discriminant(place) => {
                format!("discriminant({})", place.pretty_print())
            }
            Rvalue::Cast { operand, ty } => {
                format!("{} as {:?}", operand.pretty_print(), ty)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AggregateKind {
    Tuple,
    Array,
    Struct(DefId, String),
    /// Enum variant: (enum DefId, variant index, variant DefId)
    Enum(DefId, usize, DefId),
}

/// An operand (value that can be used)
#[derive(Debug, Clone)]
pub enum Operand {
    /// Copy from a place
    Copy(Place),
    /// Move from a place
    Move(Place),
    /// Constant value
    Constant(Constant),
}

impl Operand {
    pub fn pretty_print(&self) -> String {
        match self {
            Operand::Copy(p) => format!("copy {}", p.pretty_print()),
            Operand::Move(p) => format!("move {}", p.pretty_print()),
            Operand::Constant(c) => c.pretty_print(),
        }
    }
}

/// A constant value
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64, Type),
    Float(f64, Type),
    Bool(bool),
    /// String literal (stored as global data, accessed via pointer)
    Str(String),
    Unit,
    /// Function reference
    FnPtr(DefId, String),
    /// External static reference (pointer to global data)
    ExternStatic(DefId, String, Type),
    /// Monomorphized generic function reference
    MonomorphizedFn(DefId, String, Vec<Type>),
    /// Trait method call - resolved to concrete method at codegen time
    TraitMethodCall {
        receiver_type: Type,
        method_name: String,
        trait_bounds: Vec<DefId>,
    },
}

impl Constant {
    pub fn pretty_print(&self) -> String {
        match self {
            Constant::Int(n, _) => format!("{}", n),
            Constant::Float(n, _) => format!("{}", n),
            Constant::Bool(b) => format!("{}", b),
            Constant::Str(s) => format!("\"{}\"", s),
            Constant::Unit => "()".to_string(),
            Constant::FnPtr(_, name) => format!("fn {}", name),
            Constant::ExternStatic(_, name, _) => format!("static {}", name),
            Constant::MonomorphizedFn(_, name, _) => format!("fn {}", name),
            Constant::TraitMethodCall { receiver_type, method_name, .. } => {
                format!("<{:?}>::{}", receiver_type, method_name)
            }
        }
    }
}

/// A block terminator
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Jump to another block
    Goto { target: u32 },
    /// Conditional branch
    SwitchInt { 
        discr: Operand, 
        targets: Vec<(i64, u32)>,
        otherwise: u32,
    },
    /// Return from function
    Return,
    /// Call a function
    Call {
        func: Operand,
        args: Vec<Operand>,
        destination: Place,
        target: u32,
    },
    /// Unreachable code
    Unreachable,
}

impl Terminator {
    pub fn pretty_print(&self) -> String {
        match self {
            Terminator::Goto { target } => format!("goto -> bb{}", target),
            Terminator::SwitchInt { discr, targets, otherwise } => {
                let arms: Vec<_> = targets.iter()
                    .map(|(val, bb)| format!("{} => bb{}", val, bb))
                    .collect();
                format!("switchInt({}) -> [{}; otherwise: bb{}]", 
                    discr.pretty_print(), arms.join(", "), otherwise)
            }
            Terminator::Return => "return".to_string(),
            Terminator::Call { func, args, destination, target } => {
                let args_str: Vec<_> = args.iter().map(|a| a.pretty_print()).collect();
                format!("{} = {}({}) -> bb{}", 
                    destination.pretty_print(), 
                    func.pretty_print(), 
                    args_str.join(", "),
                    target)
            }
            Terminator::Unreachable => "unreachable".to_string(),
        }
    }
}

/// Binary operations
#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
    BitAnd, BitOr, BitXor,
    Shl, Shr,
}

/// Unary operations
#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Not,
}

