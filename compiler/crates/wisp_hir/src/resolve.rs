//! Name resolution - resolves all identifiers to DefIds

use std::collections::{HashMap, HashSet};
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

/// Namespace - a collection of names from an import
#[derive(Debug, Clone, Default)]
pub struct Namespace {
    /// Names in this namespace
    pub names: HashMap<String, DefId>,
    /// Nested namespaces (for `import std` -> `std.io.print`)
    pub children: HashMap<String, Namespace>,
}

impl Namespace {
    fn new() -> Self {
        Self { 
            names: HashMap::new(),
            children: HashMap::new(),
        }
    }
    
    fn define(&mut self, name: String, def_id: DefId) {
        self.names.insert(name, def_id);
    }
    
    fn lookup(&self, name: &str) -> Option<DefId> {
        self.names.get(name).copied()
    }
    
    /// Convert to NamespaceData for the resolved program
    fn to_namespace_data(&self) -> crate::hir::NamespaceData {
        crate::hir::NamespaceData {
            items: self.names.clone(),
            children: self.children.iter()
                .map(|(k, v)| (k.clone(), v.to_namespace_data()))
                .collect(),
        }
    }
}

/// Name resolver
pub struct Resolver {
    /// Next DefId to assign
    next_id: u32,
    /// Current scope (for local variables within functions)
    scope: Scope,
    /// All definitions
    defs: HashMap<DefId, DefInfo>,
    /// Global names (types, functions) - shared across all modules
    globals: HashMap<String, DefId>,
    /// Namespaces from imports: namespace_name -> Namespace (only public items)
    namespaces: HashMap<String, Namespace>,
    /// Top-level accessible namespaces (not transitive imports)
    /// Only these can be accessed directly (e.g., `io.print` if `import std/io`)
    accessible_namespaces: HashSet<String>,
    /// Per-module scopes: ModuleId -> Scope (all items in that module, public or private)
    module_scopes: HashMap<ModuleId, Scope>,
    /// Errors encountered
    errors: Vec<ResolveError>,
    /// Current function's locals
    current_locals: Vec<DefId>,
    /// Self type in current impl block
    self_type: Option<DefId>,
    /// Trait type parameters with defaults: trait DefId -> [(param name, default type if any)]
    trait_type_params: HashMap<DefId, Vec<(String, Option<TypeExpr>)>>,
    /// Current namespace being populated (when processing an import's items)
    current_namespace: Option<String>,
    /// Items that have already been resolved (by span start..end)
    resolved_items: HashSet<(usize, usize)>,
    /// Module registry
    modules: ModuleRegistry,
    /// Current module being resolved
    current_module: ModuleId,
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            next_id: 0,
            scope: Scope::new(),
            defs: HashMap::new(),
            globals: HashMap::new(),
            namespaces: HashMap::new(),
            accessible_namespaces: HashSet::new(),
            module_scopes: HashMap::new(),
            errors: Vec::new(),
            current_locals: Vec::new(),
            self_type: None,
            trait_type_params: HashMap::new(),
            current_namespace: None,
            resolved_items: HashSet::new(),
            modules: ModuleRegistry::new(),
            current_module: ModuleId::root(),
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
    
    /// Resolve a source file with imports to HIR, preserving namespace information
    pub fn resolve_with_imports(source: &SourceFileWithImports) -> Result<ResolvedProgram, Vec<ResolveError>> {
        let mut resolver = Resolver::new();
        let program = resolver.resolve_source_file_with_imports(source);
        
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

    fn define(&mut self, name: String, kind: DefKind, span: Span, parent: Option<DefId>, is_pub: bool) -> DefId {
        let id = self.fresh_id();
        let info = DefInfo {
            id,
            name: name.clone(),
            kind,
            span,
            parent,
            module_id: self.current_module,
            is_pub,
        };
        self.defs.insert(id, info);
        self.scope.define(name, id);
        id
    }

    fn define_global(&mut self, name: String, kind: DefKind, span: Span, is_pub: bool) -> DefId {
        let id = self.fresh_id();
        let info = DefInfo {
            id,
            name: name.clone(),
            kind,
            span,
            parent: None,
            module_id: self.current_module,
            is_pub,
        };
        self.defs.insert(id, info);
        self.globals.insert(name.clone(), id);
        
        // Track in module registry
        self.modules.add_def(self.current_module, id);
        
        // Add to the current module's scope (all items in a module can see each other)
        self.module_scopes
            .entry(self.current_module)
            .or_insert_with(Scope::new)
            .define(name.clone(), id);
        
        // Also add to namespace if we're processing an import (only public items)
        if let Some(ref ns_name) = self.current_namespace {
            if is_pub {
                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                    ns.define(name, id);
                }
            }
        }
        
        id
    }
    
    /// Lookup a name in a namespace (only returns public items)
    fn lookup_in_namespace(&self, namespace: &str, name: &str) -> Option<DefId> {
        self.namespaces.get(namespace).and_then(|ns| ns.lookup(name))
    }
    
    /// Lookup a name in a namespace, with visibility checking
    /// Returns (DefId, is_public) or None if not found
    fn lookup_in_namespace_with_visibility(&self, namespace: &str, name: &str) -> Option<(DefId, bool)> {
        self.namespaces.get(namespace).and_then(|ns| {
            ns.lookup(name).map(|def_id| {
                let is_pub = self.defs.get(&def_id)
                    .map(|info| info.is_pub)
                    .unwrap_or(false);
                (def_id, is_pub)
            })
        })
    }
    
    /// Check if accessing a def from another module is allowed
    fn check_visibility(&mut self, def_id: DefId, access_span: Span) -> bool {
        if let Some(def_info) = self.defs.get(&def_id) {
            // Same module - always allowed
            if def_info.module_id == self.current_module {
                return true;
            }
            // Different module - check if public
            if !def_info.is_pub {
                self.error(
                    format!("'{}' is private", def_info.name),
                    access_span,
                );
                return false;
            }
        }
        true
    }
    
    /// Check if a name is an accessible namespace (not a transitive import)
    fn is_namespace(&self, name: &str) -> bool {
        self.accessible_namespaces.contains(name)
    }
    
    /// Collect namespace path from a field access chain (e.g., std.io -> ["std", "io"])
    fn collect_namespace_path(&self, expr: &Expr) -> Option<Vec<String>> {
        match &expr.kind {
            ExprKind::Ident(ident) => {
                if self.is_namespace(&ident.name) {
                    Some(vec![ident.name.clone()])
                } else {
                    None
                }
            }
            ExprKind::Field(base, field) => {
                // Recursively collect path from base
                if let Some(mut path) = self.collect_namespace_path(base) {
                    // Check if this field is a child namespace
                    if let Some(ns) = self.lookup_child_namespace(&path, &field.name) {
                        eprintln!("DEBUG collect_namespace_path: path={:?}, field={}, ns.children.keys={:?}, ns.names.keys={:?}", 
                            path, field.name, ns.children.keys().collect::<Vec<_>>(), ns.names.keys().collect::<Vec<_>>());
                        if ns.children.contains_key(&field.name) || !ns.names.is_empty() {
                            path.push(field.name.clone());
                            return Some(path);
                        }
                    } else {
                        eprintln!("DEBUG collect_namespace_path: lookup_child_namespace({:?}, {}) returned None", path, field.name);
                    }
                }
                None
            }
            _ => None,
        }
    }
    
    /// Look up a child namespace by path
    fn lookup_child_namespace(&self, path: &[String], _child: &str) -> Option<&Namespace> {
        if path.is_empty() {
            return None;
        }
        
        let mut current = self.namespaces.get(&path[0])?;
        for segment in &path[1..] {
            current = current.children.get(segment)?;
        }
        Some(current)
    }
    
    /// Resolve a namespace access like std.io.print
    fn resolve_namespace_access(&mut self, ns_path: &[String], item_name: &str, field_span: Span, expr_span: Span) -> Option<ResolvedExpr> {
        if ns_path.is_empty() {
            return None;
        }
        
        // Start from the root namespace
        let mut current = self.namespaces.get(&ns_path[0])?;
        
        // Navigate through child namespaces
        for segment in &ns_path[1..] {
            current = current.children.get(segment)?;
        }
        
        // Check if the item exists in this namespace
        if let Some(&def_id) = current.names.get(item_name) {
            // Check visibility
            let is_pub = self.defs.get(&def_id).map(|d| d.is_pub).unwrap_or(false);
            if !is_pub {
                self.error(
                    format!("'{}' is private", item_name),
                    field_span,
                );
            }
            return Some(ResolvedExpr {
                kind: ResolvedExprKind::Var {
                    name: item_name.to_string(),
                    def_id,
                },
                span: expr_span,
            });
        }
        
        // Check if it's a child namespace (for further chaining)
        if current.children.contains_key(item_name) {
            let mut new_path = ns_path.to_vec();
            new_path.push(item_name.to_string());
            return Some(ResolvedExpr {
                kind: ResolvedExprKind::NamespacePath(new_path),
                span: expr_span,
            });
        }
        
        // Item not found in namespace
        let ns_display = ns_path.join(".");
        self.error(
            format!("cannot find '{}' in namespace '{}'", item_name, ns_display),
            field_span,
        );
        Some(ResolvedExpr {
            kind: ResolvedExprKind::Error,
            span: expr_span,
        })
    }
    
    /// Process an import to create its namespace (first pass)
    fn process_import_namespace(&mut self, import: &ImportDecl) {
        // Determine the namespace name
        let ns_name = if let Some(ref alias) = import.alias {
            alias.name.clone()
        } else {
            // Use the last segment of the path
            import.path.last_segment()
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        
        // Create the namespace (even for destructure imports, so we can look up items)
        if !ns_name.is_empty() {
            self.namespaces.entry(ns_name.clone()).or_insert_with(Namespace::new);
            
            // Only mark as accessible for non-destructure imports
            if !import.destructure_only {
                self.accessible_namespaces.insert(ns_name.clone());
            }
            
            // Set current namespace so define_global adds items to it
            self.current_namespace = Some(ns_name);
        }
    }
    
    /// Process import items into scope (second pass)
    fn process_import_items(&mut self, import: &ImportDecl) {
        // Clear current namespace
        self.current_namespace = None;
        
        // If destructure items are specified, import them directly into scope
        if let Some(ref items) = import.items {
            for item in items {
                let name = if let Some(ref alias) = item.alias {
                    &alias.name
                } else {
                    &item.name.name
                };
                
                // Look up the original item in the namespace
                let ns_name = import.path.last_segment().unwrap_or("");
                if let Some((def_id, is_pub)) = self.lookup_in_namespace_with_visibility(ns_name, &item.name.name) {
                    // Check visibility - only public items can be imported
                    if !is_pub {
                        self.error(
                            format!("'{}' is private and cannot be imported", item.name.name),
                            item.span,
                        );
                    }
                    // Import it into the current scope
                    self.scope.define(name.clone(), def_id);
                } else {
                    // Also check globals (for backwards compatibility)
                    if let Some(&def_id) = self.globals.get(&item.name.name) {
                        self.scope.define(name.clone(), def_id);
                    } else {
                        self.error(
                            format!("cannot find '{}' in module", item.name.name),
                            item.span,
                        );
                    }
                }
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<DefId> {
        // First check the local scope (function locals, parameters, type params)
        if let Some(id) = self.scope.lookup(name) {
            return Some(id);
        }
        
        // Then check the current module's scope (all items in this module)
        if let Some(module_scope) = self.module_scopes.get(&self.current_module) {
            if let Some(id) = module_scope.lookup(name) {
                return Some(id);
            }
        }
        
        // Finally check globals (enum variants, etc.)
        if let Some(id) = self.globals.get(name) {
            return Some(*id);
        }
        
        None
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
                Item::Import(import) => {
                    // Create namespace for this import
                    self.process_import_namespace(import);
                }
                Item::Struct(s) => {
                    self.define_global(s.name.name.clone(), DefKind::Struct, s.span, s.is_pub);
                }
                Item::Enum(e) => {
                    let enum_def_id = self.define_global(e.name.name.clone(), DefKind::Enum, e.span, e.is_pub);
                    
                    // Also define variants in globals and namespace
                    for variant in &e.variants {
                        let variant_id = self.fresh_id();
                        let variant_info = DefInfo {
                            id: variant_id,
                            name: variant.name.name.clone(),
                            kind: DefKind::EnumVariant,
                            span: variant.span,
                            parent: Some(enum_def_id),
                            module_id: self.current_module,
                            is_pub: e.is_pub,
                        };
                        self.defs.insert(variant_id, variant_info);
                        self.globals.insert(variant.name.name.clone(), variant_id);
                        
                        // Add to namespace if we're processing an import
                        if let Some(ref ns_name) = self.current_namespace {
                            if e.is_pub {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(variant.name.name.clone(), variant_id);
                                }
                            }
                        }
                    }
                }
                Item::Trait(t) => {
                    self.define_global(t.name.name.clone(), DefKind::Trait, t.span, t.is_pub);
                }
                Item::Function(f) => {
                    self.define_global(f.name.name.clone(), DefKind::Function, f.span, f.is_pub);
                }
                Item::ExternFunction(f) => {
                    self.define_global(f.name.name.clone(), DefKind::ExternFunction, f.span, f.is_pub);
                }
                Item::ExternStatic(s) => {
                    self.define_global(s.name.name.clone(), DefKind::ExternStatic, s.span, s.is_pub);
                }
                Item::Impl(_) => {
                    // Impl blocks don't define a global name
                }
            }
        }

        // Second pass: resolve all items
        for item in &source.items {
            match item {
                Item::Import(import) => {
                    // Import items into scope based on import style
                    self.process_import_items(import);
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
        program.modules = std::mem::take(&mut self.modules);
        // Export namespaces for LSP
        program.namespaces = self.namespaces.iter()
            .filter(|(name, _)| self.accessible_namespaces.contains(*name))
            .map(|(name, ns)| (name.clone(), ns.to_namespace_data()))
            .collect();
        program
    }
    
    /// Resolve a source file with imports, properly setting up namespaces
    fn resolve_source_file_with_imports(&mut self, source: &SourceFileWithImports) -> ResolvedProgram {
        let mut program = ResolvedProgram::new();
        
        // Register the root module (the main file being compiled)
        let root_module = ModuleId::root();
        self.module_scopes.insert(root_module, Scope::new());
        
        // First pass: process imported modules and their items
        // Each imported module gets its own ModuleId
        // We need to process modules in order so that when a module imports another,
        // the imported module's items are already defined.
        let mut next_module_id = 1u32;
        
        // Map from namespace name to module ID (for looking up imports within modules)
        let mut ns_to_module: HashMap<String, ModuleId> = HashMap::new();
        
        for module in &source.imported_modules {
            // Assign a ModuleId to this imported module
            let module_id = ModuleId::new(next_module_id);
            next_module_id += 1;
            self.current_module = module_id;
            self.module_scopes.insert(module_id, Scope::new());
            
            // Determine namespace name
            let ns_name = if let Some(ref alias) = module.import.alias {
                alias.name.clone()
            } else {
                module.import.path.last_segment()
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            
            // Track namespace -> module mapping
            if !ns_name.is_empty() {
                ns_to_module.insert(ns_name.clone(), module_id);
            }
            
            // Create namespace for the module (needed for looking up items during destructure)
            // For destructure-only imports, we create the namespace but don't mark it as accessible
            if !ns_name.is_empty() {
                self.namespaces.entry(ns_name.clone()).or_insert_with(Namespace::new);
                self.current_namespace = Some(ns_name.clone());
                
                // Only mark as accessible if not a transitive import and not destructure-only
                if !module.is_transitive && !module.import.destructure_only {
                    self.accessible_namespaces.insert(ns_name.clone());
                }
            }
            
            // First, process this module's imports to bring imported items into this module's scope
            for import in &module.module_imports {
                // This module imports another module - add its public items to this module's scope
                let imported_ns = import.path.last_segment().unwrap_or("");
                if let Some(ns) = self.namespaces.get(imported_ns) {
                    // Add all items from the imported namespace to this module's scope
                    for (name, &def_id) in &ns.names {
                        // Check if item is public
                        let is_pub = self.defs.get(&def_id).map(|d| d.is_pub).unwrap_or(false);
                        if is_pub {
                            self.module_scopes
                                .get_mut(&module_id)
                                .unwrap()
                                .define(name.clone(), def_id);
                        }
                    }
                    
                    // If this is a `pub import ... as child`, add as child namespace of current namespace
                    // This enables `import std` -> `std.io.print()` when std/mod.ws has `pub import std/io as io`
                    if let Some(ref parent_ns) = self.current_namespace {
                        let child_name = if let Some(ref alias) = import.alias {
                            alias.name.clone()
                        } else {
                            imported_ns.to_string()
                        };
                        
                        // Clone the namespace to add as a child
                        let child_ns = Namespace {
                            names: ns.names.clone(),
                            children: ns.children.clone(),
                        };
                        
                        if let Some(parent) = self.namespaces.get_mut(parent_ns) {
                            parent.children.insert(child_name, child_ns);
                        }
                    }
                }
            }
            
            // Now process this module's own items
            for item in &module.items {
                match item {
                    Item::Import(_) => {
                        // Already handled above
                    }
                    Item::Struct(s) => {
                        // Skip if already defined (from another import of the same module)
                        if !self.globals.contains_key(&s.name.name) {
                            self.define_global(s.name.name.clone(), DefKind::Struct, s.span, s.is_pub);
                        } else if let Some(ref ns_name) = self.current_namespace {
                            // Still add to namespace even if globally defined
                            if let Some(&def_id) = self.globals.get(&s.name.name) {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(s.name.name.clone(), def_id);
                                }
                            }
                        }
                    }
                    Item::Enum(e) => {
                        let enum_def_id = if !self.globals.contains_key(&e.name.name) {
                            self.define_global(e.name.name.clone(), DefKind::Enum, e.span, e.is_pub)
                        } else {
                            if let Some(ref ns_name) = self.current_namespace {
                                if let Some(&def_id) = self.globals.get(&e.name.name) {
                                    if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                        ns.define(e.name.name.clone(), def_id);
                                    }
                                }
                            }
                            *self.globals.get(&e.name.name).unwrap()
                        };
                        
                        // Also define variants in globals and namespace
                        for variant in &e.variants {
                            if !self.globals.contains_key(&variant.name.name) {
                                let variant_id = self.fresh_id();
                                let variant_info = DefInfo {
                                    id: variant_id,
                                    name: variant.name.name.clone(),
                                    kind: DefKind::EnumVariant,
                                    span: variant.span,
                                    parent: Some(enum_def_id),
                                    module_id: self.current_module,
                                    is_pub: e.is_pub, // Variants inherit visibility from enum
                                };
                                self.defs.insert(variant_id, variant_info);
                                self.globals.insert(variant.name.name.clone(), variant_id);
                                
                                // Add to namespace if we're processing an import
                                if let Some(ref ns_name) = self.current_namespace {
                                    if e.is_pub {
                                        if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                            ns.define(variant.name.name.clone(), variant_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Item::Trait(t) => {
                        if !self.globals.contains_key(&t.name.name) {
                            self.define_global(t.name.name.clone(), DefKind::Trait, t.span, t.is_pub);
                        } else if let Some(ref ns_name) = self.current_namespace {
                            if let Some(&def_id) = self.globals.get(&t.name.name) {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(t.name.name.clone(), def_id);
                                }
                            }
                        }
                    }
                    Item::Function(f) => {
                        if !self.globals.contains_key(&f.name.name) {
                            self.define_global(f.name.name.clone(), DefKind::Function, f.span, f.is_pub);
                        } else if let Some(ref ns_name) = self.current_namespace {
                            if let Some(&def_id) = self.globals.get(&f.name.name) {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(f.name.name.clone(), def_id);
                                }
                            }
                        }
                    }
                    Item::ExternFunction(f) => {
                        if !self.globals.contains_key(&f.name.name) {
                            self.define_global(f.name.name.clone(), DefKind::ExternFunction, f.span, f.is_pub);
                        } else if let Some(ref ns_name) = self.current_namespace {
                            if let Some(&def_id) = self.globals.get(&f.name.name) {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(f.name.name.clone(), def_id);
                                }
                            }
                        }
                    }
                    Item::ExternStatic(s) => {
                        if !self.globals.contains_key(&s.name.name) {
                            self.define_global(s.name.name.clone(), DefKind::ExternStatic, s.span, s.is_pub);
                        } else if let Some(ref ns_name) = self.current_namespace {
                            if let Some(&def_id) = self.globals.get(&s.name.name) {
                                if let Some(ns) = self.namespaces.get_mut(ns_name) {
                                    ns.define(s.name.name.clone(), def_id);
                                }
                            }
                        }
                    }
                    Item::Impl(_) => {}
                }
            }
            
            // Clear namespace after processing this module's items
            self.current_namespace = None;
            
            // Handle destructured imports
            if let Some(ref items) = module.import.items {
                for item in items {
                    let name = if let Some(ref alias) = item.alias {
                        &alias.name
                    } else {
                        &item.name.name
                    };
                    
                    // Look up the item in the namespace or globals with visibility check
                    if let Some((def_id, is_pub)) = self.lookup_in_namespace_with_visibility(&ns_name, &item.name.name) {
                        // Check visibility - only public items can be imported
                        if !is_pub {
                            self.error(
                                format!("'{}' is private and cannot be imported", item.name.name),
                                item.span,
                            );
                        }
                        // Add to root module's scope so it's accessible from main file
                        self.module_scopes
                            .entry(ModuleId::root())
                            .or_insert_with(Scope::new)
                            .define(name.clone(), def_id);
                    } else if let Some(&def_id) = self.globals.get(&item.name.name) {
                        self.module_scopes
                            .entry(ModuleId::root())
                            .or_insert_with(Scope::new)
                            .define(name.clone(), def_id);
                    } else {
                        self.error(
                            format!("cannot find '{}' in module", item.name.name),
                            item.span,
                        );
                    }
                }
            }
        }
        
        // Process local items (first pass) - switch back to root module
        self.current_module = ModuleId::root();
        self.current_namespace = None;
        
        for item in &source.local_items {
            match item {
                Item::Import(_) => {}
                Item::Struct(s) => {
                    self.define_global(s.name.name.clone(), DefKind::Struct, s.span, s.is_pub);
                }
                Item::Enum(e) => {
                    self.define_global(e.name.name.clone(), DefKind::Enum, e.span, e.is_pub);
                }
                Item::Trait(t) => {
                    self.define_global(t.name.name.clone(), DefKind::Trait, t.span, t.is_pub);
                }
                Item::Function(f) => {
                    self.define_global(f.name.name.clone(), DefKind::Function, f.span, f.is_pub);
                }
                Item::ExternFunction(f) => {
                    self.define_global(f.name.name.clone(), DefKind::ExternFunction, f.span, f.is_pub);
                }
                Item::ExternStatic(s) => {
                    self.define_global(s.name.name.clone(), DefKind::ExternStatic, s.span, s.is_pub);
                }
                Item::Impl(_) => {}
            }
        }
        
        // Second pass: resolve all items
        // Reset module ID counter to match first pass
        let mut next_module_id = 1u32;
        
        for module in &source.imported_modules {
            // Set current module to match the first pass
            let module_id = ModuleId::new(next_module_id);
            next_module_id += 1;
            self.current_module = module_id;
            
            for item in &module.items {
                self.resolve_item(item, &mut program);
            }
        }
        
        // Switch back to root module for local items
        self.current_module = ModuleId::root();
        
        for item in &source.local_items {
            self.resolve_item(item, &mut program);
        }
        
        program.defs = self.defs.clone();
        program.globals = self.globals.clone();
        program.modules = std::mem::take(&mut self.modules);
        // Export namespaces for LSP
        program.namespaces = self.namespaces.iter()
            .filter(|(name, _)| self.accessible_namespaces.contains(*name))
            .map(|(name, ns)| (name.clone(), ns.to_namespace_data()))
            .collect();
        program
    }
    
    /// Helper to resolve a single item
    /// Get the span key for an item (for deduplication)
    fn item_span_key(item: &Item) -> (usize, usize) {
        match item {
            Item::Import(i) => (i.span.start, i.span.end),
            Item::Function(f) => (f.span.start, f.span.end),
            Item::ExternFunction(f) => (f.span.start, f.span.end),
            Item::ExternStatic(s) => (s.span.start, s.span.end),
            Item::Struct(s) => (s.span.start, s.span.end),
            Item::Enum(e) => (e.span.start, e.span.end),
            Item::Trait(t) => (t.span.start, t.span.end),
            Item::Impl(i) => (i.span.start, i.span.end),
        }
    }
    
    fn resolve_item(&mut self, item: &Item, program: &mut ResolvedProgram) {
        // Skip if already resolved
        let span_key = Self::item_span_key(item);
        if self.resolved_items.contains(&span_key) {
            return;
        }
        self.resolved_items.insert(span_key);
        
        match item {
            Item::Import(_) => {}
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
                module_id: self.current_module,
                is_pub: false,
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
                module_id: self.current_module,
                is_pub: false, // TODO: Support pub fields
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
        
        // Push scope for enum type parameters
        self.push_scope();
        
        // Register enum type parameters in scope
        let mut type_params = Vec::new();
        for param in &e.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: param.span,
                parent: Some(def_id),
                module_id: self.current_module,
                is_pub: false,
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(param.name.name.clone(), param_id);
            
            // Resolve bounds
            let bounds: Vec<_> = param.bounds.iter()
                .map(|b| self.resolve_type(b))
                .collect();
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: param.name.name.clone(),
                bounds,
                default: None,
                span: param.span,
            });
        }
        
        let mut variants = Vec::new();
        for variant in &e.variants {
            // Reuse existing variant DefId if it was registered in first pass
            let variant_id = if let Some(&existing_id) = self.globals.get(&variant.name.name) {
                existing_id
            } else {
                // Create new if not found (shouldn't happen normally)
                let id = self.fresh_id();
                self.globals.insert(variant.name.name.clone(), id);
                id
            };
            
            let variant_info = DefInfo {
                id: variant_id,
                name: variant.name.name.clone(),
                kind: DefKind::EnumVariant,
                span: variant.span,
                parent: Some(def_id),
                module_id: self.current_module,
                is_pub: false, // Variants inherit visibility from enum
            };
            self.defs.insert(variant_id, variant_info);
            
            // Add to current scope for immediate access
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
                    module_id: self.current_module,
                    is_pub: false,
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

        // Pop enum type params scope
        self.pop_scope();
        
        Some(ResolvedEnum {
            def_id,
            name: e.name.name.clone(),
            type_params,
            variants,
            span: e.span,
        })
    }

    fn resolve_trait(&mut self, t: &TraitDef) -> Option<ResolvedTrait> {
        let def_id = self.globals.get(&t.name.name).copied()?;
        
        // Store trait type parameters with defaults for later use in impl blocks
        let type_params_with_defaults: Vec<(String, Option<TypeExpr>)> = t.type_params.iter()
            .map(|tp| (tp.name.name.clone(), tp.default.clone()))
            .collect();
        self.trait_type_params.insert(def_id, type_params_with_defaults);
        
        self.push_scope();
        
        // Define trait type parameters in scope
        for type_param in &t.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: type_param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: type_param.span,
                parent: Some(def_id),
                module_id: self.current_module,
                is_pub: false,
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
        }
        
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
        // Push scope for impl type parameters (e.g., impl<T> Option<T>)
        self.push_scope();
        
        // Register impl type parameters in scope and collect as ResolvedTypeParam
        let mut type_params = Vec::new();
        for param in &i.type_params {
            let param_id = self.fresh_id();
            let param_info = DefInfo {
                id: param_id,
                name: param.name.name.clone(),
                kind: DefKind::TypeParam,
                span: param.span,
                parent: None,
                module_id: self.current_module,
                is_pub: false,
            };
            self.defs.insert(param_id, param_info.clone());
            self.scope.define(param.name.name.clone(), param_id);
            
            // Resolve bounds
            let bounds: Vec<_> = param.bounds.iter()
                .map(|b| self.resolve_type(b))
                .collect();
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: param.name.name.clone(),
                bounds,
                default: None,
                span: param.span,
            });
        }
        
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
        
        // Resolve trait type arguments, applying defaults where needed
        let trait_type_args = if let Some(trait_id) = trait_def {
            // Get the trait's type parameters with defaults
            let trait_params = self.trait_type_params.get(&trait_id).cloned().unwrap_or_default();
            
            // Start with explicitly provided type args
            let mut resolved_args: Vec<ResolvedType> = i.trait_type_args.iter()
                .map(|t| self.resolve_type(t))
                .collect();
            
            // Fill in defaults for missing type args
            for idx in resolved_args.len()..trait_params.len() {
                if let Some(default_type) = &trait_params[idx].1 {
                    // Resolve the default type in the current context
                    // "Self" in defaults should resolve to the target type
                    let resolved_default = self.resolve_type(default_type);
                    resolved_args.push(resolved_default);
                } else {
                    // No default provided, this is an error
                    self.error(
                        format!("missing type argument for trait parameter '{}'", trait_params[idx].0),
                        i.span,
                    );
                    resolved_args.push(ResolvedType::Error);
                }
            }
            
            resolved_args
        } else {
            Vec::new()
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
        
        self.pop_scope();  // Pop method scope
        self.self_type = None;
        self.pop_scope();  // Pop impl type params scope

        Some(ResolvedImpl {
            type_params,
            trait_def,
            trait_type_args,
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
                module_id: self.current_module,
                is_pub: f.is_pub,
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
                module_id: self.current_module,
                is_pub: false,
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
            
            // Resolve bounds
            let mut bounds = Vec::new();
            for bound in &type_param.bounds {
                let bound_type = self.resolve_type(bound);
                bounds.push(bound_type);
            }
            
            let default = type_param.default.as_ref().map(|t| self.resolve_type(t));
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: type_param.name.name.clone(),
                bounds,
                default,
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
                module_id: self.current_module,
                is_pub: false,
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
            name_span: f.name.span,
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
                module_id: self.current_module,
                is_pub: f.is_pub,
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
                module_id: self.current_module,
                is_pub: false,
            };
            self.defs.insert(param_id, param_info);
            self.scope.define(type_param.name.name.clone(), param_id);
            
            // Resolve bounds
            let bounds: Vec<_> = type_param.bounds.iter()
                .map(|b| self.resolve_type(b))
                .collect();
            
            let default = type_param.default.as_ref().map(|t| self.resolve_type(t));
            
            type_params.push(ResolvedTypeParam {
                def_id: param_id,
                name: type_param.name.name.clone(),
                bounds,
                default,
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
                false, // parameters are not public
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
            name_span: f.name.span,
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
                module_id: self.current_module,
                is_pub: false,
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
                
                // Check for namespaced type (e.g., "io.Display" encoded as "io.Display")
                // This is a workaround until we have proper type path syntax
                if let Some(dot_pos) = name.find('.') {
                    let ns_name = &name[..dot_pos];
                    let type_name = &name[dot_pos + 1..];
                    if let Some((def_id, is_pub)) = self.lookup_in_namespace_with_visibility(ns_name, type_name) {
                        // Check visibility - types from other modules must be public
                        if !is_pub {
                            self.error(
                                format!("type '{}' is private", type_name),
                                ty.span,
                            );
                        }
                        return ResolvedType::Named {
                            name: type_name.to_string(),
                            def_id: Some(def_id),
                            type_args: resolved_args,
                        };
                    }
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
                    false, // locals are not public
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
                // Check if this is a namespace access (e.g., io.print or std.io.print)
                // First, try to resolve the full namespace path
                if let Some(ns_path) = self.collect_namespace_path(base) {
                    // Try to resolve: look for nested namespace or item
                    if let Some(result) = self.resolve_namespace_access(&ns_path, &field.name, field.span, expr.span) {
                        return result;
                    }
                }
                
                // Check if this is a simple namespace access (e.g., io.print)
                if let ExprKind::Ident(ref ident) = base.kind {
                    if self.is_namespace(&ident.name) {
                        // This is a namespace access - resolve to the item in the namespace
                        if let Some((def_id, is_pub)) = self.lookup_in_namespace_with_visibility(&ident.name, &field.name) {
                            // Check visibility - items from other modules must be public
                            if !is_pub {
                                self.error(
                                    format!("'{}' is private", field.name),
                                    field.span,
                                );
                            }
                            return ResolvedExpr {
                                kind: ResolvedExprKind::Var {
                                    name: field.name.clone(),
                                    def_id,
                                },
                                span: expr.span,
                            };
                        } else {
                            // Check if this is a child namespace (will be resolved later in the chain)
                            if let Some(ns) = self.namespaces.get(&ident.name) {
                                if ns.children.contains_key(&field.name) {
                                    // This is accessing a child namespace, return a special marker
                                    // that will be handled by the next level of field access
                                    return ResolvedExpr {
                                        kind: ResolvedExprKind::NamespacePath(vec![ident.name.clone(), field.name.clone()]),
                                        span: expr.span,
                                    };
                                }
                            }
                            self.error(
                                format!("cannot find '{}' in namespace '{}'", field.name, ident.name),
                                field.span,
                            );
                            return ResolvedExpr {
                                kind: ResolvedExprKind::Error,
                                span: expr.span,
                            };
                        }
                    }
                }
                
                // Regular field access
                ResolvedExprKind::Field {
                    expr: Box::new(self.resolve_expr(base)),
                    field: field.name.clone(),
                    field_def: None, // Resolved during type checking
                    field_span: field.span,
                }
            }
            
            ExprKind::StructLit(name, fields) => {
                match self.lookup(&name.name) {
                    Some(struct_def) => {
                        let resolved_fields: Vec<_> = fields.iter()
                            .map(|f| (f.name.name.clone(), f.name.span, self.resolve_expr(&f.value)))
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
            
            ExprKind::For(binding, iter, body) => {
                // Resolve the iterator expression first (before entering the loop scope)
                let resolved_iter = self.resolve_expr(iter);
                
                // Create a new scope for the loop body with the binding
                self.push_scope();
                let binding_def = self.define(binding.name.clone(), DefKind::Local, binding.span, None, false);
                let resolved_body = self.resolve_block(body);
                self.pop_scope();
                
                ResolvedExprKind::For {
                    binding: binding_def,
                    binding_name: binding.name.clone(),
                    iter: Box::new(resolved_iter),
                    body: resolved_body,
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
            
            ExprKind::ArrayLit(elements) => {
                ResolvedExprKind::ArrayLit(
                    elements.iter().map(|e| self.resolve_expr(e)).collect()
                )
            }
            
            ExprKind::Lambda(params, body) => {
                // Create a new scope for lambda body
                self.push_scope();
                
                // Define parameters in scope
                let resolved_params: Vec<_> = params.iter().map(|p| {
                    let def_id = self.define(p.name.name.clone(), DefKind::Parameter, p.span, None, false);
                    let ty = p.ty.as_ref().map(|t| self.resolve_type(t));
                    ResolvedLambdaParam {
                        def_id,
                        name: p.name.name.clone(),
                        ty,
                        span: p.span,
                    }
                }).collect();
                
                let resolved_body = self.resolve_expr(body);
                self.pop_scope();
                
                ResolvedExprKind::Lambda {
                    params: resolved_params,
                    body: Box::new(resolved_body),
                }
            }
            
            ExprKind::Cast(inner, target_type) => {
                let resolved_expr = self.resolve_expr(inner);
                let resolved_type = self.resolve_type(target_type);
                ResolvedExprKind::Cast {
                    expr: Box::new(resolved_expr),
                    target_type: resolved_type,
                }
            }
            
            ExprKind::StringInterp(parts) => {
                let resolved_parts = parts.iter().map(|part| {
                    match part {
                        wisp_ast::StringInterpPart::Literal(s) => {
                            ResolvedStringInterpPart::Literal(s.clone())
                        }
                        wisp_ast::StringInterpPart::Expr(e) => {
                            ResolvedStringInterpPart::Expr(self.resolve_expr(e))
                        }
                    }
                }).collect();
                ResolvedExprKind::StringInterp { parts: resolved_parts }
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
                    false, // locals are not public
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
        "bool" | "char" | "str" | "Never"
    )
}

