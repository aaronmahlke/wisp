//! Name resolution - resolves all identifiers to DefIds

use std::collections::HashMap;
use wisp_ast::*;
use wisp_lexer::Span;
use crate::hir::*;

/// Errors during name resolution
#[derive(Debug, Clone)]
pub struct ResolveError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}..{}", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for ResolveError {}

/// Scope for name resolution
#[derive(Debug, Clone)]
struct Scope {
    /// Names defined in this scope
    names: HashMap<String, DefId>,
    /// Parent scope
    parent: Option<Box<Scope>>,
}

impl Scope {
    fn new() -> Self {
        Self {
            names: HashMap::new(),
            parent: None,
        }
    }

    fn with_parent(parent: Scope) -> Self {
        Self {
            names: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    fn define(&mut self, name: String, def_id: DefId) {
        self.names.insert(name, def_id);
    }

    fn lookup(&self, name: &str) -> Option<DefId> {
        if let Some(id) = self.names.get(name) {
            Some(*id)
        } else if let Some(parent) = &self.parent {
            parent.lookup(name)
        } else {
            None
        }
    }

    fn into_parent(self) -> Option<Scope> {
        self.parent.map(|b| *b)
    }
}

/// Name resolver
pub struct Resolver {
    /// Next DefId to assign
    next_id: u32,
    /// Current scope
    scope: Scope,
    /// All definitions
    defs: HashMap<DefId, DefInfo>,
    /// Global names (types, functions)
    globals: HashMap<String, DefId>,
    /// Errors encountered
    errors: Vec<ResolveError>,
    /// Current function's locals
    current_locals: Vec<DefId>,
    /// Self type in current impl block
    self_type: Option<DefId>,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            scope: Scope::new(),
            defs: HashMap::new(),
            globals: HashMap::new(),
            errors: Vec::new(),
            current_locals: Vec::new(),
            self_type: None,
        }
    }

    /// Resolve a source file to HIR
    pub fn resolve(source: &SourceFile) -> Result<ResolvedProgram, Vec<ResolveError>> {
        let mut resolver = Resolver::new();
        let program = resolver.resolve_source_file(source);
        
        if resolver.errors.is_empty() {
            Ok(program)
        } else {
            Err(resolver.errors)
        }
    }

    fn fresh_id(&mut self) -> DefId {
        let id = DefId::new(self.next_id);
        self.next_id += 1;
        id
    }

    fn define(&mut self, name: String, kind: DefKind, span: Span, parent: Option<DefId>) -> DefId {
        let id = self.fresh_id();
        let info = DefInfo {
            id,
            name: name.clone(),
            kind,
            span,
            parent,
        };
        self.defs.insert(id, info);
        self.scope.define(name, id);
        id
    }

    fn define_global(&mut self, name: String, kind: DefKind, span: Span) -> DefId {
        let id = self.fresh_id();
        let info = DefInfo {
            id,
            name: name.clone(),
            kind,
            span,
            parent: None,
        };
        self.defs.insert(id, info);
        self.globals.insert(name.clone(), id);
        self.scope.define(name, id);
        id
    }

    fn lookup(&self, name: &str) -> Option<DefId> {
        self.scope.lookup(name)
    }

    fn error(&mut self, message: String, span: Span) {
        self.errors.push(ResolveError { message, span });
    }

    fn push_scope(&mut self) {
        let old_scope = std::mem::replace(&mut self.scope, Scope::new());
        self.scope = Scope::with_parent(old_scope);
    }

    fn pop_scope(&mut self) {
        if let Some(parent) = self.scope.clone().into_parent() {
            self.scope = parent;
        }
    }

    fn resolve_source_file(&mut self, source: &SourceFile) -> ResolvedProgram {
        let mut program = ResolvedProgram::new();

        // First pass: collect all type and function names
        for item in &source.items {
            match item {
                Item::Import(_) => {
                    // Imports are handled by the driver before resolution
                }
                Item::Struct(s) => {
                    self.define_global(s.name.name.clone(), DefKind::Struct, s.span);
                }
                Item::Enum(e) => {
                    self.define_global(e.name.name.clone(), DefKind::Enum, e.span);
                }
                Item::Trait(t) => {
                    self.define_global(t.name.name.clone(), DefKind::Trait, t.span);
                }
                Item::Function(f) => {
                    self.define_global(f.name.name.clone(), DefKind::Function, f.span);
                }
                Item::ExternFunction(f) => {
                    self.define_global(f.name.name.clone(), DefKind::ExternFunction, f.span);
                }
                Item::ExternStatic(s) => {
                    self.define_global(s.name.name.clone(), DefKind::ExternStatic, s.span);
                }
                Item::Impl(_) => {
                    // Impl blocks don't define a global name
                }
            }
        }

        // Second pass: resolve all items
        for item in &source.items {
            match item {
                Item::Import(_) => {
                    // Imports are handled by the driver before resolution
                }
                Item::Struct(s) => {
                    if let Some(resolved) = self.resolve_struct(s) {
                        program.structs.push(resolved);
                    }
                }
                Item::Enum(e) => {
                    if let Some(resolved) = self.resolve_enum(e) {
                        program.enums.push(resolved);
                    }
                }
                Item::Trait(t) => {
                    if let Some(resolved) = self.resolve_trait(t) {
                        program.traits.push(resolved);
                    }
                }
                Item::Function(f) => {
                    if let Some(resolved) = self.resolve_function(f, None) {
                        program.functions.push(resolved);
                    }
                }
                Item::ExternFunction(f) => {
                    if let Some(resolved) = self.resolve_extern_function(f) {
                        program.extern_functions.push(resolved);
                    }
                }
                Item::ExternStatic(s) => {
                    if let Some(resolved) = self.resolve_extern_static(s) {
                        program.extern_statics.push(resolved);
                    }
                }
                Item::Impl(i) => {
                    if let Some(resolved) = self.resolve_impl(i) {
                        program.impls.push(resolved);
                    }
                }
            }
        }

        program.defs = self.defs.clone();
        program.globals = self.globals.clone();
        program
    }

    fn resolve_struct(&mut self, s: &StructDef) -> Option<ResolvedStruct> {
        let def_id = self.globals.get(&s.name.name).copied()?;
        
        // Create a scope for type parameters
        self.push_scope();
        
        // Add type parameters to scope
        for type_param in &s.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: type_param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: type_param.span,
                parent: Some(def_id),
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
        }
        
        let mut fields = Vec::new();
        for field in &s.fields {
            let field_id = self.fresh_id();
            let field_info = DefInfo {
                id: field_id,
                name: field.name.name.clone(),
                kind: DefKind::Field,
                span: field.span,
                parent: Some(def_id),
            };
            self.defs.insert(field_id, field_info);
            
            let ty = self.resolve_type(&field.ty);
            fields.push(ResolvedField {
                def_id: field_id,
                name: field.name.name.clone(),
                ty,
                span: field.span,
            });
        }
        
        self.pop_scope();

        Some(ResolvedStruct {
            def_id,
            name: s.name.name.clone(),
            fields,
            span: s.span,
        })
    }

    fn resolve_enum(&mut self, e: &EnumDef) -> Option<ResolvedEnum> {
        let def_id = self.globals.get(&e.name.name).copied()?;
        
        let mut variants = Vec::new();
        for variant in &e.variants {
            let variant_id = self.fresh_id();
            let variant_info = DefInfo {
                id: variant_id,
                name: variant.name.name.clone(),
                kind: DefKind::EnumVariant,
                span: variant.span,
                parent: Some(def_id),
            };
            self.defs.insert(variant_id, variant_info);
            
            // Also add variant to global scope for pattern matching
            self.scope.define(variant.name.name.clone(), variant_id);
            
            let mut fields = Vec::new();
            for field in &variant.fields {
                let field_id = self.fresh_id();
                let field_info = DefInfo {
                    id: field_id,
                    name: field.name.name.clone(),
                    kind: DefKind::Field,
                    span: field.span,
                    parent: Some(variant_id),
                };
                self.defs.insert(field_id, field_info);
                
                let ty = self.resolve_type(&field.ty);
                fields.push(ResolvedField {
                    def_id: field_id,
                    name: field.name.name.clone(),
                    ty,
                    span: field.span,
                });
            }
            
            variants.push(ResolvedVariant {
                def_id: variant_id,
                name: variant.name.name.clone(),
                fields,
                span: variant.span,
            });
        }

        Some(ResolvedEnum {
            def_id,
            name: e.name.name.clone(),
            variants,
            span: e.span,
        })
    }

    fn resolve_trait(&mut self, t: &TraitDef) -> Option<ResolvedTrait> {
        let def_id = self.globals.get(&t.name.name).copied()?;
        
        self.push_scope();
        
        let mut methods = Vec::new();
        for method in &t.methods {
            if let Some(resolved) = self.resolve_function(method, Some(def_id)) {
                methods.push(resolved);
            }
        }
        
        self.pop_scope();

        Some(ResolvedTrait {
            def_id,
            name: t.name.name.clone(),
            methods,
            span: t.span,
        })
    }

    fn resolve_impl(&mut self, i: &ImplBlock) -> Option<ResolvedImpl> {
        let trait_def = i.trait_name.as_ref().and_then(|name| {
            self.lookup(&name.name).or_else(|| {
                self.error(format!("undefined trait '{}'", name.name), name.span);
                None
            })
        });
        
        // Resolve the target type and set self_type
        let target_type = self.resolve_type(&i.target_type);
        
        // Get the DefId for Self type if it's a named type
        let impl_target_id = if let ResolvedType::Named { def_id: Some(id), .. } = &target_type {
            self.self_type = Some(*id);
            Some(*id)
        } else {
            // For primitive types, we still need to mark methods as methods
            // We use a sentinel value to indicate "this is a method but for a primitive"
            None
        };
        
        // For primitive types, we still need methods to get their own DefIds
        // We pass true to indicate this is an impl block context
        let is_impl_context = true;
        
        self.push_scope();
        
        let mut methods = Vec::new();
        for method in &i.methods {
            // Pass the impl target as parent so methods get their own DefId
            // For primitives (impl_target_id = None), we still want to create methods
            if let Some(resolved) = self.resolve_impl_method(method, impl_target_id, is_impl_context) {
                methods.push(resolved);
            }
        }
        
        self.pop_scope();
        self.self_type = None;

        Some(ResolvedImpl {
            trait_def,
            target_type: target_type.clone(),
            methods,
            span: i.span,
        })
    }

    /// Resolve a method inside an impl block (always creates a new DefId)
    fn resolve_impl_method(&mut self, f: &FnDef, parent: Option<DefId>, _is_impl_context: bool) -> Option<ResolvedFunction> {
        // Always create a new DefId for impl methods (even for primitives)
        let def_id = {
            let id = self.fresh_id();
            let info = DefInfo {
                id,
                name: f.name.name.clone(),
                kind: DefKind::Method,
                span: f.span,
                parent,
            };
            self.defs.insert(id, info);
            id
        };
        
        self.push_scope();
        self.current_locals.clear();
        
        // Add type parameters to scope and collect them
        let mut type_params = Vec::new();
        for type_param in &f.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: type_param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: type_param.span,
                parent: Some(def_id),
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
            
            // Resolve bounds
            let mut bounds = Vec::new();
            for bound in &type_param.bounds {
                let bound_type = self.resolve_type(bound);
                bounds.push(bound_type);
            }
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: type_param.name.name.clone(),
                bounds,
                span: type_param.span,
            });
        }
        
        // Resolve parameters
        let mut params = Vec::new();
        for p in &f.params {
            let param_def_id = self.fresh_id();
            let info = DefInfo {
                id: param_def_id,
                name: p.name.name.clone(),
                kind: DefKind::Local,
                span: p.name.span,
                parent: Some(def_id),
            };
            self.defs.insert(param_def_id, info.clone());
            self.scope.define(p.name.name.clone(), param_def_id);
            self.current_locals.push(param_def_id);
            
            params.push(ResolvedParam {
                def_id: param_def_id,
                name: p.name.name.clone(),
                ty: self.resolve_type(&p.ty),
                is_mut: p.is_mut,
                span: p.span,
            });
        }
        
        let return_type = f.return_type.as_ref()
            .map(|t| self.resolve_type(t));
        
        let body = f.body.as_ref().map(|b| self.resolve_block(b));
        
        let locals = self.current_locals.clone();
        
        self.pop_scope();
        
        Some(ResolvedFunction {
            def_id,
            name: f.name.name.clone(),
            type_params,
            params,
            return_type,
            body,
            locals,
            span: f.span,
        })
    }

    fn resolve_function(&mut self, f: &FnDef, parent: Option<DefId>) -> Option<ResolvedFunction> {
        let def_id = if parent.is_some() {
            // Method - create new DefId
            let id = self.fresh_id();
            let info = DefInfo {
                id,
                name: f.name.name.clone(),
                kind: DefKind::Method,
                span: f.span,
                parent,
            };
            self.defs.insert(id, info);
            id
        } else {
            // Free function - already defined
            self.globals.get(&f.name.name).copied()?
        };
        
        self.push_scope();
        self.current_locals.clear();
        
        // Add type parameters to scope and collect them
        let mut type_params = Vec::new();
        for type_param in &f.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: type_param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: type_param.span,
                parent: Some(def_id),
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
            
            // Resolve bounds
            let bounds: Vec<_> = type_param.bounds.iter()
                .map(|b| self.resolve_type(b))
                .collect();
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: type_param.name.name.clone(),
                bounds,
                span: type_param.span,
            });
        }
        
        // Resolve parameters
        let mut params = Vec::new();
        for param in &f.params {
            let param_id = self.define(
                param.name.name.clone(),
                DefKind::Parameter,
                param.span,
                Some(def_id),
            );
            
            let ty = self.resolve_type(&param.ty);
            params.push(ResolvedParam {
                def_id: param_id,
                name: param.name.name.clone(),
                is_mut: param.is_mut,
                ty,
                span: param.span,
            });
        }
        
        // Resolve return type
        let return_type = f.return_type.as_ref().map(|t| self.resolve_type(t));
        
        // Resolve body
        let body = f.body.as_ref().map(|b| self.resolve_block(b));
        
        let locals = std::mem::take(&mut self.current_locals);
        
        self.pop_scope();

        Some(ResolvedFunction {
            def_id,
            name: f.name.name.clone(),
            type_params,
            params,
            return_type,
            body,
            locals,
            span: f.span,
        })
    }

    fn resolve_extern_function(&mut self, f: &ExternFnDef) -> Option<ResolvedExternFunction> {
        let def_id = self.globals.get(&f.name.name).copied()?;
        
        // Resolve parameters (no scope needed - no body)
        let mut params = Vec::new();
        for param in &f.params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: param.name.name.clone(),
                kind: DefKind::Parameter,
                span: param.span,
                parent: Some(def_id),
            };
            self.defs.insert(param_id, param_info);
            
            let ty = self.resolve_type(&param.ty);
            params.push(ResolvedParam {
                def_id: param_id,
                name: param.name.name.clone(),
                is_mut: param.is_mut,
                ty,
                span: param.span,
            });
        }
        
        // Resolve return type
        let return_type = f.return_type.as_ref().map(|t| self.resolve_type(t));
        
        Some(ResolvedExternFunction {
            def_id,
            name: f.name.name.clone(),
            params,
            return_type,
            span: f.span,
        })
    }

    fn resolve_extern_static(&mut self, s: &ExternStaticDef) -> Option<ResolvedExternStatic> {
        let def_id = self.globals.get(&s.name.name).copied()?;
        let ty = self.resolve_type(&s.ty);
        
        Some(ResolvedExternStatic {
            def_id,
            name: s.name.name.clone(),
            ty,
            span: s.span,
        })
    }

    fn resolve_type(&mut self, ty: &TypeExpr) -> ResolvedType {
        match &ty.kind {
            TypeKind::Named(ident, type_args) => {
                let name = &ident.name;
                
                // Resolve type arguments
                let resolved_args: Vec<_> = type_args.iter()
                    .map(|arg| self.resolve_type(arg))
                    .collect();
                
                // Check for Self
                if name == "Self" {
                    return ResolvedType::SelfType;
                }
                
                // Look up user-defined type first (allows shadowing primitives like String)
                if let Some(def_id) = self.lookup(name) {
                    return ResolvedType::Named {
                        name: name.clone(),
                        def_id: Some(def_id),
                        type_args: resolved_args,
                    };
                }
                
                // Fall back to primitives
                if is_primitive(name) {
                    return ResolvedType::Named {
                        name: name.clone(),
                        def_id: None,
                        type_args: resolved_args,
                    };
                }
                
                // Unknown type
                self.error(format!("undefined type '{}'", name), ident.span);
                ResolvedType::Error
            }
            TypeKind::Ref(is_mut, inner) => {
                let inner_resolved = self.resolve_type(inner);
                ResolvedType::Ref {
                    is_mut: *is_mut,
                    inner: Box::new(inner_resolved),
                }
            }
            TypeKind::Slice(elem) => {
                let elem_resolved = self.resolve_type(elem);
                ResolvedType::Slice {
                    elem: Box::new(elem_resolved),
                }
            }
            TypeKind::Unit => ResolvedType::Unit,
            TypeKind::Array(_, _) | TypeKind::Tuple(_) => {
                // TODO: implement these
                ResolvedType::Error
            }
        }
    }

    fn resolve_block(&mut self, block: &Block) -> ResolvedBlock {
        self.push_scope();
        
        let stmts: Vec<_> = block.stmts.iter()
            .map(|s| self.resolve_stmt(s))
            .collect();
        
        self.pop_scope();
        
        ResolvedBlock {
            stmts,
            span: block.span,
        }
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) -> ResolvedStmt {
        match stmt {
            Stmt::Let(l) => {
                // Resolve initializer first (before the binding is in scope)
                let init = l.init.as_ref().map(|e| self.resolve_expr(e));
                let ty = l.ty.as_ref().map(|t| self.resolve_type(t));
                
                // Now define the binding
                let def_id = self.define(
                    l.name.name.clone(),
                    DefKind::Local,
                    l.span,
                    None,
                );
                self.current_locals.push(def_id);
                
                ResolvedStmt::Let {
                    def_id,
                    name: l.name.name.clone(),
                    is_mut: l.is_mut,
                    ty,
                    init,
                    span: l.span,
                }
            }
            Stmt::Expr(e) => {
                ResolvedStmt::Expr(self.resolve_expr(&e.expr))
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr) -> ResolvedExpr {
        let kind = match &expr.kind {
            ExprKind::IntLiteral(n) => ResolvedExprKind::IntLiteral(*n),
            ExprKind::FloatLiteral(n) => ResolvedExprKind::FloatLiteral(*n),
            ExprKind::BoolLiteral(b) => ResolvedExprKind::BoolLiteral(*b),
            ExprKind::StringLiteral(s) => ResolvedExprKind::StringLiteral(s.clone()),
            
            ExprKind::Ident(ident) => {
                match self.lookup(&ident.name) {
                    Some(def_id) => ResolvedExprKind::Var {
                        name: ident.name.clone(),
                        def_id,
                    },
                    None => {
                        self.error(format!("undefined variable '{}'", ident.name), ident.span);
                        ResolvedExprKind::Error
                    }
                }
            }
            
            ExprKind::Binary(left, op, right) => {
                ResolvedExprKind::Binary {
                    left: Box::new(self.resolve_expr(left)),
                    op: *op,
                    right: Box::new(self.resolve_expr(right)),
                }
            }
            
            ExprKind::Unary(op, inner) => {
                ResolvedExprKind::Unary {
                    op: *op,
                    expr: Box::new(self.resolve_expr(inner)),
                }
            }
            
            ExprKind::Call(callee, args) => {
                ResolvedExprKind::Call {
                    callee: Box::new(self.resolve_expr(callee)),
                    args: args.iter().map(|a| ResolvedCallArg {
                        name: a.name.as_ref().map(|n| n.name.clone()),
                        value: self.resolve_expr(&a.value),
                        span: a.span,
                    }).collect(),
                }
            }
            
            ExprKind::Field(base, field) => {
                ResolvedExprKind::Field {
                    expr: Box::new(self.resolve_expr(base)),
                    field: field.name.clone(),
                    field_def: None, // Resolved during type checking
                }
            }
            
            ExprKind::StructLit(name, fields) => {
                match self.lookup(&name.name) {
                    Some(struct_def) => {
                        let resolved_fields: Vec<_> = fields.iter()
                            .map(|f| (f.name.name.clone(), self.resolve_expr(&f.value)))
                            .collect();
                        ResolvedExprKind::StructLit {
                            struct_def,
                            fields: resolved_fields,
                        }
                    }
                    None => {
                        self.error(format!("undefined struct '{}'", name.name), name.span);
                        ResolvedExprKind::Error
                    }
                }
            }
            
            ExprKind::If(cond, then_block, else_branch) => {
                let resolved_else = else_branch.as_ref().map(|eb| match eb {
                    ElseBranch::Block(b) => ResolvedElse::Block(self.resolve_block(b)),
                    ElseBranch::If(e) => ResolvedElse::If(Box::new(self.resolve_expr(e))),
                });
                
                ResolvedExprKind::If {
                    cond: Box::new(self.resolve_expr(cond)),
                    then_block: self.resolve_block(then_block),
                    else_block: resolved_else,
                }
            }
            
            ExprKind::While(cond, body) => {
                ResolvedExprKind::While {
                    cond: Box::new(self.resolve_expr(cond)),
                    body: self.resolve_block(body),
                }
            }
            
            ExprKind::Block(block) => {
                ResolvedExprKind::Block(self.resolve_block(block))
            }
            
            ExprKind::Assign(target, value) => {
                ResolvedExprKind::Assign {
                    target: Box::new(self.resolve_expr(target)),
                    value: Box::new(self.resolve_expr(value)),
                }
            }
            
            ExprKind::Ref(is_mut, inner) => {
                ResolvedExprKind::Ref {
                    is_mut: *is_mut,
                    expr: Box::new(self.resolve_expr(inner)),
                }
            }
            
            ExprKind::Deref(inner) => {
                ResolvedExprKind::Deref(Box::new(self.resolve_expr(inner)))
            }
            
            ExprKind::Match(scrutinee, arms) => {
                ResolvedExprKind::Match {
                    scrutinee: Box::new(self.resolve_expr(scrutinee)),
                    arms: arms.iter().map(|a| self.resolve_match_arm(a)).collect(),
                }
            }
            
            ExprKind::Index(base, index) => {
                ResolvedExprKind::Index {
                    expr: Box::new(self.resolve_expr(base)),
                    index: Box::new(self.resolve_expr(index)),
                }
            }
        };
        
        ResolvedExpr {
            kind,
            span: expr.span,
        }
    }

    fn resolve_match_arm(&mut self, arm: &MatchArm) -> ResolvedMatchArm {
        self.push_scope();
        
        let pattern = self.resolve_pattern(&arm.pattern);
        let body = self.resolve_expr(&arm.body);
        
        self.pop_scope();
        
        ResolvedMatchArm {
            pattern,
            body,
            span: arm.span,
        }
    }

    fn resolve_pattern(&mut self, pattern: &Pattern) -> ResolvedPattern {
        let kind = match &pattern.kind {
            PatternKind::Wildcard => ResolvedPatternKind::Wildcard,
            
            PatternKind::Ident(ident) => {
                // Check if this is a variant name or a binding
                if let Some(def_id) = self.lookup(&ident.name) {
                    let def = self.defs.get(&def_id);
                    if matches!(def.map(|d| &d.kind), Some(DefKind::EnumVariant)) {
                        return ResolvedPattern {
                            kind: ResolvedPatternKind::Variant {
                                variant_def: def_id,
                                fields: Vec::new(),
                            },
                            span: pattern.span,
                        };
                    }
                }
                
                // It's a binding
                let def_id = self.define(
                    ident.name.clone(),
                    DefKind::Local,
                    ident.span,
                    None,
                );
                self.current_locals.push(def_id);
                
                ResolvedPatternKind::Binding {
                    def_id,
                    name: ident.name.clone(),
                }
            }
            
            PatternKind::Literal(expr) => {
                ResolvedPatternKind::Literal(self.resolve_expr(expr))
            }
            
            PatternKind::Variant(name, fields) => {
                match self.lookup(&name.name) {
                    Some(variant_def) => {
                        let resolved_fields: Vec<_> = fields.iter()
                            .map(|p| self.resolve_pattern(p))
                            .collect();
                        ResolvedPatternKind::Variant {
                            variant_def,
                            fields: resolved_fields,
                        }
                    }
                    None => {
                        self.error(format!("undefined variant '{}'", name.name), name.span);
                        ResolvedPatternKind::Wildcard
                    }
                }
            }
        };
        
        ResolvedPattern {
            kind,
            span: pattern.span,
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a type name is a primitive
fn is_primitive(name: &str) -> bool {
    matches!(name, 
        "i8" | "i16" | "i32" | "i64" | "i128" |
        "u8" | "u16" | "u32" | "u64" | "u128" |
        "f32" | "f64" |
        "bool" | "char" | "str"
    )
}

