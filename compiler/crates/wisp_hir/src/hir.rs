//! HIR types - similar to AST but with resolved names

use wisp_lexer::Span;
use std::collections::HashMap;

/// Unique identifier for definitions (types, functions, variables)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

impl DefId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Kind of definition
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefKind {
    Struct,
    Enum,
    EnumVariant,
    Trait,
    Function,
    ExternFunction,
    ExternStatic,
    Method,
    Parameter,
    Local,
    Field,
    TypeParam,
}

/// Information about a definition
#[derive(Debug, Clone)]
pub struct DefInfo {
    pub id: DefId,
    pub name: String,
    pub kind: DefKind,
    pub span: Span,
    /// Parent definition (e.g., struct for a field, impl for a method)
    pub parent: Option<DefId>,
}

/// The resolved program with all definitions
#[derive(Debug)]
pub struct ResolvedProgram {
    /// All definitions in the program
    pub defs: HashMap<DefId, DefInfo>,
    /// Map from name to definition at global scope
    pub globals: HashMap<String, DefId>,
    /// Struct definitions
    pub structs: Vec<ResolvedStruct>,
    /// Enum definitions  
    pub enums: Vec<ResolvedEnum>,
    /// Trait definitions
    pub traits: Vec<ResolvedTrait>,
    /// Impl blocks
    pub impls: Vec<ResolvedImpl>,
    /// Free functions
    pub functions: Vec<ResolvedFunction>,
    /// External function declarations
    pub extern_functions: Vec<ResolvedExternFunction>,
    /// External static variable declarations
    pub extern_statics: Vec<ResolvedExternStatic>,
}

/// External function declaration (C FFI)
#[derive(Debug, Clone)]
pub struct ResolvedExternFunction {
    pub def_id: DefId,
    pub name: String,
    pub params: Vec<ResolvedParam>,
    pub return_type: Option<ResolvedType>,
    pub span: Span,
}

/// External static variable declaration (C FFI)
#[derive(Debug, Clone)]
pub struct ResolvedExternStatic {
    pub def_id: DefId,
    pub name: String,
    pub ty: ResolvedType,
    pub span: Span,
}

impl ResolvedProgram {
    pub fn new() -> Self {
        Self {
            defs: HashMap::new(),
            globals: HashMap::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            traits: Vec::new(),
            impls: Vec::new(),
            functions: Vec::new(),
            extern_functions: Vec::new(),
            extern_statics: Vec::new(),
        }
    }

    pub fn get_def(&self, id: DefId) -> Option<&DefInfo> {
        self.defs.get(&id)
    }

    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        
        out.push_str("=== Resolved Program ===\n\n");
        
        out.push_str("--- Globals ---\n");
        for (name, id) in &self.globals {
            let def = self.defs.get(id).unwrap();
            out.push_str(&format!("  {} -> {:?} ({:?})\n", name, id, def.kind));
        }
        out.push('\n');
        
        out.push_str("--- Structs ---\n");
        for s in &self.structs {
            out.push_str(&format!("  {} ({:?})\n", s.name, s.def_id));
            for f in &s.fields {
                out.push_str(&format!("    .{}: {:?} ({:?})\n", f.name, f.ty, f.def_id));
            }
        }
        out.push('\n');
        
        out.push_str("--- Enums ---\n");
        for e in &self.enums {
            out.push_str(&format!("  {} ({:?})\n", e.name, e.def_id));
            for v in &e.variants {
                if v.fields.is_empty() {
                    out.push_str(&format!("    ::{} ({:?})\n", v.name, v.def_id));
                } else {
                    let fields: Vec<_> = v.fields.iter()
                        .map(|f| format!("{}: {:?}", f.name, f.ty))
                        .collect();
                    out.push_str(&format!("    ::{}({}) ({:?})\n", v.name, fields.join(", "), v.def_id));
                }
            }
        }
        out.push('\n');
        
        out.push_str("--- Traits ---\n");
        for t in &self.traits {
            out.push_str(&format!("  {} ({:?})\n", t.name, t.def_id));
            for m in &t.methods {
                out.push_str(&format!("    fn {} ({:?})\n", m.name, m.def_id));
            }
        }
        out.push('\n');
        
        out.push_str("--- Impls ---\n");
        for i in &self.impls {
            let target = format!("{:?}", i.target_type);
            if let Some(trait_id) = i.trait_def {
                let trait_name = self.defs.get(&trait_id).map(|d| d.name.as_str()).unwrap_or("?");
                out.push_str(&format!("  impl {} for {}\n", trait_name, target));
            } else {
                out.push_str(&format!("  impl {}\n", target));
            }
            for m in &i.methods {
                out.push_str(&format!("    fn {} ({:?})\n", m.name, m.def_id));
            }
        }
        out.push('\n');
        
        out.push_str("--- Functions ---\n");
        for f in &self.functions {
            out.push_str(&format!("  fn {} ({:?})\n", f.name, f.def_id));
            for p in &f.params {
                out.push_str(&format!("    param {}: {:?} ({:?})\n", p.name, p.ty, p.def_id));
            }
            out.push_str(&format!("    locals: {}\n", f.locals.len()));
        }
        out.push('\n');
        
        out.push_str("--- Extern Functions ---\n");
        for f in &self.extern_functions {
            let params: Vec<_> = f.params.iter()
                .map(|p| format!("{}: {:?}", p.name, p.ty))
                .collect();
            let ret = f.return_type.as_ref()
                .map(|t| format!(" -> {:?}", t))
                .unwrap_or_default();
            out.push_str(&format!("  extern fn {}({}){} ({:?})\n", f.name, params.join(", "), ret, f.def_id));
        }
        out.push('\n');
        
        out.push_str("--- Extern Statics ---\n");
        for s in &self.extern_statics {
            out.push_str(&format!("  extern static {}: {:?} ({:?})\n", s.name, s.ty, s.def_id));
        }
        
        out
    }
}

impl Default for ResolvedProgram {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolved struct definition
#[derive(Debug, Clone)]
pub struct ResolvedStruct {
    pub def_id: DefId,
    pub name: String,
    pub fields: Vec<ResolvedField>,
    pub span: Span,
}

/// Resolved field
#[derive(Debug, Clone)]
pub struct ResolvedField {
    pub def_id: DefId,
    pub name: String,
    pub ty: ResolvedType,
    pub span: Span,
}

/// Resolved enum definition
#[derive(Debug, Clone)]
pub struct ResolvedEnum {
    pub def_id: DefId,
    pub name: String,
    pub variants: Vec<ResolvedVariant>,
    pub span: Span,
}

/// Resolved enum variant
#[derive(Debug, Clone)]
pub struct ResolvedVariant {
    pub def_id: DefId,
    pub name: String,
    pub fields: Vec<ResolvedField>,
    pub span: Span,
}

/// Resolved trait definition
#[derive(Debug, Clone)]
pub struct ResolvedTrait {
    pub def_id: DefId,
    pub name: String,
    pub methods: Vec<ResolvedFunction>,
    pub span: Span,
}

/// Resolved impl block
#[derive(Debug, Clone)]
pub struct ResolvedImpl {
    pub trait_def: Option<DefId>,
    pub target_type: ResolvedType,
    pub methods: Vec<ResolvedFunction>,
    pub span: Span,
}

/// Resolved type parameter
#[derive(Debug, Clone)]
pub struct ResolvedTypeParam {
    pub def_id: DefId,
    pub name: String,
    pub bounds: Vec<ResolvedType>,
    pub span: Span,
}

/// Resolved function
#[derive(Debug, Clone)]
pub struct ResolvedFunction {
    pub def_id: DefId,
    pub name: String,
    pub type_params: Vec<ResolvedTypeParam>,
    pub params: Vec<ResolvedParam>,
    pub return_type: Option<ResolvedType>,
    pub body: Option<ResolvedBlock>,
    /// Local variables defined in this function
    pub locals: Vec<DefId>,
    pub span: Span,
}

/// Resolved parameter
#[derive(Debug, Clone)]
pub struct ResolvedParam {
    pub def_id: DefId,
    pub name: String,
    pub is_mut: bool,
    pub ty: ResolvedType,
    pub span: Span,
}

/// Resolved type
#[derive(Debug, Clone)]
pub enum ResolvedType {
    /// Named type with resolved DefId (None if primitive/builtin) and type arguments
    Named { name: String, def_id: Option<DefId>, type_args: Vec<ResolvedType> },
    /// Reference type
    Ref { is_mut: bool, inner: Box<ResolvedType> },
    /// Slice type
    Slice { elem: Box<ResolvedType> },
    /// Unit type
    Unit,
    /// Self type (in trait/impl context)
    SelfType,
    /// Unresolved (error recovery)
    Error,
}

/// Resolved block
#[derive(Debug, Clone)]
pub struct ResolvedBlock {
    pub stmts: Vec<ResolvedStmt>,
    pub span: Span,
}

/// Resolved statement
#[derive(Debug, Clone)]
pub enum ResolvedStmt {
    Let {
        def_id: DefId,
        name: String,
        is_mut: bool,
        ty: Option<ResolvedType>,
        init: Option<ResolvedExpr>,
        span: Span,
    },
    Expr(ResolvedExpr),
}

/// Resolved expression
#[derive(Debug, Clone)]
pub struct ResolvedExpr {
    pub kind: ResolvedExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ResolvedExprKind {
    /// Literal values
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    StringLiteral(String),
    
    /// Variable reference with resolved DefId
    Var { name: String, def_id: DefId },
    
    /// Binary operation
    Binary {
        left: Box<ResolvedExpr>,
        op: wisp_ast::BinOp,
        right: Box<ResolvedExpr>,
    },
    
    /// Unary operation
    Unary {
        op: wisp_ast::UnaryOp,
        expr: Box<ResolvedExpr>,
    },
    
    /// Function/method call
    Call {
        callee: Box<ResolvedExpr>,
        args: Vec<ResolvedCallArg>,
    },
    
    /// Field access
    Field {
        expr: Box<ResolvedExpr>,
        field: String,
        field_def: Option<DefId>,
    },
    
    /// Struct literal
    StructLit {
        struct_def: DefId,
        fields: Vec<(String, ResolvedExpr)>,
    },
    
    /// If expression
    If {
        cond: Box<ResolvedExpr>,
        then_block: ResolvedBlock,
        else_block: Option<ResolvedElse>,
    },
    
    /// While loop
    While {
        cond: Box<ResolvedExpr>,
        body: ResolvedBlock,
    },
    
    /// Block expression
    Block(ResolvedBlock),
    
    /// Assignment
    Assign {
        target: Box<ResolvedExpr>,
        value: Box<ResolvedExpr>,
    },
    
    /// Reference
    Ref {
        is_mut: bool,
        expr: Box<ResolvedExpr>,
    },
    
    /// Dereference
    Deref(Box<ResolvedExpr>),
    
    /// Match expression
    Match {
        scrutinee: Box<ResolvedExpr>,
        arms: Vec<ResolvedMatchArm>,
    },
    
    /// Index expression
    Index {
        expr: Box<ResolvedExpr>,
        index: Box<ResolvedExpr>,
    },
    
    /// Error (for recovery)
    Error,
}

/// Resolved function call argument
#[derive(Debug, Clone)]
pub struct ResolvedCallArg {
    /// If Some, this is a named argument
    pub name: Option<String>,
    pub value: ResolvedExpr,
    pub span: Span,
}

/// Resolved else branch
#[derive(Debug, Clone)]
pub enum ResolvedElse {
    Block(ResolvedBlock),
    If(Box<ResolvedExpr>),
}

/// Resolved match arm
#[derive(Debug, Clone)]
pub struct ResolvedMatchArm {
    pub pattern: ResolvedPattern,
    pub body: ResolvedExpr,
    pub span: Span,
}

/// Resolved pattern
#[derive(Debug, Clone)]
pub struct ResolvedPattern {
    pub kind: ResolvedPatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ResolvedPatternKind {
    Wildcard,
    Binding { def_id: DefId, name: String },
    Literal(ResolvedExpr),
    Variant { 
        variant_def: DefId,
        fields: Vec<ResolvedPattern>,
    },
}

