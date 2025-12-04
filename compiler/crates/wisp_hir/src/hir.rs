//! HIR types - similar to AST but with resolved names

use wisp_lexer::Span;
use std::collections::HashMap;
use std::path::PathBuf;

/// Unique identifier for definitions (types, functions, variables)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefId(pub u32);

impl DefId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Unique identifier for modules (source files)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ModuleId(pub u32);

impl ModuleId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }
    
    /// The root module (the main source file being compiled)
    pub fn root() -> Self {
        Self(0)
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
    /// Module where this definition is located
    pub module_id: ModuleId,
    /// Whether this definition is public (accessible from other modules)
    pub is_pub: bool,
}

/// Information about a module (source file)
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub id: ModuleId,
    pub path: PathBuf,
    /// All definitions in this module
    pub defs: Vec<DefId>,
}

/// Registry of all modules in the program
#[derive(Debug, Default)]
pub struct ModuleRegistry {
    pub modules: HashMap<ModuleId, ModuleInfo>,
    pub path_to_id: HashMap<PathBuf, ModuleId>,
    next_id: u32,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Register a new module and return its ID
    pub fn register(&mut self, path: PathBuf) -> ModuleId {
        if let Some(&id) = self.path_to_id.get(&path) {
            return id;
        }
        
        let id = ModuleId::new(self.next_id);
        self.next_id += 1;
        
        self.modules.insert(id, ModuleInfo {
            id,
            path: path.clone(),
            defs: Vec::new(),
        });
        self.path_to_id.insert(path, id);
        
        id
    }
    
    /// Add a definition to a module
    pub fn add_def(&mut self, module_id: ModuleId, def_id: DefId) {
        if let Some(module) = self.modules.get_mut(&module_id) {
            module.defs.push(def_id);
        }
    }
    
    /// Get module ID by path
    pub fn get_by_path(&self, path: &PathBuf) -> Option<ModuleId> {
        self.path_to_id.get(path).copied()
    }
}

/// Namespace information for LSP
#[derive(Debug, Clone, Default)]
pub struct NamespaceData {
    /// Items in this namespace: name -> DefId
    pub items: HashMap<String, DefId>,
    /// Child namespaces
    pub children: HashMap<String, NamespaceData>,
}

/// The resolved program with all definitions
#[derive(Debug)]
pub struct ResolvedProgram {
    /// All definitions in the program
    pub defs: HashMap<DefId, DefInfo>,
    /// Map from name to definition at global scope
    pub globals: HashMap<String, DefId>,
    /// Module registry
    pub modules: ModuleRegistry,
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
    /// Namespaces for LSP completion
    pub namespaces: HashMap<String, NamespaceData>,
}

impl ResolvedProgram {
    /// Get the span of a definition by DefId (for go-to-definition)
    pub fn definition_span(&self, def_id: DefId) -> Option<Span> {
        self.defs.get(&def_id).map(|info| info.span)
    }
    
    /// Get all references to a definition (for find-references)
    /// Note: This is a placeholder - actual reference tracking would need to be added during resolution
    pub fn references_to(&self, _def_id: DefId) -> Vec<Span> {
        // TODO: Track references during name resolution
        Vec::new()
    }
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
            modules: ModuleRegistry::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            traits: Vec::new(),
            impls: Vec::new(),
            functions: Vec::new(),
            extern_functions: Vec::new(),
            extern_statics: Vec::new(),
            namespaces: HashMap::new(),
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
    pub type_params: Vec<ResolvedTypeParam>,
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
    pub type_params: Vec<ResolvedTypeParam>,  // Generic parameters: impl<T, U>
    pub trait_def: Option<DefId>,
    pub trait_type_args: Vec<ResolvedType>,  // Type arguments for trait
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
    pub default: Option<ResolvedType>,
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
    /// Span of just the function name (for hover)
    pub name_span: Span,
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
        /// Span of the field name (for hover on method calls)
        field_span: Span,
    },
    
    /// Struct literal
    StructLit {
        struct_def: DefId,
        /// (field_name, field_name_span, value_expr)
        fields: Vec<(String, Span, ResolvedExpr)>,
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
    
    /// For loop
    For {
        binding: DefId,
        binding_name: String,
        iter: Box<ResolvedExpr>,
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
    
    /// Array literal
    ArrayLit(Vec<ResolvedExpr>),
    
    /// Lambda/closure
    Lambda {
        params: Vec<ResolvedLambdaParam>,
        body: Box<ResolvedExpr>,
    },
    
    /// Type cast: expr as Type
    Cast {
        expr: Box<ResolvedExpr>,
        target_type: ResolvedType,
    },
    
    /// String interpolation: "hello {name}!"
    StringInterp {
        parts: Vec<ResolvedStringInterpPart>,
    },
    
    /// Namespace path (intermediate state for nested namespace resolution)
    /// e.g., `std.io` before accessing `.print`
    NamespacePath(Vec<String>),
    
    /// Error (for recovery)
    Error,
}

/// Part of an interpolated string (resolved)
#[derive(Debug, Clone)]
pub enum ResolvedStringInterpPart {
    Literal(String),
    Expr(ResolvedExpr),
}

/// Resolved lambda parameter
#[derive(Debug, Clone)]
pub struct ResolvedLambdaParam {
    pub def_id: DefId,
    pub name: String,
    pub ty: Option<ResolvedType>,
    pub span: Span,
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

