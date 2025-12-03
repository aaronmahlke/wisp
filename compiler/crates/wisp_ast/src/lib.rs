use wisp_lexer::Span;

/// Unique identifier for AST nodes
pub type NodeId = u32;

/// A complete Wisp source file
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub items: Vec<Item>,
}

/// A module with its namespace information
#[derive(Debug, Clone)]
pub struct ImportedModule {
    /// The import declaration that brought this module in
    pub import: ImportDecl,
    /// Items defined in this module (not including imports)
    pub items: Vec<Item>,
    /// Imports declared within this module (for scope resolution)
    pub module_imports: Vec<ImportDecl>,
    /// If true, this module was imported transitively through another module
    /// and should NOT create a top-level namespace
    pub is_transitive: bool,
}

/// A source file with import information preserved
#[derive(Debug, Clone)]
pub struct SourceFileWithImports {
    /// Items defined in this file
    pub local_items: Vec<Item>,
    /// Imported modules with their namespaces
    pub imported_modules: Vec<ImportedModule>,
}

/// Top-level items
#[derive(Debug, Clone)]
pub enum Item {
    Import(ImportDecl),
    Function(FnDef),
    ExternFunction(ExternFnDef),
    ExternStatic(ExternStaticDef),
    Struct(StructDef),
    Enum(EnumDef),
    Trait(TraitDef),
    Impl(ImplBlock),
}

/// Import path type
#[derive(Debug, Clone, PartialEq)]
pub enum ImportPath {
    /// Standard library: std/io
    Std(Vec<String>),
    /// Project-relative: @/utils/math
    Project(Vec<String>),
    /// External package: pkg/name/sub (future)
    Package(String, Vec<String>),
}

impl ImportPath {
    /// Get the last segment of the path (for default namespace name)
    pub fn last_segment(&self) -> Option<&str> {
        match self {
            ImportPath::Std(segs) => {
                if segs.is_empty() {
                    Some("std")  // `import std` -> namespace is "std"
                } else {
                    segs.last().map(|s| s.as_str())
                }
            }
            ImportPath::Project(segs) => segs.last().map(|s| s.as_str()),
            ImportPath::Package(name, segs) => {
                if segs.is_empty() {
                    Some(name.as_str())  // `import pkg/foo` -> namespace is "foo"
                } else {
                    segs.last().map(|s| s.as_str())
                }
            }
        }
    }
}

/// Import item (for destructuring imports)
#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: Ident,
    pub alias: Option<Ident>,
    pub span: Span,
}

/// Import declaration
#[derive(Debug, Clone)]
pub struct ImportDecl {
    /// Whether this is a `pub import` (for re-exporting in mod.ws)
    pub is_pub: bool,
    /// The import path (std/io, @/utils, etc.)
    pub path: ImportPath,
    /// Namespace alias: `import std/io as stdio`
    pub alias: Option<Ident>,
    /// Destructured items: `import { print, File as F } from std/io`
    pub items: Option<Vec<ImportItem>>,
    /// If true, only destructured items are imported (no namespace)
    /// `import { print } from std/io` vs `import std/io { print }`
    pub destructure_only: bool,
    pub span: Span,
}

/// External function declaration (C FFI)
#[derive(Debug, Clone)]
pub struct ExternFnDef {
    pub is_pub: bool,
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub span: Span,
}

/// External static variable declaration (C FFI)
#[derive(Debug, Clone)]
pub struct ExternStaticDef {
    pub is_pub: bool,
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

/// Generic type parameter
#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: Ident,
    pub bounds: Vec<TypeExpr>,  // Trait bounds like T: Clone + Debug
    pub default: Option<TypeExpr>,  // Default type like T = i32
    pub span: Span,
}

/// Function definition
#[derive(Debug, Clone)]
pub struct FnDef {
    pub is_pub: bool,
    pub name: Ident,
    pub type_params: Vec<GenericParam>,  // Generic type parameters
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Option<Block>,  // None for trait method signatures
    pub span: Span,
}

/// Function parameter
#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub is_mut: bool,
    pub ty: TypeExpr,
    pub span: Span,
}

/// Struct definition
#[derive(Debug, Clone)]
pub struct StructDef {
    pub is_pub: bool,
    pub name: Ident,
    pub type_params: Vec<GenericParam>,  // Generic type parameters
    pub fields: Vec<StructField>,
    pub span: Span,
}

/// Struct field
#[derive(Debug, Clone)]
pub struct StructField {
    pub name: Ident,
    pub ty: TypeExpr,
    pub span: Span,
}

/// Enum definition
#[derive(Debug, Clone)]
pub struct EnumDef {
    pub is_pub: bool,
    pub name: Ident,
    pub type_params: Vec<GenericParam>,  // Generic type parameters
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

/// Enum variant
#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: Ident,
    pub fields: Vec<StructField>,  // Empty for unit variants
    pub span: Span,
}

/// Trait definition
#[derive(Debug, Clone)]
pub struct TraitDef {
    pub is_pub: bool,
    pub name: Ident,
    pub type_params: Vec<GenericParam>,  // Generic type parameters
    pub methods: Vec<FnDef>,
    pub span: Span,
}

/// Impl block
#[derive(Debug, Clone)]
pub struct ImplBlock {
    pub trait_name: Option<Ident>,  // None for inherent impl
    pub trait_type_args: Vec<TypeExpr>,  // Type arguments for trait (e.g., Add<i32>)
    pub target_type: TypeExpr,
    pub methods: Vec<FnDef>,
    pub span: Span,
}

/// A block of statements
#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// Statements
#[derive(Debug, Clone)]
pub enum Stmt {
    Let(LetStmt),
    Expr(ExprStmt),
}

/// Let binding
#[derive(Debug, Clone)]
pub struct LetStmt {
    pub name: Ident,
    pub is_mut: bool,
    pub ty: Option<TypeExpr>,
    pub init: Option<Expr>,
    pub span: Span,
}

/// Expression statement
#[derive(Debug, Clone)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

/// Expressions
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    /// Integer literal: 42
    IntLiteral(i64),
    /// Float literal: 3.14
    FloatLiteral(f64),
    /// Boolean literal: true, false
    BoolLiteral(bool),
    /// String literal: "hello"
    StringLiteral(String),
    /// Identifier: foo
    Ident(Ident),
    /// Binary operation: a + b
    Binary(Box<Expr>, BinOp, Box<Expr>),
    /// Unary operation: -x, !x
    Unary(UnaryOp, Box<Expr>),
    /// Function call: foo(a, b) or foo(x: a, y: b)
    Call(Box<Expr>, Vec<CallArg>),
    /// Field access: foo.bar
    Field(Box<Expr>, Ident),
    /// Struct literal: Point { x: 1, y: 2 }
    StructLit(Ident, Vec<FieldInit>),
    /// If expression: if cond { ... } else { ... }
    If(Box<Expr>, Block, Option<ElseBranch>),
    /// While loop: while cond { ... }
    While(Box<Expr>, Block),
    /// For loop: for x in iter { ... }
    For(Ident, Box<Expr>, Block),
    /// Block expression: { ... }
    Block(Block),
    /// Assignment: x = expr
    Assign(Box<Expr>, Box<Expr>),
    /// Reference: &expr, &mut expr
    Ref(bool, Box<Expr>),  // (is_mut, expr)
    /// Dereference: *expr
    Deref(Box<Expr>),
    /// Match expression: match expr { ... }
    Match(Box<Expr>, Vec<MatchArm>),
    /// Index expression: arr[idx]
    Index(Box<Expr>, Box<Expr>),
    /// Array literal: [1, 2, 3]
    ArrayLit(Vec<Expr>),
    /// Lambda/closure: (x, y) -> x + y
    Lambda(Vec<LambdaParam>, Box<Expr>),
    /// Type cast: expr as Type
    Cast(Box<Expr>, TypeExpr),
    /// String interpolation: "hello {name}!"
    /// Parts alternate between string literals and expressions
    StringInterp(Vec<StringInterpPart>),
}

/// Part of an interpolated string
#[derive(Debug, Clone)]
pub enum StringInterpPart {
    /// Literal string part
    Literal(String),
    /// Interpolated expression: {expr}
    Expr(Expr),
}

/// Lambda parameter (may have type annotation)
#[derive(Debug, Clone)]
pub struct LambdaParam {
    pub name: Ident,
    pub ty: Option<TypeExpr>,
    pub span: Span,
}

/// Else branch - can be a block or another if
#[derive(Debug, Clone)]
pub enum ElseBranch {
    Block(Block),
    If(Box<Expr>),  // else if ... 
}

/// Match arm
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// Patterns for match
#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
    /// Wildcard: _
    Wildcard,
    /// Identifier binding: x
    Ident(Ident),
    /// Literal: 42, "hello", true
    Literal(Expr),
    /// Enum variant: Some(x), None
    Variant(Ident, Vec<Pattern>),
}

/// Field initializer in struct literal
#[derive(Debug, Clone)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
    pub span: Span,
}

/// Function call argument - can be positional or named
#[derive(Debug, Clone)]
pub struct CallArg {
    /// If Some, this is a named argument (name: value)
    pub name: Option<Ident>,
    pub value: Expr,
    pub span: Span,
}

impl CallArg {
    /// Pretty print with indentation for full AST display
    pub fn pretty_print_indented(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        if let Some(name) = &self.name {
            let mut out = format!("{}NamedArg({}):\n", ind, name.name);
            out.push_str(&self.value.pretty_print_indented(indent + 1));
            out
        } else {
            self.value.pretty_print_indented(indent)
        }
    }
    
    /// Compact pretty print (for inline display)
    pub fn pretty_print(&self) -> String {
        if let Some(name) = &self.name {
            format!("{}: {}", name.name, self.value.pretty_print())
        } else {
            self.value.pretty_print()
        }
    }
}

/// Binary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    // Logical
    And,
    Or,
    // Range
    Range,  // ..
}

impl BinOp {
    pub fn precedence(self) -> u8 {
        match self {
            BinOp::Range => 0,  // Lowest precedence
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::NotEq => 3,
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 6,
        }
    }
}

impl std::fmt::Display for BinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Mod => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::NotEq => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Gt => write!(f, ">"),
            BinOp::LtEq => write!(f, "<="),
            BinOp::GtEq => write!(f, ">="),
            BinOp::And => write!(f, "&&"),
            BinOp::Or => write!(f, "||"),
            BinOp::Range => write!(f, ".."),
        }
    }
}

/// Unary operators
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,  // -
    Not,  // !
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnaryOp::Neg => write!(f, "-"),
            UnaryOp::Not => write!(f, "!"),
        }
    }
}

/// Type expressions
#[derive(Debug, Clone)]
pub struct TypeExpr {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeKind {
    /// Named type: i32, Point, Vec<i32>, Option<T>
    Named(Ident, Vec<TypeExpr>),  // (name, type_args)
    /// Reference type: &T, &mut T
    Ref(bool, Box<TypeExpr>),  // (is_mut, inner)
    /// Slice type: &[T]
    Slice(Box<TypeExpr>),
    /// Array type: [T; N]
    Array(Box<TypeExpr>, Box<Expr>),
    /// Tuple type: (T, U, V)
    Tuple(Vec<TypeExpr>),
    /// Unit type: ()
    Unit,
}

/// Identifier with span
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

impl Ident {
    pub fn new(name: String, span: Span) -> Self {
        Self { name, span }
    }
}

// === Pretty Printing ===

impl SourceFile {
    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            out.push_str(&item.pretty_print(0));
            out.push('\n');
        }
        out
    }
}

impl Item {
    pub fn pretty_print(&self, indent: usize) -> String {
        match self {
            Item::Import(i) => {
                let ind = "  ".repeat(indent);
                let path_str = match &i.path {
                    ImportPath::Std(segs) => format!("std/{}", segs.join("/")),
                    ImportPath::Project(segs) => format!("@/{}", segs.join("/")),
                    ImportPath::Package(name, segs) => format!("pkg/{}/{}", name, segs.join("/")),
                };
                let alias_str = i.alias.as_ref()
                    .map(|a| format!(" as {}", a.name))
                    .unwrap_or_default();
                let items_str = i.items.as_ref()
                    .map(|items| {
                        let names: Vec<_> = items.iter().map(|item| {
                            if let Some(alias) = &item.alias {
                                format!("{} as {}", item.name.name, alias.name)
                            } else {
                                item.name.name.clone()
                            }
                        }).collect();
                        format!(" {{ {} }}", names.join(", "))
                    })
                    .unwrap_or_default();
                let from_str = if i.destructure_only { " from " } else { "" };
                format!("{}Import{}{}{}{}\n", ind, from_str, items_str, path_str, alias_str)
            }
            Item::Function(f) => f.pretty_print(indent),
            Item::ExternFunction(f) => f.pretty_print(indent),
            Item::ExternStatic(s) => s.pretty_print(indent),
            Item::Struct(s) => s.pretty_print(indent),
            Item::Enum(e) => e.pretty_print(indent),
            Item::Trait(t) => t.pretty_print(indent),
            Item::Impl(i) => i.pretty_print(indent),
        }
    }
}

impl ExternStaticDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        format!("{}{}ExternStatic '{}': {}\n", ind, pub_str, self.name.name, self.ty.pretty_print())
    }
}

impl ExternFnDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        let params_str = self.params.iter()
            .map(|p| format!("{}: {}", p.name.name, p.ty.pretty_print()))
            .collect::<Vec<_>>()
            .join(", ");
        let ret_str = self.return_type.as_ref()
            .map(|t| format!(" -> {}", t.pretty_print()))
            .unwrap_or_default();
        format!("{}{}ExternFn '{}'({}){}\n", ind, pub_str, self.name.name, params_str, ret_str)
    }
}

impl FnDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        
        // Format generic parameters
        let generics = if self.type_params.is_empty() {
            String::new()
        } else {
            let params: Vec<_> = self.type_params.iter()
                .map(|p| {
                    if p.bounds.is_empty() {
                        p.name.name.clone()
                    } else {
                        let bounds: Vec<_> = p.bounds.iter().map(|b| b.pretty_print()).collect();
                        format!("{}: {}", p.name.name, bounds.join(" + "))
                    }
                })
                .collect();
            format!("<{}>", params.join(", "))
        };
        
        let mut out = format!("{}{}FnDef '{}{}'\n", ind, pub_str, self.name.name, generics);
        
        if !self.params.is_empty() {
            out.push_str(&format!("{}  params:\n", ind));
            for p in &self.params {
                let mut_str = if p.is_mut { "mut " } else { "" };
                out.push_str(&format!("{}    {}{}: {}\n", ind, mut_str, p.name.name, p.ty.pretty_print()));
            }
        }
        
        if let Some(ret) = &self.return_type {
            out.push_str(&format!("{}  returns: {}\n", ind, ret.pretty_print()));
        }
        
        if let Some(body) = &self.body {
            out.push_str(&format!("{}  body:\n", ind));
            out.push_str(&body.pretty_print(indent + 2));
        } else {
            out.push_str(&format!("{}  (signature only)\n", ind));
        }
        out
    }
}

impl StructDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        
        // Format generic parameters
        let generics = if self.type_params.is_empty() {
            String::new()
        } else {
            let params: Vec<_> = self.type_params.iter()
                .map(|p| p.name.name.clone())
                .collect();
            format!("<{}>", params.join(", "))
        };
        
        let mut out = format!("{}{}StructDef '{}{}'\n", ind, pub_str, self.name.name, generics);
        for field in &self.fields {
            out.push_str(&format!("{}  {}: {}\n", ind, field.name.name, field.ty.pretty_print()));
        }
        out
    }
}

impl EnumDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        let mut out = format!("{}{}EnumDef '{}'\n", ind, pub_str, self.name.name);
        for variant in &self.variants {
            if variant.fields.is_empty() {
                out.push_str(&format!("{}  {}\n", ind, variant.name.name));
            } else {
                let fields_str = variant.fields.iter()
                    .map(|f| format!("{}: {}", f.name.name, f.ty.pretty_print()))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("{}  {}({})\n", ind, variant.name.name, fields_str));
            }
        }
        out
    }
}

impl TraitDef {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let pub_str = if self.is_pub { "pub " } else { "" };
        let mut out = format!("{}{}TraitDef '{}'\n", ind, pub_str, self.name.name);
        for method in &self.methods {
            out.push_str(&method.pretty_print(indent + 1));
        }
        out
    }
}

impl ImplBlock {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let target = self.target_type.pretty_print();
        let mut out = if let Some(trait_name) = &self.trait_name {
            format!("{}Impl {} for {}\n", ind, trait_name.name, target)
        } else {
            format!("{}Impl {}\n", ind, target)
        };
        for method in &self.methods {
            out.push_str(&method.pretty_print(indent + 1));
        }
        out
    }
}

impl Block {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        let mut out = format!("{}Block\n", ind);
        for stmt in &self.stmts {
            out.push_str(&stmt.pretty_print(indent + 1));
        }
        out
    }
}

impl Stmt {
    pub fn pretty_print(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        match self {
            Stmt::Let(l) => {
                let mut_str = if l.is_mut { "mut " } else { "" };
                let ty_str = l.ty.as_ref().map(|t| format!(": {}", t.pretty_print())).unwrap_or_default();
                let mut out = format!("{}Let {}{}{}", ind, mut_str, l.name.name, ty_str);
                if let Some(init) = &l.init {
                    out.push_str(" =\n");
                    out.push_str(&init.pretty_print_indented(indent + 1));
                } else {
                    out.push('\n');
                }
                out
            }
            Stmt::Expr(e) => {
                let mut out = format!("{}ExprStmt\n", ind);
                out.push_str(&e.expr.pretty_print_indented(indent + 1));
                out
            }
        }
    }
}

impl Expr {
    /// Pretty print with indentation for full AST display
    pub fn pretty_print_indented(&self, indent: usize) -> String {
        let ind = "  ".repeat(indent);
        match &self.kind {
            ExprKind::IntLiteral(n) => format!("{}Int({})\n", ind, n),
            ExprKind::FloatLiteral(n) => format!("{}Float({})\n", ind, n),
            ExprKind::BoolLiteral(b) => format!("{}Bool({})\n", ind, b),
            ExprKind::StringLiteral(s) => format!("{}String(\"{}\")\n", ind, s),
            ExprKind::Ident(id) => format!("{}Ident({})\n", ind, id.name),
            ExprKind::Binary(l, op, r) => {
                let mut out = format!("{}Binary({})\n", ind, op);
                out.push_str(&l.pretty_print_indented(indent + 1));
                out.push_str(&r.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::Unary(op, e) => {
                let mut out = format!("{}Unary({})\n", ind, op);
                out.push_str(&e.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::Call(callee, args) => {
                let mut out = format!("{}Call\n", ind);
                out.push_str(&format!("{}callee:\n", "  ".repeat(indent + 1)));
                out.push_str(&callee.pretty_print_indented(indent + 2));
                if !args.is_empty() {
                    out.push_str(&format!("{}args:\n", "  ".repeat(indent + 1)));
                    for arg in args {
                        out.push_str(&arg.pretty_print_indented(indent + 2));
                    }
                }
                out
            }
            ExprKind::Field(e, f) => {
                let mut out = format!("{}Field(.{})\n", ind, f.name);
                out.push_str(&e.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::StructLit(name, fields) => {
                let mut out = format!("{}StructLit({})\n", ind, name.name);
                for field in fields {
                    out.push_str(&format!("{}.{}:\n", "  ".repeat(indent + 1), field.name.name));
                    out.push_str(&field.value.pretty_print_indented(indent + 2));
                }
                out
            }
            ExprKind::If(cond, then_block, else_branch) => {
                let mut out = format!("{}If\n", ind);
                out.push_str(&format!("{}condition:\n", "  ".repeat(indent + 1)));
                out.push_str(&cond.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}then:\n", "  ".repeat(indent + 1)));
                out.push_str(&then_block.pretty_print(indent + 2));
                if let Some(else_br) = else_branch {
                    out.push_str(&format!("{}else:\n", "  ".repeat(indent + 1)));
                    match else_br {
                        ElseBranch::Block(b) => out.push_str(&b.pretty_print(indent + 2)),
                        ElseBranch::If(e) => out.push_str(&e.pretty_print_indented(indent + 2)),
                    }
                }
                out
            }
            ExprKind::While(cond, body) => {
                let mut out = format!("{}While\n", ind);
                out.push_str(&format!("{}condition:\n", "  ".repeat(indent + 1)));
                out.push_str(&cond.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}body:\n", "  ".repeat(indent + 1)));
                out.push_str(&body.pretty_print(indent + 2));
                out
            }
            ExprKind::For(binding, iter, body) => {
                let mut out = format!("{}For\n", ind);
                out.push_str(&format!("{}binding: {}\n", "  ".repeat(indent + 1), binding.name));
                out.push_str(&format!("{}iter:\n", "  ".repeat(indent + 1)));
                out.push_str(&iter.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}body:\n", "  ".repeat(indent + 1)));
                out.push_str(&body.pretty_print(indent + 2));
                out
            }
            ExprKind::Block(block) => {
                let mut out = format!("{}Block\n", ind);
                for stmt in &block.stmts {
                    out.push_str(&stmt.pretty_print(indent + 1));
                }
                out
            }
            ExprKind::Assign(lhs, rhs) => {
                let mut out = format!("{}Assign\n", ind);
                out.push_str(&format!("{}target:\n", "  ".repeat(indent + 1)));
                out.push_str(&lhs.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}value:\n", "  ".repeat(indent + 1)));
                out.push_str(&rhs.pretty_print_indented(indent + 2));
                out
            }
            ExprKind::Ref(is_mut, e) => {
                let mut_str = if *is_mut { "mut " } else { "" };
                let mut out = format!("{}Ref({})\n", ind, mut_str);
                out.push_str(&e.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::Deref(e) => {
                let mut out = format!("{}Deref\n", ind);
                out.push_str(&e.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::Match(expr, arms) => {
                let mut out = format!("{}Match\n", ind);
                out.push_str(&format!("{}scrutinee:\n", "  ".repeat(indent + 1)));
                out.push_str(&expr.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}arms:\n", "  ".repeat(indent + 1)));
                for arm in arms {
                    out.push_str(&format!("{}pattern: {}\n", "  ".repeat(indent + 2), arm.pattern.pretty_print()));
                    out.push_str(&format!("{}body:\n", "  ".repeat(indent + 2)));
                    out.push_str(&arm.body.pretty_print_indented(indent + 3));
                }
                out
            }
            ExprKind::Index(e, idx) => {
                let mut out = format!("{}Index\n", ind);
                out.push_str(&format!("{}base:\n", "  ".repeat(indent + 1)));
                out.push_str(&e.pretty_print_indented(indent + 2));
                out.push_str(&format!("{}index:\n", "  ".repeat(indent + 1)));
                out.push_str(&idx.pretty_print_indented(indent + 2));
                out
            }
            ExprKind::ArrayLit(elements) => {
                let mut out = format!("{}ArrayLit\n", ind);
                for elem in elements {
                    out.push_str(&elem.pretty_print_indented(indent + 1));
                }
                out
            }
            ExprKind::Lambda(params, body) => {
                let params_str = params.iter()
                    .map(|p| {
                        if let Some(ty) = &p.ty {
                            format!("{}: {}", p.name.name, ty.pretty_print())
                        } else {
                            p.name.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut out = format!("{}Lambda(({}))\n", ind, params_str);
                out.push_str(&body.pretty_print_indented(indent + 1));
                out
            }
            ExprKind::Cast(expr, ty) => {
                format!("{}Cast({} as {})\n", ind, expr.pretty_print(), ty.pretty_print())
            }
            ExprKind::StringInterp(parts) => {
                let mut out = format!("{}StringInterp\n", ind);
                for part in parts {
                    match part {
                        StringInterpPart::Literal(s) => {
                            out.push_str(&format!("{}  Literal: \"{}\"\n", ind, s));
                        }
                        StringInterpPart::Expr(e) => {
                            out.push_str(&format!("{}  Expr:\n", ind));
                            out.push_str(&e.pretty_print_indented(indent + 2));
                        }
                    }
                }
                out
            }
        }
    }
    
    /// Compact pretty print (for inline display)
    pub fn pretty_print(&self) -> String {
        match &self.kind {
            ExprKind::IntLiteral(n) => format!("{}", n),
            ExprKind::FloatLiteral(n) => format!("{}", n),
            ExprKind::BoolLiteral(b) => format!("{}", b),
            ExprKind::StringLiteral(s) => format!("\"{}\"", s),
            ExprKind::Ident(id) => id.name.clone(),
            ExprKind::Binary(l, op, r) => format!("({} {} {})", l.pretty_print(), op, r.pretty_print()),
            ExprKind::Unary(op, e) => format!("({}{})", op, e.pretty_print()),
            ExprKind::Call(callee, args) => {
                let args_str = args.iter().map(|a| a.pretty_print()).collect::<Vec<_>>().join(", ");
                format!("{}({})", callee.pretty_print(), args_str)
            }
            ExprKind::Field(e, f) => format!("{}.{}", e.pretty_print(), f.name),
            ExprKind::StructLit(name, fields) => {
                let fields_str = fields.iter()
                    .map(|f| format!("{}: {}", f.name.name, f.value.pretty_print()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {} }}", name.name, fields_str)
            }
            ExprKind::If(cond, then_block, else_branch) => {
                let then_str = if then_block.stmts.len() == 1 {
                    if let Stmt::Expr(e) = &then_block.stmts[0] {
                        e.expr.pretty_print()
                    } else {
                        "...".to_string()
                    }
                } else {
                    "...".to_string()
                };
                let else_str = match else_branch {
                    Some(ElseBranch::Block(b)) => {
                        if b.stmts.len() == 1 {
                            if let Stmt::Expr(e) = &b.stmts[0] {
                                format!(" else {{ {} }}", e.expr.pretty_print())
                            } else {
                                " else { ... }".to_string()
                            }
                        } else {
                            " else { ... }".to_string()
                        }
                    }
                    Some(ElseBranch::If(e)) => format!(" else {}", e.pretty_print()),
                    None => "".to_string(),
                };
                format!("if {} {{ {} }}{}", cond.pretty_print(), then_str, else_str)
            }
            ExprKind::While(cond, _) => format!("while {} {{ ... }}", cond.pretty_print()),
            ExprKind::For(binding, iter, _) => format!("for {} in {} {{ ... }}", binding.name, iter.pretty_print()),
            ExprKind::Block(_) => "{ ... }".to_string(),
            ExprKind::Assign(lhs, rhs) => format!("({} = {})", lhs.pretty_print(), rhs.pretty_print()),
            ExprKind::Ref(is_mut, e) => {
                let mut_str = if *is_mut { "mut " } else { "" };
                format!("(&{}{})", mut_str, e.pretty_print())
            }
            ExprKind::Deref(e) => format!("(*{})", e.pretty_print()),
            ExprKind::Match(expr, arms) => {
                format!("match {} {{ {} arms }}", expr.pretty_print(), arms.len())
            }
            ExprKind::Index(e, idx) => format!("{}[{}]", e.pretty_print(), idx.pretty_print()),
            ExprKind::ArrayLit(elements) => {
                let elems_str = elements.iter().map(|e| e.pretty_print()).collect::<Vec<_>>().join(", ");
                format!("[{}]", elems_str)
            }
            ExprKind::Lambda(params, body) => {
                let params_str = params.iter()
                    .map(|p| {
                        if let Some(ty) = &p.ty {
                            format!("{}: {}", p.name.name, ty.pretty_print())
                        } else {
                            p.name.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({}) -> {}", params_str, body.pretty_print())
            }
            ExprKind::Cast(expr, ty) => {
                format!("{} as {}", expr.pretty_print(), ty.pretty_print())
            }
            ExprKind::StringInterp(parts) => {
                let mut out = String::from("\"");
                for part in parts {
                    match part {
                        StringInterpPart::Literal(s) => out.push_str(s),
                        StringInterpPart::Expr(e) => {
                            out.push('{');
                            out.push_str(&e.pretty_print());
                            out.push('}');
                        }
                    }
                }
                out.push('"');
                out
            }
        }
    }
}

impl TypeExpr {
    pub fn pretty_print(&self) -> String {
        match &self.kind {
            TypeKind::Named(id, type_args) => {
                if type_args.is_empty() {
                    id.name.clone()
                } else {
                    let args_str = type_args.iter().map(|t| t.pretty_print()).collect::<Vec<_>>().join(", ");
                    format!("{}<{}>", id.name, args_str)
                }
            }
            TypeKind::Ref(is_mut, inner) => {
                let mut_str = if *is_mut { "mut " } else { "" };
                format!("&{}{}", mut_str, inner.pretty_print())
            }
            TypeKind::Slice(elem) => format!("&[{}]", elem.pretty_print()),
            TypeKind::Array(elem, size) => {
                format!("[{}; {}]", elem.pretty_print(), size.pretty_print())
            }
            TypeKind::Tuple(elems) => {
                let elems_str = elems.iter().map(|e| e.pretty_print()).collect::<Vec<_>>().join(", ");
                format!("({})", elems_str)
            }
            TypeKind::Unit => "()".to_string(),
        }
    }
}

impl Pattern {
    pub fn pretty_print(&self) -> String {
        match &self.kind {
            PatternKind::Wildcard => "_".to_string(),
            PatternKind::Ident(id) => id.name.clone(),
            PatternKind::Literal(e) => e.pretty_print(),
            PatternKind::Variant(name, fields) => {
                if fields.is_empty() {
                    name.name.clone()
                } else {
                    let fields_str = fields.iter().map(|p| p.pretty_print()).collect::<Vec<_>>().join(", ");
                    format!("{}({})", name.name, fields_str)
                }
            }
        }
    }
}
