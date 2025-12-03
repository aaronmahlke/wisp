//! Type checking pass

use wisp_hir::*;
use wisp_lexer::Span;
use crate::types::*;
use std::collections::{HashMap, HashSet};

/// Type error
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}..{}", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for TypeError {}

/// A specific instantiation of a generic function
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericInstantiation {
    /// The generic function's DefId
    pub func_def_id: DefId,
    /// The concrete types for each type parameter
    pub type_args: Vec<Type>,
}

/// Type parameter info including bounds
#[derive(Debug, Clone)]
pub struct TypeParamInfo {
    pub def_id: DefId,
    pub name: String,
    pub bounds: Vec<DefId>,  // Trait DefIds
}

/// Type checker
pub struct TypeChecker {
    ctx: TypeContext,
    errors: Vec<TypeError>,
    /// Current function's return type
    current_return_type: Option<Type>,
    /// Expression types (by span for now, could use NodeId)
    expr_types: HashMap<(usize, usize), Type>,
    /// Method lookup: (struct DefId, method name) -> (method DefId, method type)
    methods: HashMap<(DefId, String), (DefId, Type)>,
    /// Current Self type (when inside an impl block)
    current_self_type: Option<Type>,
    /// Collected generic instantiations
    generic_instantiations: HashSet<GenericInstantiation>,
    /// Map from generic function DefId to its type parameters (with bounds)
    generic_functions: HashMap<DefId, Vec<TypeParamInfo>>,
    /// Function parameter names and types: DefId -> Vec<(name, Type)>
    function_params: HashMap<DefId, Vec<(String, Type)>>,
    /// Trait methods: trait DefId -> [(method name, method type with Self as placeholder)]
    trait_methods: HashMap<DefId, Vec<(String, Type)>>,
    /// Trait implementations: (type DefId, trait DefId) -> impl methods
    trait_impls: HashMap<(DefId, DefId), Vec<(String, DefId, Type)>>,
    /// Function parameter names: function DefId -> [param names]
    function_param_names: HashMap<DefId, Vec<String>>,
    /// Associated functions (no self): (struct DefId, fn name) -> (fn DefId, fn type)
    associated_functions: HashMap<(DefId, String), (DefId, Type)>,
    /// Methods on primitive types: (primitive type name, method name) -> (method DefId, method type)
    primitive_methods: HashMap<(String, String), (DefId, Type)>,
    /// Trait implementations for primitives: (primitive type name, trait DefId) -> true
    primitive_trait_impls: HashSet<(String, DefId)>,
    /// Trait name to DefId lookup
    trait_by_name: HashMap<String, DefId>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            ctx: TypeContext::new(),
            errors: Vec::new(),
            current_return_type: None,
            expr_types: HashMap::new(),
            methods: HashMap::new(),
            current_self_type: None,
            generic_instantiations: HashSet::new(),
            generic_functions: HashMap::new(),
            function_params: HashMap::new(),
            trait_methods: HashMap::new(),
            trait_impls: HashMap::new(),
            function_param_names: HashMap::new(),
            associated_functions: HashMap::new(),
            primitive_methods: HashMap::new(),
            primitive_trait_impls: HashSet::new(),
            trait_by_name: HashMap::new(),
        }
    }

    /// Type check a resolved program
    pub fn check(program: &ResolvedProgram) -> Result<TypedProgram, Vec<TypeError>> {
        let mut checker = TypeChecker::new();
        let result = checker.check_program(program);
        
        if checker.errors.is_empty() {
            Ok(result)
        } else {
            Err(checker.errors)
        }
    }

    fn error(&mut self, message: String, span: Span) {
        self.errors.push(TypeError { message, span });
    }
    
    fn argument_count_error(&mut self, def_id: Option<DefId>, expected: usize, got: usize, span: Span) {
        self.argument_count_error_skip_params(def_id, expected, got, 0, span);
    }
    
    fn argument_count_error_skip_params(&mut self, def_id: Option<DefId>, expected: usize, got: usize, skip: usize, span: Span) {
        let message = if let Some(id) = def_id {
            if let Some(params) = self.function_params.get(&id) {
                if got < expected {
                    // Missing arguments (skip first N params for methods)
                    let start_idx = skip + got;
                    let missing: Vec<String> = params[start_idx..skip + expected].iter()
                        .map(|(name, ty)| format!("'{}:  {}'", name, ty.display(&self.ctx)))
                        .collect();
                    if missing.len() == 1 {
                        format!("missing argument {}", missing[0])
                    } else {
                        format!("missing arguments: {}", missing.join(", "))
                    }
                } else {
                    // Too many arguments
                    format!("too many arguments: expected {}, got {}", expected, got)
                }
            } else {
                // Fallback: no param info found
                format!("expected {} arguments, got {}", expected, got)
            }
        } else {
            // No def_id available
            format!("expected {} arguments, got {}", expected, got)
        };
        self.error(message, span);
    }

    fn check_program(&mut self, program: &ResolvedProgram) -> TypedProgram {
        // First pass: register all type names and struct/enum info
        for s in &program.structs {
            self.ctx.register_type_name(s.def_id, s.name.clone());
            self.ctx.register_def_type(s.def_id, Type::Struct(s.def_id));
        }
        
        for e in &program.enums {
            self.ctx.register_type_name(e.def_id, e.name.clone());
            self.ctx.register_def_type(e.def_id, Type::Enum(e.def_id));
        }

        // Second pass: register struct fields and enum variants
        for s in &program.structs {
            let fields: Vec<_> = s.fields.iter()
                .map(|f| (f.name.clone(), self.resolve_type(&f.ty)))
                .collect();
            self.ctx.register_struct_fields(s.def_id, fields.clone());
            
            // Record struct field spans for LSP
            for (i, f) in s.fields.iter().enumerate() {
                let field_type = &fields[i].1;
                self.ctx.record_span_type(f.span.start, f.span.end, format!("{}: {}", f.name, field_type.display(&self.ctx)));
                self.ctx.record_span_definition(f.span.start, f.span.end, f.def_id);
            }
        }

        for e in &program.enums {
            let variants: Vec<_> = e.variants.iter()
                .map(|v| {
                    let field_types: Vec<_> = v.fields.iter()
                        .map(|f| self.resolve_type(&f.ty))
                        .collect();
                    (v.name.clone(), v.def_id, field_types)
                })
                .collect();
            self.ctx.register_enum_variants(e.def_id, variants);
        }

        // Register trait names and methods
        for t in &program.traits {
            self.ctx.register_type_name(t.def_id, t.name.clone());
            self.trait_by_name.insert(t.name.clone(), t.def_id);
            
            // Collect trait method signatures
            let mut methods = Vec::new();
            for m in &t.methods {
                // Register type params for method
                for tp in &m.type_params {
                    self.ctx.register_type_param(tp.def_id, tp.name.clone());
                }
                let method_type = self.function_type(m);
                methods.push((m.name.clone(), method_type));
            }
            self.trait_methods.insert(t.def_id, methods);
        }

        // Third pass: register function types and names
        for f in &program.functions {
            // Register type parameters first so resolve_type can find them
            for tp in &f.type_params {
                self.ctx.register_type_param(tp.def_id, tp.name.clone());
            }
            let fn_type = self.function_type(f);
            self.ctx.register_def_type(f.def_id, fn_type);
            self.ctx.register_type_name(f.def_id, f.name.clone());
            
            // Register parameter names for named argument support
            let param_names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
            self.function_param_names.insert(f.def_id, param_names);
            
            // Track generic functions with their bounds
            if !f.type_params.is_empty() {
                let type_params: Vec<_> = f.type_params.iter()
                    .map(|tp| {
                        // Extract trait DefIds from bounds
                        let bounds: Vec<DefId> = tp.bounds.iter()
                            .filter_map(|b| {
                                if let ResolvedType::Named { def_id: Some(id), .. } = b {
                                    Some(*id)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        TypeParamInfo {
                            def_id: tp.def_id,
                            name: tp.name.clone(),
                            bounds,
                        }
                    })
                    .collect();
                self.generic_functions.insert(f.def_id, type_params);
            }
        }
        
        for imp in &program.impls {
            // Get the target type info
            let (target_struct_id, primitive_name) = match &imp.target_type {
                ResolvedType::Named { name, def_id: Some(id), .. } => (Some(*id), None),
                ResolvedType::Named { name, def_id: None, .. } => (None, Some(name.clone())), // Primitive type
                _ => (None, None),
            };
            
            // Set current_self_type so function_type can resolve &self correctly
            let target_type = self.resolve_type(&imp.target_type);
            self.current_self_type = Some(target_type);
            
            let mut impl_methods = Vec::new();
            
            for m in &imp.methods {
                let fn_type = self.function_type(m);
                self.ctx.register_def_type(m.def_id, fn_type.clone());
                self.ctx.register_type_name(m.def_id, m.name.clone());
                
                // Check if this is a method (has self) or associated function (no self)
                let has_self = m.params.first().map(|p| p.name == "self").unwrap_or(false);
                
                if let Some(struct_id) = target_struct_id {
                    if has_self {
                        // Method: called as instance.method(args)
                        self.methods.insert((struct_id, m.name.clone()), (m.def_id, fn_type.clone()));
                    } else {
                        // Associated function: called as Type.function(args)
                        self.associated_functions.insert((struct_id, m.name.clone()), (m.def_id, fn_type.clone()));
                    }
                } else if let Some(ref prim_name) = primitive_name {
                    if has_self {
                        // Method on primitive type
                        self.primitive_methods.insert((prim_name.clone(), m.name.clone()), (m.def_id, fn_type.clone()));
                    }
                }
                
                // Register parameter names for named argument support
                let param_names: Vec<String> = m.params.iter().map(|p| p.name.clone()).collect();
                self.function_param_names.insert(m.def_id, param_names);
                
                impl_methods.push((m.name.clone(), m.def_id, fn_type));
            }
            
            // Register trait implementation
            if let Some(trait_def) = imp.trait_def {
                if let Some(struct_id) = target_struct_id {
                    self.trait_impls.insert((struct_id, trait_def), impl_methods);
                } else if let Some(ref prim_name) = primitive_name {
                    // Primitive trait impl
                    self.primitive_trait_impls.insert((prim_name.clone(), trait_def));
                }
            }
            
            self.current_self_type = None;
        }
        
        // Register extern function types
        for f in &program.extern_functions {
            let fn_type = self.extern_function_type(f);
            self.ctx.register_def_type(f.def_id, fn_type);
            self.ctx.register_type_name(f.def_id, f.name.clone());
            
            // Register parameter names for named argument support
            let param_names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
            self.function_param_names.insert(f.def_id, param_names);
        }
        
        // Register extern static types
        for s in &program.extern_statics {
            let ty = self.resolve_type(&s.ty);
            self.ctx.register_def_type(s.def_id, ty);
            self.ctx.register_type_name(s.def_id, s.name.clone());
        }

        // Fourth pass: type check function bodies
        let mut typed_functions = Vec::new();
        for f in &program.functions {
            typed_functions.push(self.check_function(f));
        }

        let mut typed_impls = Vec::new();
        for imp in &program.impls {
            // Set current_self_type for this impl block
            let target_type = self.resolve_type(&imp.target_type);
            self.current_self_type = Some(target_type.clone());
            
            let mut methods = Vec::new();
            for m in &imp.methods {
                methods.push(self.check_function(m));
            }
            
            self.current_self_type = None;
            
            typed_impls.push(TypedImpl {
                trait_def: imp.trait_def,
                target_type,
                methods,
            });
        }
        
        // Create typed extern functions
        let mut typed_extern_functions = Vec::new();
        for f in &program.extern_functions {
            let params: Vec<_> = f.params.iter()
                .map(|p| TypedParam {
                    def_id: p.def_id,
                    name: p.name.clone(),
                    is_mut: p.is_mut,
                    ty: self.resolve_type(&p.ty),
                    span: p.span,
                })
                .collect();
            let return_type = f.return_type.as_ref()
                .map(|t| self.resolve_type(t))
                .unwrap_or(Type::Unit);
            typed_extern_functions.push(TypedExternFunction {
                def_id: f.def_id,
                name: f.name.clone(),
                params,
                return_type,
            });
        }

        // Create typed extern statics
        let mut typed_extern_statics = Vec::new();
        for s in &program.extern_statics {
            let ty = self.resolve_type(&s.ty);
            typed_extern_statics.push(TypedExternStatic {
                def_id: s.def_id,
                name: s.name.clone(),
                ty,
            });
        }

        TypedProgram {
            ctx: std::mem::take(&mut self.ctx),
            structs: program.structs.clone(),
            enums: program.enums.clone(),
            functions: typed_functions,
            extern_functions: typed_extern_functions,
            extern_statics: typed_extern_statics,
            impls: typed_impls,
            generic_instantiations: std::mem::take(&mut self.generic_instantiations),
        }
    }

    fn function_type(&self, f: &ResolvedFunction) -> Type {
        let params: Vec<_> = f.params.iter()
            .map(|p| self.resolve_type(&p.ty))
            .collect();
        let ret = f.return_type.as_ref()
            .map(|t| self.resolve_type(t))
            .unwrap_or(Type::Unit);
        Type::Function {
            params,
            ret: Box::new(ret),
        }
    }
    
    fn extern_function_type(&self, f: &ResolvedExternFunction) -> Type {
        let params: Vec<_> = f.params.iter()
            .map(|p| self.resolve_type(&p.ty))
            .collect();
        let ret = f.return_type.as_ref()
            .map(|t| self.resolve_type(t))
            .unwrap_or(Type::Unit);
        Type::Function {
            params,
            ret: Box::new(ret),
        }
    }

    fn resolve_type(&self, ty: &ResolvedType) -> Type {
        match ty {
            ResolvedType::Named { name, def_id, type_args } => {
                // Resolve type arguments
                let _resolved_args: Vec<_> = type_args.iter()
                    .map(|arg| self.resolve_type(arg))
                    .collect();
                
                if let Some(prim) = parse_type_name(name) {
                    // For now, primitives don't have type args
                    prim
                } else if let Some(id) = def_id {
                    // Check if it's a type parameter
                    if self.ctx.is_type_param(*id) {
                        Type::TypeParam(*id, name.clone())
                    } else if let Some(existing) = self.ctx.get_def_type(*id) {
                        // TODO: Apply type arguments to generic types
                        existing.clone()
                    } else {
                        Type::Struct(*id) // Default to struct
                    }
                } else {
                    Type::Error
                }
            }
            ResolvedType::Ref { is_mut, inner } => Type::Ref {
                is_mut: *is_mut,
                inner: Box::new(self.resolve_type(inner)),
            },
            ResolvedType::Slice { elem } => Type::Slice(Box::new(self.resolve_type(elem))),
            ResolvedType::Unit => Type::Unit,
            ResolvedType::SelfType => {
                self.current_self_type.clone().unwrap_or(Type::Error)
            }
            ResolvedType::Error => Type::Error,
        }
    }

    /// Check if a cast from one type to another is valid
    fn is_valid_cast(&self, from: &Type, to: &Type) -> bool {
        // Same type is always valid
        if from == to {
            return true;
        }
        
        // Check for numeric casts
        let is_numeric = |t: &Type| matches!(t,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64
        );
        
        // Numeric to numeric is valid
        if is_numeric(from) && is_numeric(to) {
            return true;
        }
        
        // Char to/from integer is valid
        if matches!(from, Type::Char) && is_numeric(to) {
            return true;
        }
        if is_numeric(from) && matches!(to, Type::Char) {
            return true;
        }
        
        // Bool to integer is valid
        if matches!(from, Type::Bool) && is_numeric(to) {
            return true;
        }
        
        // Pointer types (str, references) to i64 is valid (for FFI)
        if matches!(from, Type::Str | Type::Ref { .. }) && matches!(to, Type::I64) {
            return true;
        }
        
        // i64 to pointer types is valid (for FFI)
        if matches!(from, Type::I64) && matches!(to, Type::Str | Type::Ref { .. }) {
            return true;
        }
        
        false
    }

    fn check_function(&mut self, f: &ResolvedFunction) -> TypedFunction {
        // Register type parameters
        for tp in &f.type_params {
            self.ctx.register_type_param(tp.def_id, tp.name.clone());
        }
        
        // Register parameter types and record span→type for LSP
        let mut param_types = Vec::new();
        for p in &f.params {
            let ty = self.resolve_type(&p.ty);
            self.ctx.register_def_type(p.def_id, ty.clone());
            // Record parameter span for hover
            self.ctx.record_span_type(p.span.start, p.span.end, format!("{}: {}", p.name, ty.display(&self.ctx)));
            // Record definition for go-to-definition
            self.ctx.record_span_definition(p.span.start, p.span.end, p.def_id);
            param_types.push((p.name.clone(), ty));
        }
        
        // Store parameter info for better error messages
        self.function_params.insert(f.def_id, param_types.clone());

        // Set return type context
        let return_type = f.return_type.as_ref()
            .map(|t| self.resolve_type(t))
            .unwrap_or(Type::Unit);
        self.current_return_type = Some(return_type.clone());
        
        // Record function signature at name span for hover on function definitions
        let params_str: Vec<String> = param_types.iter()
            .map(|(name, ty)| format!("{}: {}", name, ty.display(&self.ctx)))
            .collect();
        let sig = format!("fn {}({}) -> {}", f.name, params_str.join(", "), return_type.display(&self.ctx));
        self.ctx.record_span_type(f.name_span.start, f.name_span.end, sig);
        self.ctx.record_span_definition(f.name_span.start, f.name_span.end, f.def_id);

        // Check body
        let body = f.body.as_ref().map(|b| self.check_block(b, Some(&return_type)));

        self.current_return_type = None;

        TypedFunction {
            def_id: f.def_id,
            name: f.name.clone(),
            params: f.params.iter().map(|p| {
                TypedParam {
                    def_id: p.def_id,
                    name: p.name.clone(),
                    is_mut: p.is_mut,
                    ty: self.resolve_type(&p.ty),
                    span: p.span,
                }
            }).collect(),
            return_type,
            body,
            span: f.span,
            name_span: f.name_span,
        }
    }

    fn check_block(&mut self, block: &ResolvedBlock, expected: Option<&Type>) -> TypedBlock {
        let mut stmts = Vec::new();
        let mut last_type = Type::Unit;

        for (i, stmt) in block.stmts.iter().enumerate() {
            let is_last = i == block.stmts.len() - 1;
            let (typed_stmt, stmt_type) = self.check_stmt(stmt);
            stmts.push(typed_stmt);
            
            if is_last {
                last_type = stmt_type;
            }
        }

        // Check that block type matches expected
        if let Some(expected) = expected {
            if let Err(e) = self.ctx.unify(&last_type, expected) {
                self.error(format!("block type mismatch: {}", e), block.span);
            }
        }

        TypedBlock {
            stmts,
            ty: self.ctx.apply(&last_type),
        }
    }

    fn check_stmt(&mut self, stmt: &ResolvedStmt) -> (TypedStmt, Type) {
        match stmt {
            ResolvedStmt::Let { def_id, name, is_mut, ty, init, span } => {
                let declared_type = ty.as_ref().map(|t| self.resolve_type(t));
                
                // Type check the initializer with the expected type (if declared)
                let typed_init = init.as_ref().map(|e| {
                    self.check_expr_with_expected(e, declared_type.as_ref())
                });

                // Determine the type
                let var_type = match (&declared_type, &typed_init) {
                    (Some(d), Some(init_expr)) => {
                        if let Err(e) = self.ctx.unify(d, &init_expr.ty) {
                            self.error(format!("type mismatch in let: {}", e), *span);
                        }
                        self.ctx.apply(d)
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(init_expr)) => init_expr.ty.clone(),
                    (None, None) => {
                        self.error("cannot infer type without initializer".to_string(), *span);
                        Type::Error
                    }
                };

                self.ctx.register_def_type(*def_id, var_type.clone());
                
                // Record span→type for LSP hover
                self.ctx.record_span_type(span.start, span.end, format!("{}: {}", name, var_type.display(&self.ctx)));
                // Record definition for go-to-definition
                self.ctx.record_span_definition(span.start, span.end, *def_id);

                (TypedStmt::Let {
                    def_id: *def_id,
                    name: name.clone(),
                    is_mut: *is_mut,
                    ty: var_type,
                    init: typed_init,
                    span: *span,
                }, Type::Unit)
            }
            ResolvedStmt::Expr(expr) => {
                let typed = self.check_expr(expr);
                let ty = typed.ty.clone();
                (TypedStmt::Expr(typed), ty)
            }
        }
    }

    /// Reorder named arguments to match parameter order
    /// Returns references to the argument expressions in the correct order
    /// For named arguments with missing parameters, reports errors with type info
    fn reorder_named_args<'a>(
        &mut self,
        args: &'a [ResolvedCallArg],
        func_def_id: DefId,
        span: Span,
    ) -> Vec<&'a ResolvedExpr> {
        // Check if any args are named
        let has_named = args.iter().any(|a| a.name.is_some());
        
        if !has_named {
            // All positional - return in order
            return args.iter().map(|a| &a.value).collect();
        }
        
        // Get parameter names for this function
        let param_names = match self.function_param_names.get(&func_def_id) {
            Some(names) => names.clone(),
            None => {
                // No param names registered, use positional
                return args.iter().map(|a| &a.value).collect();
            }
        };
        
        // Get parameter types
        let param_types = match self.function_params.get(&func_def_id) {
            Some(types) => types.clone(),
            None => {
                // No type info, can't give detailed errors
                return args.iter().map(|a| &a.value).collect();
            }
        };
        
        // Check if all args are named (no mixing allowed)
        let has_positional = args.iter().any(|a| a.name.is_none());
        if has_positional {
            self.error("cannot mix positional and named arguments".to_string(), span);
            return args.iter().map(|a| &a.value).collect();
        }
        
        // Build reordered args
        let mut result: Vec<Option<&'a ResolvedExpr>> = (0..param_names.len()).map(|_| None).collect();
        let mut used_params: HashSet<String> = HashSet::new();
        
        for arg in args {
            let name = arg.name.as_ref().unwrap();
            
            if let Some(idx) = param_names.iter().position(|p| p == name) {
                if used_params.contains(name) {
                    self.error(format!("argument '{}' specified more than once", name), arg.span);
                    continue;
                }
                used_params.insert(name.clone());
                result[idx] = Some(&arg.value);
            } else {
                self.error(format!("unknown parameter '{}'", name), arg.span);
            }
        }
        
        // Check for missing arguments and report with type info
        let missing: Vec<String> = param_names.iter().enumerate()
            .filter(|(i, _)| result[*i].is_none())
            .map(|(i, name)| format!("'{}:  {}'", name, param_types[i].1.display(&self.ctx)))
            .collect();
        
        if !missing.is_empty() {
            let message = if missing.len() == 1 {
                format!("missing argument {}", missing[0])
            } else {
                format!("missing arguments: {}", missing.join(", "))
            };
            self.error(message, span);
        }
        
        // Return only the provided arguments in order (filter out None)
        result.into_iter().filter_map(|o| o).collect()
    }

    /// Check and reorder function call arguments based on parameter names
    /// Returns the typed arguments in the correct order, or None if there's an error
    fn check_call_args(
        &mut self,
        args: &[ResolvedCallArg],
        param_names: &[String],
        param_types: &[Type],
        span: Span,
    ) -> Vec<TypedExpr> {
        // Check if all args are named or all are positional
        let has_named = args.iter().any(|a| a.name.is_some());
        let has_positional = args.iter().any(|a| a.name.is_none());
        
        if has_named && has_positional {
            self.error("cannot mix positional and named arguments".to_string(), span);
            return args.iter().map(|a| self.check_expr(&a.value)).collect();
        }
        
        if has_named {
            // Named arguments - reorder to match parameter order
            let mut result: Vec<Option<TypedExpr>> = (0..param_names.len()).map(|_| None).collect();
            let mut used_params: HashSet<&str> = HashSet::new();
            
            for arg in args {
                let name = arg.name.as_ref().unwrap();
                
                // Find the parameter index
                if let Some(idx) = param_names.iter().position(|p| p == name) {
                    if used_params.contains(name.as_str()) {
                        self.error(format!("argument '{}' specified more than once", name), arg.span);
                        continue;
                    }
                    used_params.insert(name);
                    
                    let typed = self.check_expr(&arg.value);
                    
                    // Check type
                    if let Err(e) = self.ctx.unify(&typed.ty, &param_types[idx]) {
                        self.error(
                            format!("argument '{}' type mismatch: {}", name, e),
                            arg.span
                        );
                    }
                    
                    result[idx] = Some(typed);
                } else {
                    self.error(format!("unknown parameter '{}'", name), arg.span);
                }
            }
            
            // Check all required parameters are provided
            let missing: Vec<String> = param_names.iter().enumerate()
                .filter(|(i, _)| result[*i].is_none())
                .map(|(i, name)| format!("'{}:  {}'", name, param_types[i].display(&self.ctx)))
                .collect();
            
            if !missing.is_empty() {
                let message = if missing.len() == 1 {
                    format!("missing argument {}", missing[0])
                } else {
                    format!("missing arguments: {}", missing.join(", "))
                };
                self.error(message, span);
                
                // Create error placeholders
                for i in 0..param_names.len() {
                    if result[i].is_none() {
                        result[i] = Some(TypedExpr {
                            kind: TypedExprKind::IntLiteral(0),
                            ty: Type::Error,
                            span,
                        });
                    }
                }
            }
            
            result.into_iter().map(|o| o.unwrap()).collect()
        } else {
            // Positional arguments - just type check in order
            if args.len() != param_types.len() {
                // Generate specific error message
                let message = if args.len() < param_types.len() {
                    // Missing arguments
                    let missing: Vec<String> = param_names[args.len()..].iter().zip(&param_types[args.len()..])
                        .map(|(name, ty)| format!("'{}:  {}'", name, ty.display(&self.ctx)))
                        .collect();
                    if missing.len() == 1 {
                        format!("missing argument {}", missing[0])
                    } else {
                        format!("missing arguments: {}", missing.join(", "))
                    }
                } else {
                    // Too many arguments
                    format!("too many arguments: expected {}, got {}", param_types.len(), args.len())
                };
                self.error(message, span);
            }
            
            args.iter().enumerate().map(|(i, arg)| {
                let typed = self.check_expr(&arg.value);
                if i < param_types.len() {
                    if let Err(e) = self.ctx.unify(&typed.ty, &param_types[i]) {
                        self.error(
                            format!("argument {} type mismatch: {}", i + 1, e),
                            arg.span
                        );
                    }
                }
                typed
            }).collect()
        }
    }

    fn check_expr(&mut self, expr: &ResolvedExpr) -> TypedExpr {
        self.check_expr_with_expected(expr, None)
    }
    
    fn check_expr_with_expected(&mut self, expr: &ResolvedExpr, expected: Option<&Type>) -> TypedExpr {
        let (kind, ty) = match &expr.kind {
            ResolvedExprKind::IntLiteral(n) => {
                // Use expected type if it's a numeric type, otherwise default to i32
                let ty = match expected {
                    Some(Type::I8) => Type::I8,
                    Some(Type::I16) => Type::I16,
                    Some(Type::I32) => Type::I32,
                    Some(Type::I64) => Type::I64,
                    Some(Type::I128) => Type::I128,
                    Some(Type::U8) => Type::U8,
                    Some(Type::U16) => Type::U16,
                    Some(Type::U32) => Type::U32,
                    Some(Type::U64) => Type::U64,
                    Some(Type::U128) => Type::U128,
                    _ => Type::I32, // Default to i32
                };
                (TypedExprKind::IntLiteral(*n), ty)
            }
            ResolvedExprKind::FloatLiteral(n) => {
                // Use expected type if it's a float type, otherwise default to f64
                let ty = match expected {
                    Some(Type::F32) => Type::F32,
                    Some(Type::F64) => Type::F64,
                    _ => Type::F64, // Default to f64
                };
                (TypedExprKind::FloatLiteral(*n), ty)
            }
            ResolvedExprKind::BoolLiteral(b) => {
                (TypedExprKind::BoolLiteral(*b), Type::Bool)
            }
            ResolvedExprKind::StringLiteral(s) => {
                (TypedExprKind::StringLiteral(s.clone()), Type::Str)
            }
            
            ResolvedExprKind::Var { name, def_id } => {
                let ty = self.ctx.get_def_type(*def_id)
                    .cloned()
                    .unwrap_or_else(|| {
                        self.error(format!("no type for variable '{}'", name), expr.span);
                        Type::Error
                    });
                (TypedExprKind::Var { name: name.clone(), def_id: *def_id }, ty)
            }
            
            ResolvedExprKind::Binary { left, op, right } => {
                let left_typed = self.check_expr(left);
                let right_typed = self.check_expr(right);
                
                // Check if this is a comparison operator (uses references)
                let is_comparison_op = matches!(*op, 
                    wisp_ast::BinOp::Eq | wisp_ast::BinOp::NotEq |
                    wisp_ast::BinOp::Lt | wisp_ast::BinOp::Gt |
                    wisp_ast::BinOp::LtEq | wisp_ast::BinOp::GtEq
                );
                
                // Auto-deref support: if operands are references, extract inner types
                // and prepare deref expressions
                let (effective_left_ty, left_for_op, needs_left_deref) = if let Type::Ref { inner, .. } = &left_typed.ty {
                    ((**inner).clone(), left_typed.clone(), true)
                } else {
                    (left_typed.ty.clone(), left_typed.clone(), false)
                };
                
                let (effective_right_ty, right_for_op, needs_right_deref) = if let Type::Ref { inner, .. } = &right_typed.ty {
                    ((**inner).clone(), right_typed.clone(), true)
                } else {
                    (right_typed.ty.clone(), right_typed.clone(), false)
                };
                
                // Get the type name for mangling (using effective type, after auto-deref)
                let left_type_name: Option<String> = match &effective_left_ty {
                    Type::Struct(def_id) | Type::Enum(def_id) => self.ctx.get_type_name(*def_id),
                    Type::I8 => Some("i8".to_string()),
                    Type::I16 => Some("i16".to_string()),
                    Type::I32 => Some("i32".to_string()),
                    Type::I64 => Some("i64".to_string()),
                    Type::U8 => Some("u8".to_string()),
                    Type::U16 => Some("u16".to_string()),
                    Type::U32 => Some("u32".to_string()),
                    Type::U64 => Some("u64".to_string()),
                    Type::F32 => Some("f32".to_string()),
                    Type::F64 => Some("f64".to_string()),
                    Type::Bool => Some("bool".to_string()),
                    Type::Str => Some("str".to_string()),
                    _ => None,
                };
                
                // Special handling for != : desugar to !(left == right)
                if *op == wisp_ast::BinOp::NotEq {
                    // Skip type parameters - they'll be handled during monomorphization
                    // Use effective_left_ty to support auto-deref
                    if !matches!(&effective_left_ty, Type::TypeParam(_, _)) && !effective_left_ty.is_primitive() {
                        // Try to find PartialEq::eq implementation on the effective (dereferenced) type
                        if let Some((method_def_id, _method_type)) = self.find_op_trait_method(&effective_left_ty, "PartialEq", "eq") {
                            // Construct full method name: TypeName::eq
                            let method_name = left_type_name.as_ref()
                                .map(|name| format!("{}::eq", name))
                                .unwrap_or_else(|| "eq".to_string());
                            
                            // For comparison, operands should be references to the effective types
                            // If already references, use them directly; otherwise wrap in Ref
                            let left_ref = if needs_left_deref {
                                // Already a reference, use as-is
                                left_for_op.clone()
                            } else {
                                TypedExpr {
                                    ty: Type::Ref { is_mut: false, inner: Box::new(effective_left_ty.clone()) },
                                    kind: TypedExprKind::Ref { is_mut: false, expr: Box::new(left_typed.clone()) },
                                    span: left_typed.span,
                                }
                            };
                            let right_ref = if needs_right_deref {
                                right_for_op.clone()
                            } else {
                                TypedExpr {
                                    ty: Type::Ref { is_mut: false, inner: Box::new(effective_right_ty.clone()) },
                                    kind: TypedExprKind::Ref { is_mut: false, expr: Box::new(right_typed.clone()) },
                                    span: right_typed.span,
                                }
                            };
                            
                            // Create the eq call
                            let eq_call = TypedExpr {
                                kind: TypedExprKind::OperatorCall {
                                    method_def_id,
                                    method_name,
                                    left: Box::new(left_ref),
                                    right: Box::new(right_ref),
                                },
                                ty: Type::Bool,
                                span: expr.span,
                            };
                            
                            // Wrap in unary not
                            return TypedExpr {
                                kind: TypedExprKind::Unary {
                                    op: wisp_ast::UnaryOp::Not,
                                    expr: Box::new(eq_call),
                                },
                                ty: Type::Bool,
                                span: expr.span,
                            };
                        }
                    }
                }
                
                // Check if this is an overloadable operator on a non-primitive type
                // For type parameters, we keep it as Binary and let monomorphization handle it
                // Use effective types to support auto-deref
                if let Some((trait_name, method_name)) = Self::op_to_trait(*op) {
                    // Skip type parameters - they'll be handled during monomorphization
                    if !matches!(&effective_left_ty, Type::TypeParam(_, _)) && !effective_left_ty.is_primitive() {
                        // Try to find an operator trait implementation for the effective (dereferenced) type
                        if let Some((method_def_id, method_type)) = self.find_op_trait_method(&effective_left_ty, trait_name, method_name) {
                            // Desugar to an operator call: left.method(right)
                            let result_type = if let Type::Function { ret, .. } = &method_type {
                                (**ret).clone()
                            } else {
                                self.error(format!("operator {} method has wrong type", method_name), expr.span);
                                Type::Error
                            };
                            
                            // Prepare operands with auto-deref and reference wrapping as needed
                            let (left_arg, right_arg) = if is_comparison_op {
                                // Comparison operators take references
                                // If already a reference, use as-is; otherwise wrap in Ref
                                let left_ref = if needs_left_deref {
                                    left_for_op.clone()
                                } else {
                                    TypedExpr {
                                        ty: Type::Ref { is_mut: false, inner: Box::new(effective_left_ty.clone()) },
                                        kind: TypedExprKind::Ref { is_mut: false, expr: Box::new(left_typed.clone()) },
                                        span: left_typed.span,
                                    }
                                };
                                let right_ref = if needs_right_deref {
                                    right_for_op.clone()
                                } else {
                                    TypedExpr {
                                        ty: Type::Ref { is_mut: false, inner: Box::new(effective_right_ty.clone()) },
                                        kind: TypedExprKind::Ref { is_mut: false, expr: Box::new(right_typed.clone()) },
                                        span: right_typed.span,
                                    }
                                };
                                (left_ref, right_ref)
                            } else {
                                // Arithmetic operators take by value - auto-deref if needed
                                let left_val = if needs_left_deref {
                                    TypedExpr {
                                        ty: effective_left_ty.clone(),
                                        kind: TypedExprKind::Deref(Box::new(left_for_op.clone())),
                                        span: left_typed.span,
                                    }
                                } else {
                                    left_typed.clone()
                                };
                                let right_val = if needs_right_deref {
                                    TypedExpr {
                                        ty: effective_right_ty.clone(),
                                        kind: TypedExprKind::Deref(Box::new(right_for_op.clone())),
                                        span: right_typed.span,
                                    }
                                } else {
                                    right_typed.clone()
                                };
                                (left_val, right_val)
                            };
                            
                            // Construct full method name: TypeName::method
                            let full_method_name = left_type_name.as_ref()
                                .map(|name| format!("{}::{}", name, method_name))
                                .unwrap_or_else(|| method_name.to_string());
                            
                            return TypedExpr {
                                kind: TypedExprKind::OperatorCall {
                                    method_def_id,
                                    method_name: full_method_name,
                                    left: Box::new(left_arg),
                                    right: Box::new(right_arg),
                                },
                                ty: result_type,
                                span: expr.span,
                            };
                        } else {
                            // Non-primitive type doesn't implement the required operator trait
                            let type_name = left_type_name.as_ref()
                                .map(|s| s.as_str())
                                .unwrap_or("unknown type");
                            let op_symbol = match *op {
                                wisp_ast::BinOp::Add => "+",
                                wisp_ast::BinOp::Sub => "-",
                                wisp_ast::BinOp::Mul => "*",
                                wisp_ast::BinOp::Div => "/",
                                wisp_ast::BinOp::Mod => "%",
                                wisp_ast::BinOp::Eq => "==",
                                wisp_ast::BinOp::NotEq => "!=",
                                wisp_ast::BinOp::Lt => "<",
                                wisp_ast::BinOp::Gt => ">",
                                wisp_ast::BinOp::LtEq => "<=",
                                wisp_ast::BinOp::GtEq => ">=",
                                _ => "?",
                            };
                            self.error(
                                format!("cannot apply `{}` to type `{}`: `{}` does not implement `{}`",
                                    op_symbol, type_name, type_name, trait_name),
                                expr.span
                            );
                            return TypedExpr {
                                kind: TypedExprKind::Binary {
                                    left: Box::new(left_typed),
                                    op: *op,
                                    right: Box::new(right_typed),
                                },
                                ty: Type::Error,
                                span: expr.span,
                            };
                        }
                    }
                }
                
                // Fall back to built-in operator handling (for primitives)
                let result_type = self.check_binary_op(*op, &left_typed.ty, &right_typed.ty, expr.span);
                
                (TypedExprKind::Binary {
                    left: Box::new(left_typed),
                    op: *op,
                    right: Box::new(right_typed),
                }, result_type)
            }
            
            ResolvedExprKind::Unary { op, expr: inner } => {
                let inner_typed = self.check_expr(inner);
                let result_type = self.check_unary_op(*op, &inner_typed.ty, expr.span);
                
                (TypedExprKind::Unary {
                    op: *op,
                    expr: Box::new(inner_typed),
                }, result_type)
            }
            
            ResolvedExprKind::Call { callee, args } => {
                // Check if this is a method call or associated function call: expr.method(args) or Type.function(args)
                if let ResolvedExprKind::Field { expr: receiver, field: method_name, field_span, .. } = &callee.kind {
                    let method_span = *field_span;
                    // Check if receiver is a type name (for associated function calls like Point.new())
                    if let ResolvedExprKind::Var { def_id, .. } = &receiver.kind {
                        // Check if this def_id refers to a struct type
                        if let Some(Type::Struct(struct_id)) = self.ctx.get_def_type(*def_id) {
                            let struct_id = *struct_id; // Copy the DefId
                            // This is potentially an associated function call
                            if let Some((fn_def_id, fn_type)) = self.associated_functions.get(&(struct_id, method_name.clone())).cloned() {
                                // This is an associated function call!
                                let args_typed: Vec<_> = args.iter().map(|a| self.check_expr(&a.value)).collect();
                                
                                let result_type = if let Type::Function { params, ret } = &fn_type {
                                    // Check argument count and types (no self parameter)
                                    if args_typed.len() != params.len() {
                                        self.argument_count_error(Some(fn_def_id), params.len(), args_typed.len(), expr.span);
                                        Type::Error
                                    } else {
                                        for (i, (arg, param)) in args_typed.iter().zip(params.iter()).enumerate() {
                                            if let Err(e) = self.ctx.unify(&arg.ty, param) {
                                                self.error(
                                                    format!("argument {} type mismatch: {}", i + 1, e),
                                                    expr.span
                                                );
                                            }
                                        }
                                        (**ret).clone()
                                    }
                                } else {
                                    Type::Error
                                };
                                
                                // Record function signature at function span for hover
                                if let Type::Function { params, ret } = &fn_type {
                                    let params_str: Vec<String> = params.iter()
                                        .map(|p| p.display(&self.ctx))
                                        .collect();
                                    let sig = format!("fn {}({}) -> {}", method_name, params_str.join(", "), ret.display(&self.ctx));
                                    self.ctx.record_span_type(method_span.start, method_span.end, sig);
                                    self.ctx.record_span_definition(method_span.start, method_span.end, fn_def_id);
                                }
                                
                                return TypedExpr {
                                    kind: TypedExprKind::AssociatedFunctionCall {
                                        type_id: struct_id,  // struct_id is already owned from the match
                                        function: method_name.clone(),
                                        function_def_id: fn_def_id,
                                        function_span: method_span,
                                        args: args_typed,
                                    },
                                    ty: result_type,
                                    span: expr.span,
                                };
                            }
                        }
                    }
                    
                    // First, check the receiver type
                    let receiver_typed = self.check_expr(receiver);
                    
                    // Get the struct id (with auto-deref for references)
                    let struct_id = match &receiver_typed.ty {
                        Type::Struct(id) => Some(*id),
                        Type::Ref { inner, .. } => {
                            if let Type::Struct(id) = inner.as_ref() {
                                Some(*id)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    
                    // Check if receiver is a type parameter with trait bounds
                    let type_param_info = match &receiver_typed.ty {
                        Type::TypeParam(def_id, _) => {
                            // Find the bounds for this type param
                            self.find_type_param_bounds(*def_id)
                        }
                        Type::Ref { inner, .. } => {
                            if let Type::TypeParam(def_id, _) = inner.as_ref() {
                                self.find_type_param_bounds(*def_id)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    
                    // Look up method on struct
                    if let Some(struct_id) = struct_id {
                        if let Some((method_def_id, method_type)) = self.methods.get(&(struct_id, method_name.clone())).cloned() {
                            // This is a method call!
                            // TODO: Handle named arguments for methods
                            let args_typed: Vec<_> = args.iter().map(|a| self.check_expr(&a.value)).collect();
                            
                            let (result_type, is_mut_self) = if let Type::Function { params, ret } = &method_type {
                                // Method's first param is &self or &mut self
                                // Check remaining args against remaining params
                                let method_params = &params[1..]; // Skip self param
                                
                                if args_typed.len() != method_params.len() {
                                    self.argument_count_error_skip_params(Some(method_def_id), method_params.len(), args_typed.len(), 1, expr.span);
                                    (Type::Error, false)
                                } else {
                                    for (i, (arg, param)) in args_typed.iter().zip(method_params.iter()).enumerate() {
                                        if let Err(e) = self.ctx.unify(&arg.ty, param) {
                                            self.error(
                                                format!("argument {} type mismatch: {}", i + 1, e),
                                                expr.span
                                            );
                                        }
                                    }
                                    // Check if self param is &mut
                                    let is_mut = params.first().map(|p| matches!(p, Type::Ref { is_mut: true, .. })).unwrap_or(false);
                                    ((**ret).clone(), is_mut)
                                }
                            } else {
                                (Type::Error, false)
                            };
                            
                            // Record method signature at method span for hover
                            if let Type::Function { params, ret } = &method_type {
                                let params_str: Vec<String> = params.iter()
                                    .map(|p| p.display(&self.ctx))
                                    .collect();
                                let sig = format!("fn {}({}) -> {}", method_name, params_str.join(", "), ret.display(&self.ctx));
                                self.ctx.record_span_type(method_span.start, method_span.end, sig);
                                self.ctx.record_span_definition(method_span.start, method_span.end, method_def_id);
                            }
                            
                            return TypedExpr {
                                kind: TypedExprKind::MethodCall {
                                    receiver: Box::new(receiver_typed),
                                    method: method_name.clone(),
                                    method_def_id,
                                    method_span,
                                    is_mut_self,
                                    args: args_typed,
                                },
                                ty: result_type,
                                span: expr.span,
                            };
                        }
                    }
                    
                    // Look up method via trait bounds on type parameter
                    if let Some(bounds) = type_param_info {
                        if let Some(method_type) = self.lookup_trait_method(&bounds, method_name) {
                            // TODO: Handle named arguments for trait methods
                            let args_typed: Vec<_> = args.iter().map(|a| self.check_expr(&a.value)).collect();
                            
                            let (result_type, is_mut_self) = if let Type::Function { params, ret } = &method_type {
                                let method_params = &params[1..]; // Skip self param
                                
                                if args_typed.len() != method_params.len() {
                                    self.argument_count_error(None, method_params.len(), args_typed.len(), expr.span);
                                    (Type::Error, false)
                                } else {
                                    // Check if self param is &mut
                                    let is_mut = params.first().map(|p| matches!(p, Type::Ref { is_mut: true, .. })).unwrap_or(false);
                                    // Note: we don't check arg types here since they're generic
                                    ((**ret).clone(), is_mut)
                                }
                            } else {
                                (Type::Error, false)
                            };
                            
                            // For trait method calls on type params, we use TraitMethodCall
                            return TypedExpr {
                                kind: TypedExprKind::TraitMethodCall {
                                    receiver: Box::new(receiver_typed),
                                    method: method_name.clone(),
                                    method_span,
                                    is_mut_self,
                                    trait_bounds: bounds,
                                    args: args_typed,
                                },
                                ty: result_type,
                                span: expr.span,
                            };
                        }
                    }
                    
                    // Look up method on primitive type
                    let primitive_name = match &receiver_typed.ty {
                        Type::I8 => Some("i8"),
                        Type::I16 => Some("i16"),
                        Type::I32 => Some("i32"),
                        Type::I64 => Some("i64"),
                        Type::U8 => Some("u8"),
                        Type::U16 => Some("u16"),
                        Type::U32 => Some("u32"),
                        Type::U64 => Some("u64"),
                        Type::F32 => Some("f32"),
                        Type::F64 => Some("f64"),
                        Type::Bool => Some("bool"),
                        Type::Str => Some("str"),
                        Type::Ref { inner, .. } => match inner.as_ref() {
                            Type::I32 => Some("i32"),
                            Type::I64 => Some("i64"),
                            Type::Bool => Some("bool"),
                            Type::Str => Some("str"),
                            _ => None,
                        },
                        _ => None,
                    };
                    
                    if let Some(prim_name) = primitive_name {
                        if let Some((method_def_id, method_type)) = self.primitive_methods.get(&(prim_name.to_string(), method_name.clone())).cloned() {
                            // Method call on primitive type
                            let args_typed: Vec<_> = args.iter().map(|a| self.check_expr(&a.value)).collect();
                            
                            let (result_type, is_mut_self) = if let Type::Function { params, ret } = &method_type {
                                let method_params = &params[1..]; // Skip self param
                                
                                if args_typed.len() != method_params.len() {
                                    self.argument_count_error_skip_params(Some(method_def_id), method_params.len(), args_typed.len(), 1, expr.span);
                                    (Type::Error, false)
                                } else {
                                    // Check if self param is &mut
                                    let is_mut = params.first().map(|p| matches!(p, Type::Ref { is_mut: true, .. })).unwrap_or(false);
                                    ((**ret).clone(), is_mut)
                                }
                            } else {
                                (Type::Error, false)
                            };
                            
                            // Record method signature at method span for hover
                            if let Type::Function { params, ret } = &method_type {
                                let params_str: Vec<String> = params.iter()
                                    .map(|p| p.display(&self.ctx))
                                    .collect();
                                let sig = format!("fn {}({}) -> {}", method_name, params_str.join(", "), ret.display(&self.ctx));
                                self.ctx.record_span_type(method_span.start, method_span.end, sig);
                                self.ctx.record_span_definition(method_span.start, method_span.end, method_def_id);
                            }
                            
                            return TypedExpr {
                                kind: TypedExprKind::PrimitiveMethodCall {
                                    receiver: Box::new(receiver_typed),
                                    method: method_name.clone(),
                                    method_def_id,
                                    method_span,
                                    is_mut_self,
                                    args: args_typed,
                                },
                                ty: result_type,
                                span: expr.span,
                            };
                        }
                    }
                    
                    // Not a method - fall through to regular field access + call
                    // This will error on the field access
                }
                
                // Regular function call
                let callee_typed = self.check_expr(callee);
                
                // Check if this is a call to a generic function
                let callee_def_id = match &callee_typed.kind {
                    TypedExprKind::Var { def_id, .. } => Some(*def_id),
                    _ => None,
                };
                
                // Check if any arguments are named
                let has_named = args.iter().any(|a| a.name.is_some());
                
                // Handle named arguments - reorder if needed
                let reordered_args = if let Some(def_id) = callee_def_id {
                    self.reorder_named_args(args, def_id, expr.span)
                } else {
                    // No def_id, just use args in order
                    args.iter().map(|a| &a.value).collect()
                };
                
                let args_typed: Vec<_> = reordered_args.iter().map(|a| self.check_expr(a)).collect();
                
                let (result_type, type_args) = match &callee_typed.ty {
                    Type::Function { params, ret } => {
                        // Check argument count (skip for named args, already checked in reorder_named_args)
                        if !has_named && args_typed.len() != params.len() {
                            self.argument_count_error(callee_def_id, params.len(), args_typed.len(), expr.span);
                            (Type::Error, None)
                        } else {
                            // For generic functions, infer type arguments
                            let inferred_type_args = if let Some(def_id) = callee_def_id {
                                if let Some(type_params) = self.generic_functions.get(&def_id).cloned() {
                                    // Create a mapping from TypeParam DefId to concrete type
                                    let mut type_arg_map: HashMap<DefId, Type> = HashMap::new();
                                    
                                    // Infer type arguments by matching params with args
                                    for (arg, param) in args_typed.iter().zip(params.iter()) {
                                        self.infer_type_args(param, &arg.ty, &mut type_arg_map);
                                    }
                                    
                                    // Build the type args vector in order
                                    let type_args: Vec<Type> = type_params.iter()
                                        .map(|tp_info| {
                                            type_arg_map.get(&tp_info.def_id).cloned().unwrap_or(Type::Error)
                                        })
                                        .collect();
                                    
                                    // Check trait bounds are satisfied
                                    for (tp_info, concrete_type) in type_params.iter().zip(type_args.iter()) {
                                        for &trait_def_id in &tp_info.bounds {
                                            if !self.type_implements_trait(concrete_type, trait_def_id) {
                                                let trait_name = self.ctx.get_type_name(trait_def_id)
                                                    .unwrap_or_else(|| format!("trait#{}", trait_def_id.0));
                                                self.error(
                                                    format!("type {} does not implement trait {}", 
                                                        concrete_type.display(&self.ctx), trait_name),
                                                    expr.span
                                                );
                                            }
                                        }
                                    }
                                    
                                    // Record the instantiation
                                    if !type_args.iter().any(|t| matches!(t, Type::Error)) {
                                        self.generic_instantiations.insert(GenericInstantiation {
                                            func_def_id: def_id,
                                            type_args: type_args.clone(),
                                        });
                                    }
                                    
                                    Some(type_args)
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            
                            // Check argument types (unification handles TypeParam)
                            for (i, (arg, param)) in args_typed.iter().zip(params.iter()).enumerate() {
                                if let Err(e) = self.ctx.unify(&arg.ty, param) {
                                    self.error(
                                        format!("argument {} type mismatch: {}", i + 1, e),
                                        expr.span
                                    );
                                }
                            }
                            
                            // Substitute type parameters in return type
                            let final_ret = if let Some(ref type_args) = inferred_type_args {
                                if let Some(def_id) = callee_def_id {
                                    if let Some(type_params) = self.generic_functions.get(&def_id) {
                                        // Extract just (DefId, String) pairs for substitution
                                        let tp_pairs: Vec<_> = type_params.iter()
                                            .map(|tp| (tp.def_id, tp.name.clone()))
                                            .collect();
                                        self.substitute_type_params(ret, &tp_pairs, type_args)
                                    } else {
                                        (**ret).clone()
                                    }
                                } else {
                                    (**ret).clone()
                                }
                            } else {
                                (**ret).clone()
                            };
                            
                            (final_ret, inferred_type_args)
                        }
                    }
                    Type::Error => (Type::Error, None),
                    _ => {
                        self.error(format!("cannot call non-function type"), expr.span);
                        (Type::Error, None)
                    }
                };
                
                // Create the call expression, including type args if this is a generic call
                let call_kind = if let Some(type_args) = type_args {
                    if let Some(def_id) = callee_def_id {
                        TypedExprKind::GenericCall {
                            func_def_id: def_id,
                            type_args,
                            args: args_typed,
                        }
                    } else {
                        TypedExprKind::Call {
                            callee: Box::new(callee_typed),
                            args: args_typed,
                        }
                    }
                } else {
                    TypedExprKind::Call {
                        callee: Box::new(callee_typed),
                        args: args_typed,
                    }
                };
                
                (call_kind, result_type)
            }
            
            ResolvedExprKind::Field { expr: base, field, field_span, .. } => {
                let base_typed = self.check_expr(base);
                
                let field_type = match &base_typed.ty {
                    Type::Struct(struct_id) => {
                        self.ctx.get_struct_field(*struct_id, field)
                            .cloned()
                            .unwrap_or_else(|| {
                                self.error(format!("no field '{}' on struct", field), expr.span);
                                Type::Error
                            })
                    }
                    Type::Ref { inner, .. } => {
                        // Auto-deref for field access
                        if let Type::Struct(struct_id) = inner.as_ref() {
                            self.ctx.get_struct_field(*struct_id, field)
                                .cloned()
                                .unwrap_or_else(|| {
                                    self.error(format!("no field '{}' on struct", field), expr.span);
                                    Type::Error
                                })
                        } else {
                            self.error(format!("cannot access field on non-struct type"), expr.span);
                            Type::Error
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error(format!("cannot access field on non-struct type"), expr.span);
                        Type::Error
                    }
                };
                
                (TypedExprKind::Field {
                    expr: Box::new(base_typed),
                    field: field.clone(),
                    field_span: *field_span,
                }, field_type)
            }
            
            ResolvedExprKind::StructLit { struct_def, fields } => {
                let struct_type = Type::Struct(*struct_def);
                
                // Check field types
                let mut typed_fields = Vec::new();
                for (name, name_span, field_expr) in fields {
                    let typed = self.check_expr(field_expr);
                    
                    if let Some(expected) = self.ctx.get_struct_field(*struct_def, name).cloned() {
                        if let Err(e) = self.ctx.unify(&typed.ty, &expected) {
                            self.error(format!("field '{}' type mismatch: {}", name, e), field_expr.span);
                        }
                        // Record the field type at the field name span for hover
                        self.ctx.record_span_type(name_span.start, name_span.end, format!("{}: {}", name, expected.display(&self.ctx)));
                    }
                    
                    typed_fields.push((name.clone(), typed));
                }
                
                (TypedExprKind::StructLit {
                    struct_def: *struct_def,
                    fields: typed_fields,
                }, struct_type)
            }
            
            ResolvedExprKind::If { cond, then_block, else_block } => {
                let cond_typed = self.check_expr(cond);
                
                // Condition must be bool
                if let Err(e) = self.ctx.unify(&cond_typed.ty, &Type::Bool) {
                    self.error(format!("if condition must be bool: {}", e), expr.span);
                }
                
                let then_typed = self.check_block(then_block, None);
                let then_ty = then_typed.ty.clone();
                
                let (else_typed, result_type) = match else_block {
                    Some(ResolvedElse::Block(b)) => {
                        let typed = self.check_block(b, Some(&then_ty));
                        let ty = typed.ty.clone();
                        (Some(TypedElse::Block(typed)), ty)
                    }
                    Some(ResolvedElse::If(e)) => {
                        let typed = self.check_expr(e);
                        let ty = typed.ty.clone();
                        (Some(TypedElse::If(Box::new(typed))), ty)
                    }
                    None => (None, Type::Unit),
                };
                
                // Unify then and else types
                if else_typed.is_some() {
                    if let Err(e) = self.ctx.unify(&then_ty, &result_type) {
                        self.error(format!("if/else type mismatch: {}", e), expr.span);
                    }
                }
                
                let final_ty = self.ctx.apply(&then_ty);
                
                (TypedExprKind::If {
                    cond: Box::new(cond_typed),
                    then_block: then_typed,
                    else_block: else_typed,
                }, final_ty)
            }
            
            ResolvedExprKind::While { cond, body } => {
                let cond_typed = self.check_expr(cond);
                
                if let Err(e) = self.ctx.unify(&cond_typed.ty, &Type::Bool) {
                    self.error(format!("while condition must be bool: {}", e), expr.span);
                }
                
                let body_typed = self.check_block(body, None);
                
                (TypedExprKind::While {
                    cond: Box::new(cond_typed),
                    body: body_typed,
                }, Type::Unit)
            }
            
            ResolvedExprKind::For { binding, binding_name, iter, body } => {
                // Check that iter is a range expression (Binary with Range operator)
                // For now, we only support `for x in start..end` syntax
                match &iter.kind {
                    ResolvedExprKind::Binary { left, op: wisp_ast::BinOp::Range, right } => {
                        let start_typed = self.check_expr(left);
                        let end_typed = self.check_expr(right);
                        
                        // Both start and end must be i32 (for now)
                        if let Err(e) = self.ctx.unify(&start_typed.ty, &Type::I32) {
                            self.error(format!("for loop range start must be i32: {}", e), left.span);
                        }
                        if let Err(e) = self.ctx.unify(&end_typed.ty, &Type::I32) {
                            self.error(format!("for loop range end must be i32: {}", e), right.span);
                        }
                        
                        // The binding has type i32
                        self.ctx.register_def_type(*binding, Type::I32);
                        
                        let body_typed = self.check_block(body, None);
                        
                        (TypedExprKind::For {
                            binding: *binding,
                            binding_name: binding_name.clone(),
                            start: Box::new(start_typed),
                            end: Box::new(end_typed),
                            body: body_typed,
                        }, Type::Unit)
                    }
                    _ => {
                        self.error("for loop currently only supports range expressions (start..end)".to_string(), iter.span);
                        (TypedExprKind::Error, Type::Error)
                    }
                }
            }
            
            ResolvedExprKind::Block(block) => {
                let typed = self.check_block(block, None);
                let ty = typed.ty.clone();
                (TypedExprKind::Block(typed), ty)
            }
            
            ResolvedExprKind::Assign { target, value } => {
                let target_typed = self.check_expr(target);
                let value_typed = self.check_expr(value);
                
                if let Err(e) = self.ctx.unify(&target_typed.ty, &value_typed.ty) {
                    self.error(format!("assignment type mismatch: {}", e), expr.span);
                }
                
                (TypedExprKind::Assign {
                    target: Box::new(target_typed),
                    value: Box::new(value_typed),
                }, Type::Unit)
            }
            
            ResolvedExprKind::Ref { is_mut, expr: inner } => {
                let inner_typed = self.check_expr(inner);
                let ref_type = Type::Ref {
                    is_mut: *is_mut,
                    inner: Box::new(inner_typed.ty.clone()),
                };
                
                (TypedExprKind::Ref {
                    is_mut: *is_mut,
                    expr: Box::new(inner_typed),
                }, ref_type)
            }
            
            ResolvedExprKind::Deref(inner) => {
                let inner_typed = self.check_expr(inner);
                
                let result_type = match &inner_typed.ty {
                    Type::Ref { inner, .. } => (**inner).clone(),
                    Type::Error => Type::Error,
                    _ => {
                        self.error("cannot dereference non-reference type".to_string(), expr.span);
                        Type::Error
                    }
                };
                
                (TypedExprKind::Deref(Box::new(inner_typed)), result_type)
            }
            
            ResolvedExprKind::Match { scrutinee, arms } => {
                let scrutinee_typed = self.check_expr(scrutinee);
                
                let mut result_type = self.ctx.fresh_var();
                let mut typed_arms = Vec::new();
                
                for arm in arms {
                    let typed_arm = self.check_match_arm(arm, &scrutinee_typed.ty);
                    if let Err(e) = self.ctx.unify(&typed_arm.body.ty, &result_type) {
                        self.error(format!("match arm type mismatch: {}", e), arm.span);
                    }
                    result_type = self.ctx.apply(&result_type);
                    typed_arms.push(typed_arm);
                }
                
                (TypedExprKind::Match {
                    scrutinee: Box::new(scrutinee_typed),
                    arms: typed_arms,
                }, self.ctx.apply(&result_type))
            }
            
            ResolvedExprKind::Index { expr: base, index } => {
                let base_typed = self.check_expr(base);
                let index_typed = self.check_expr(index);
                
                // Index must be integer
                if !index_typed.ty.is_integer() && !matches!(index_typed.ty, Type::Error) {
                    self.error("index must be an integer".to_string(), expr.span);
                }
                
                let elem_type = match &base_typed.ty {
                    Type::Slice(elem) => (**elem).clone(),
                    Type::Array(elem, _) => (**elem).clone(),
                    Type::Ref { inner, .. } => {
                        match inner.as_ref() {
                            Type::Slice(elem) => (**elem).clone(),
                            Type::Array(elem, _) => (**elem).clone(),
                            _ => {
                                self.error("cannot index non-array type".to_string(), expr.span);
                                Type::Error
                            }
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.error("cannot index non-array type".to_string(), expr.span);
                        Type::Error
                    }
                };
                
                (TypedExprKind::Index {
                    expr: Box::new(base_typed),
                    index: Box::new(index_typed),
                }, elem_type)
            }
            
            ResolvedExprKind::ArrayLit(elements) => {
                if elements.is_empty() {
                    self.error("cannot infer type of empty array literal".to_string(), expr.span);
                    (TypedExprKind::ArrayLit(vec![]), Type::Error)
                } else {
                    // Type check all elements
                    let typed_elements: Vec<_> = elements.iter().map(|e| self.check_expr(e)).collect();
                    
                    // All elements must have the same type
                    let elem_type = typed_elements[0].ty.clone();
                    for (i, elem) in typed_elements.iter().enumerate().skip(1) {
                        if let Err(e) = self.ctx.unify(&elem_type, &elem.ty) {
                            self.error(format!("array element {} has wrong type: {}", i, e), elem.span);
                        }
                    }
                    
                    let len = typed_elements.len();
                    let array_type = Type::Array(Box::new(self.ctx.apply(&elem_type)), len);
                    
                    (TypedExprKind::ArrayLit(typed_elements), array_type)
                }
            }
            
            ResolvedExprKind::Lambda { params, body } => {
                // Type check the lambda
                // For now, we require type annotations on parameters (no inference)
                let mut param_types = Vec::new();
                let mut typed_params = Vec::new();
                
                for p in params {
                    let param_ty = if let Some(ty) = &p.ty {
                        self.resolve_type(ty)
                    } else {
                        // No type annotation - use a fresh type variable for inference
                        self.ctx.fresh_var()
                    };
                    
                    self.ctx.register_def_type(p.def_id, param_ty.clone());
                    param_types.push(param_ty.clone());
                    typed_params.push(TypedLambdaParam {
                        def_id: p.def_id,
                        name: p.name.clone(),
                        ty: param_ty,
                        span: p.span,
                    });
                }
                
                let body_typed = self.check_expr(body);
                let ret_type = body_typed.ty.clone();
                
                let fn_type = Type::Function {
                    params: param_types,
                    ret: Box::new(ret_type),
                };
                
                (TypedExprKind::Lambda {
                    params: typed_params,
                    body: Box::new(body_typed),
                }, fn_type)
            }
            
            ResolvedExprKind::Cast { expr, target_type } => {
                let typed_expr = self.check_expr(expr);
                let target = self.resolve_type(target_type);
                
                // Check if the cast is valid
                // For now, allow casts between numeric types and pointer types
                let from_ty = self.ctx.apply(&typed_expr.ty);
                let to_ty = self.ctx.apply(&target);
                
                let valid = self.is_valid_cast(&from_ty, &to_ty);
                if !valid {
                    self.error(format!("cannot cast {} to {}", from_ty.display(&self.ctx), to_ty.display(&self.ctx)), expr.span);
                }
                
                (TypedExprKind::Cast {
                    expr: Box::new(typed_expr),
                    target_type: target.clone(),
                }, target)
            }
            
            ResolvedExprKind::NamespacePath(path) => {
                // This is an intermediate state that should be resolved during field access
                // If we get here, it means we have something like `std.io` without a final member access
                self.error(
                    format!("namespace path '{}' cannot be used as a value", path.join(".")),
                    expr.span
                );
                (TypedExprKind::Error, Type::Error)
            }
            
            ResolvedExprKind::Error => (TypedExprKind::Error, Type::Error),
            
            ResolvedExprKind::StringInterp { parts } => {
                // Desugar string interpolation into a chain of + operations
                // "hello {name}!" becomes:
                //   String.from("hello ") + name.to_string() + String.from("!")
                
                // Get the String type
                let string_type = self.ctx.lookup_type_by_name("String")
                    .unwrap_or(Type::Str);
                
                // Convert each part to a TypedExpr that produces a String
                let mut string_exprs: Vec<TypedExpr> = Vec::new();
                
                for part in parts {
                    match part {
                        wisp_hir::ResolvedStringInterpPart::Literal(s) => {
                            // Create String.from("literal")
                            // For now, we'll create a StringLiteral and let it be converted
                            // Actually, we need to call String.from() - but that requires
                            // looking up the method. For simplicity, store as StringLiteral
                            // and handle in MIR lowering
                            string_exprs.push(TypedExpr {
                                kind: TypedExprKind::StringLiteral(s.clone()),
                                ty: Type::Str,
                                span: expr.span,
                            });
                        }
                        wisp_hir::ResolvedStringInterpPart::Expr(e) => {
                            let typed_expr = self.check_expr(e);
                            
                            // If the expression is already a String, use it directly
                            // Otherwise, we need to call .to_string() on it
                            if typed_expr.ty == string_type {
                                string_exprs.push(typed_expr);
                            } else {
                                // Need to call .to_string() - create a method call
                                // Check if the type implements Display
                                let display_trait = self.trait_by_name.get("Display").copied();
                                
                                if let Some(trait_id) = display_trait {
                                    // Create a reference to the expression for &self
                                    let ref_expr = TypedExpr {
                                        kind: TypedExprKind::Ref {
                                            is_mut: false,
                                            expr: Box::new(typed_expr.clone()),
                                        },
                                        ty: Type::Ref {
                                            is_mut: false,
                                            inner: Box::new(typed_expr.ty.clone()),
                                        },
                                        span: expr.span,
                                    };
                                    
                                    // Create the to_string() method call
                                    let to_string_call = TypedExpr {
                                        kind: TypedExprKind::TraitMethodCall {
                                            receiver: Box::new(ref_expr),
                                            method: "to_string".to_string(),
                                            method_span: expr.span,
                                            is_mut_self: false, // to_string takes &self, not &mut self
                                            trait_bounds: vec![trait_id],
                                            args: vec![],
                                        },
                                        ty: string_type.clone(),
                                        span: expr.span,
                                    };
                                    
                                    string_exprs.push(to_string_call);
                                } else {
                                    // No Display trait found, just use the expression
                                    // This will likely fail at runtime
                                    self.error(
                                        format!("type {} does not implement Display for string interpolation", 
                                            typed_expr.ty.display(&self.ctx)),
                                        expr.span
                                    );
                                    string_exprs.push(typed_expr);
                                }
                            }
                        }
                    }
                }
                
                // Now chain all parts with + operator
                if string_exprs.is_empty() {
                    // Empty interpolation - return empty string
                    (TypedExprKind::StringLiteral(String::new()), Type::Str)
                } else if string_exprs.len() == 1 {
                    // Single part - just return it
                    let single = string_exprs.pop().unwrap();
                    (single.kind, single.ty)
                } else {
                    // Multiple parts - chain with +
                    // We need to wrap str literals in String.from() for the + operator
                    // Build: part1 + part2 + part3 + ...
                    
                    let mut result = string_exprs.remove(0);
                    
                    // If first part is str, wrap in String.from()
                    if result.ty == Type::Str {
                        result = self.wrap_str_in_string_from(result, expr.span);
                    }
                    
                    for part in string_exprs {
                        let mut rhs = part;
                        
                        // If rhs is str, wrap in String.from()
                        if rhs.ty == Type::Str {
                            rhs = self.wrap_str_in_string_from(rhs, expr.span);
                        }
                        
                        // Look up the Add trait implementation for String
                        let add_trait = self.trait_by_name.get("Add").copied();
                        
                        if let Some(_trait_id) = add_trait {
                            if let Some((method_def_id, _)) = self.find_op_trait_method(&string_type, "Add", "add") {
                                // Create the + operation as a method call
                                // Note: Add::add takes self by value, not &mut self
                                result = TypedExpr {
                                    kind: TypedExprKind::MethodCall {
                                        receiver: Box::new(result),
                                        method: "add".to_string(),
                                        method_def_id,
                                        method_span: expr.span,
                                        is_mut_self: false,
                                        args: vec![rhs],
                                    },
                                    ty: string_type.clone(),
                                    span: expr.span,
                                };
                            } else {
                                // Fallback: use binary op (won't work for String)
                                result = TypedExpr {
                                    kind: TypedExprKind::Binary {
                                        left: Box::new(result),
                                        op: wisp_ast::BinOp::Add,
                                        right: Box::new(rhs),
                                    },
                                    ty: string_type.clone(),
                                    span: expr.span,
                                };
                            }
                        } else {
                            // No Add trait, use binary op
                            result = TypedExpr {
                                kind: TypedExprKind::Binary {
                                    left: Box::new(result),
                                    op: wisp_ast::BinOp::Add,
                                    right: Box::new(rhs),
                                },
                                ty: string_type.clone(),
                                span: expr.span,
                            };
                        }
                    }
                    
                    (result.kind, result.ty)
                }
            }
        };

        // Record span→type mapping for LSP hover support
        // Format the type nicely for variables
        let type_str = match &kind {
            TypedExprKind::Var { name, .. } => format!("{}: {}", name, ty.display(&self.ctx)),
            _ => ty.display(&self.ctx),
        };
        self.ctx.record_span_type(expr.span.start, expr.span.end, type_str);

        TypedExpr { kind, ty, span: expr.span }
    }
    
    /// Map binary operators to (trait name, method name) for operator overloading
    fn op_to_trait(op: wisp_ast::BinOp) -> Option<(&'static str, &'static str)> {
        use wisp_ast::BinOp;
        match op {
            // Arithmetic operators
            BinOp::Add => Some(("Add", "add")),
            BinOp::Sub => Some(("Sub", "sub")),
            BinOp::Mul => Some(("Mul", "mul")),
            BinOp::Div => Some(("Div", "div")),
            BinOp::Mod => Some(("Rem", "rem")),
            // Comparison operators
            BinOp::Eq => Some(("PartialEq", "eq")),
            BinOp::Lt => Some(("PartialLt", "lt")),
            BinOp::Gt => Some(("PartialGt", "gt")),
            BinOp::LtEq => Some(("PartialLe", "le")),
            BinOp::GtEq => Some(("PartialGe", "ge")),
            // NotEq is handled specially (negate eq result)
            // Logical ops don't use trait overloading
            _ => None,
        }
    }
    
    /// Try to find an operator trait implementation for the given type
    fn find_op_trait_method(&self, ty: &Type, trait_name: &str, method_name: &str) -> Option<(DefId, Type)> {
        // Get the trait DefId
        let trait_def_id = self.trait_by_name.get(trait_name)?;
        
        // Special handling for type parameters: look up method from trait definition
        if let Type::TypeParam(param_def_id, _) = ty {
            // Get the trait bounds for this type parameter
            if let Some(bounds) = self.find_type_param_bounds(*param_def_id) {
                // Check if this trait is in the bounds
                if bounds.contains(trait_def_id) {
                    // Look up the method from the trait definition
                    if let Some(method_sigs) = self.trait_methods.get(trait_def_id) {
                        for (name, method_type) in method_sigs {
                            if name == method_name {
                                // Return a dummy DefId (we don't have a concrete impl yet)
                                // and the method type with Self substituted for the type parameter
                                return Some((*trait_def_id, method_type.clone()));
                            }
                        }
                    }
                }
            }
            return None;
        }
        
        // Get the type's DefId for concrete types
        let type_def_id = match ty {
            Type::Struct(id) => Some(*id),
            Type::Enum(id) => Some(*id),
            _ => None,
        }?;
        
        // Check if this type implements the trait
        let impl_methods = self.trait_impls.get(&(type_def_id, *trait_def_id))?;
        
        // Find the method
        for (name, def_id, method_type) in impl_methods {
            if name == method_name {
                return Some((*def_id, method_type.clone()));
            }
        }
        
        None
    }
    
    /// Wrap a str expression in String.from() call
    fn wrap_str_in_string_from(&self, str_expr: TypedExpr, span: Span) -> TypedExpr {
        // Look up String type and String.from associated function
        let string_type = self.ctx.lookup_type_by_name("String")
            .unwrap_or(Type::Str);
        
        let string_def_id = match &string_type {
            Type::Struct(id) => Some(*id),
            _ => None,
        };
        
        if let Some(struct_id) = string_def_id {
            // Look up String.from
            if let Some((fn_def_id, _fn_type)) = self.associated_functions.get(&(struct_id, "from".to_string())) {
                // Create String.from(str_expr) call
                return TypedExpr {
                    kind: TypedExprKind::AssociatedFunctionCall {
                        type_id: struct_id,
                        function: "from".to_string(),
                        function_def_id: *fn_def_id,
                        function_span: span,
                        args: vec![str_expr],
                    },
                    ty: string_type,
                    span,
                };
            }
        }
        
        // Fallback: just return the str expr (will fail at runtime)
        str_expr
    }

    fn check_binary_op(&mut self, op: wisp_ast::BinOp, left: &Type, right: &Type, span: Span) -> Type {
        use wisp_ast::BinOp;
        
        match op {
            // Arithmetic
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if let Err(e) = self.ctx.unify(left, right) {
                    self.error(format!("arithmetic type mismatch: {}", e), span);
                    return Type::Error;
                }
                // Allow numeric types, type variables, type parameters (which will be checked via trait bounds), and error types
                if !left.is_numeric() && !matches!(left, Type::Var(_) | Type::TypeParam(_, _) | Type::Error) {
                    self.error("arithmetic requires numeric types".to_string(), span);
                    return Type::Error;
                }
                self.ctx.apply(left)
            }
            // Comparison
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                if let Err(e) = self.ctx.unify(left, right) {
                    self.error(format!("comparison type mismatch: {}", e), span);
                }
                Type::Bool
            }
            // Logical
            BinOp::And | BinOp::Or => {
                if let Err(e) = self.ctx.unify(left, &Type::Bool) {
                    self.error(format!("logical operator requires bool: {}", e), span);
                }
                if let Err(e) = self.ctx.unify(right, &Type::Bool) {
                    self.error(format!("logical operator requires bool: {}", e), span);
                }
                Type::Bool
            }
            // Range (used in for loops)
            BinOp::Range => {
                // Range is only valid in for loop context, which is handled specially
                // If we get here, it's an error
                self.error("range operator (..) can only be used in for loops".to_string(), span);
                Type::Error
            }
        }
    }

    fn check_unary_op(&mut self, op: wisp_ast::UnaryOp, inner: &Type, span: Span) -> Type {
        use wisp_ast::UnaryOp;
        
        match op {
            UnaryOp::Neg => {
                if !inner.is_numeric() && !matches!(inner, Type::Var(_) | Type::Error) {
                    self.error("negation requires numeric type".to_string(), span);
                    Type::Error
                } else {
                    inner.clone()
                }
            }
            UnaryOp::Not => {
                if let Err(e) = self.ctx.unify(inner, &Type::Bool) {
                    self.error(format!("logical not requires bool: {}", e), span);
                }
                Type::Bool
            }
        }
    }

    fn check_match_arm(&mut self, arm: &ResolvedMatchArm, scrutinee_type: &Type) -> TypedMatchArm {
        // TODO: proper pattern type checking
        let pattern = self.check_pattern(&arm.pattern, scrutinee_type);
        let body = self.check_expr(&arm.body);
        
        TypedMatchArm {
            pattern,
            body,
        }
    }

    fn check_pattern(&mut self, pattern: &ResolvedPattern, expected: &Type) -> TypedPattern {
        match &pattern.kind {
            ResolvedPatternKind::Wildcard => TypedPattern::Wildcard,
            ResolvedPatternKind::Binding { def_id, name } => {
                self.ctx.register_def_type(*def_id, expected.clone());
                TypedPattern::Binding { def_id: *def_id, name: name.clone(), ty: expected.clone() }
            }
            ResolvedPatternKind::Literal(expr) => {
                let typed = self.check_expr(expr);
                TypedPattern::Literal(typed)
            }
            ResolvedPatternKind::Variant { variant_def, fields } => {
                // TODO: check variant fields match
                let mut typed_fields = Vec::new();
                for p in fields {
                    let var = self.ctx.fresh_var();
                    typed_fields.push(self.check_pattern(p, &var));
                }
                TypedPattern::Variant {
                    variant_def: *variant_def,
                    fields: typed_fields,
                }
            }
        }
    }
    
    /// Infer type arguments by matching a parameter type with an argument type
    fn infer_type_args(&self, param_type: &Type, arg_type: &Type, map: &mut HashMap<DefId, Type>) {
        match param_type {
            Type::TypeParam(def_id, _) => {
                // If this type param isn't already inferred, bind it to the arg type
                if !map.contains_key(def_id) {
                    map.insert(*def_id, arg_type.clone());
                }
            }
            Type::Ref { inner, .. } => {
                if let Type::Ref { inner: arg_inner, .. } = arg_type {
                    self.infer_type_args(inner, arg_inner, map);
                }
            }
            Type::Slice(elem) => {
                if let Type::Slice(arg_elem) = arg_type {
                    self.infer_type_args(elem, arg_elem, map);
                }
            }
            Type::Array(elem, _) => {
                if let Type::Array(arg_elem, _) = arg_type {
                    self.infer_type_args(elem, arg_elem, map);
                }
            }
            Type::Tuple(elems) => {
                if let Type::Tuple(arg_elems) = arg_type {
                    for (e, a) in elems.iter().zip(arg_elems.iter()) {
                        self.infer_type_args(e, a, map);
                    }
                }
            }
            Type::Function { params, ret } => {
                if let Type::Function { params: arg_params, ret: arg_ret } = arg_type {
                    for (p, a) in params.iter().zip(arg_params.iter()) {
                        self.infer_type_args(p, a, map);
                    }
                    self.infer_type_args(ret, arg_ret, map);
                }
            }
            _ => {}
        }
    }
    
    /// Substitute type parameters with concrete types
    fn substitute_type_params(&self, ty: &Type, type_params: &[(DefId, String)], type_args: &[Type]) -> Type {
        match ty {
            Type::TypeParam(def_id, _) => {
                // Find the index of this type param
                for (i, (tp_def_id, _)) in type_params.iter().enumerate() {
                    if tp_def_id == def_id {
                        return type_args.get(i).cloned().unwrap_or(ty.clone());
                    }
                }
                ty.clone()
            }
            Type::Ref { is_mut, inner } => Type::Ref {
                is_mut: *is_mut,
                inner: Box::new(self.substitute_type_params(inner, type_params, type_args)),
            },
            Type::Slice(elem) => Type::Slice(Box::new(self.substitute_type_params(elem, type_params, type_args))),
            Type::Array(elem, size) => Type::Array(
                Box::new(self.substitute_type_params(elem, type_params, type_args)),
                *size,
            ),
            Type::Tuple(elems) => Type::Tuple(
                elems.iter().map(|e| self.substitute_type_params(e, type_params, type_args)).collect()
            ),
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(|p| self.substitute_type_params(p, type_params, type_args)).collect(),
                ret: Box::new(self.substitute_type_params(ret, type_params, type_args)),
            },
            _ => ty.clone(),
        }
    }
    
    /// Get the collected generic instantiations
    pub fn get_generic_instantiations(&self) -> &HashSet<GenericInstantiation> {
        &self.generic_instantiations
    }
    
    /// Check if a type implements a trait
    fn type_implements_trait(&self, ty: &Type, trait_def_id: DefId) -> bool {
        match ty {
            Type::Struct(struct_def_id) => {
                // Check if there's an impl for this (struct, trait) pair
                self.trait_impls.contains_key(&(*struct_def_id, trait_def_id))
            }
            Type::Enum(enum_def_id) => {
                self.trait_impls.contains_key(&(*enum_def_id, trait_def_id))
            }
            // Numeric primitives implicitly implement operator traits
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64 => {
                // Check if this is an operator trait
                if let Some(trait_name) = self.ctx.get_type_name(trait_def_id) {
                    match trait_name.as_str() {
                        "Add" | "Sub" | "Mul" | "Div" | "Rem" | 
                        "BitAnd" | "BitOr" | "BitXor" | "Shl" | "Shr" |
                        "Neg" | "Not" => return true,
                        _ => {}
                    }
                }
                // Otherwise check explicit impls
                let type_name = ty.display(&self.ctx);
                self.primitive_trait_impls.contains(&(type_name, trait_def_id))
            }
            // Bool implicitly implements BitAnd, BitOr, BitXor, Not
            Type::Bool => {
                if let Some(trait_name) = self.ctx.get_type_name(trait_def_id) {
                    match trait_name.as_str() {
                        "BitAnd" | "BitOr" | "BitXor" | "Not" => return true,
                        _ => {}
                    }
                }
                self.primitive_trait_impls.contains(&("bool".to_string(), trait_def_id))
            }
            Type::Str => self.primitive_trait_impls.contains(&("str".to_string(), trait_def_id)),
            Type::Ref { inner, .. } => self.type_implements_trait(inner, trait_def_id),
            _ => false,
        }
    }
    
    /// Look up a method on a type parameter via its trait bounds
    fn lookup_trait_method(&self, type_param_bounds: &[DefId], method_name: &str) -> Option<Type> {
        for &trait_def_id in type_param_bounds {
            if let Some(methods) = self.trait_methods.get(&trait_def_id) {
                for (name, method_type) in methods {
                    if name == method_name {
                        return Some(method_type.clone());
                    }
                }
            }
        }
        None
    }
    
    /// Find the trait bounds for a type parameter by its DefId
    fn find_type_param_bounds(&self, type_param_def_id: DefId) -> Option<Vec<DefId>> {
        // Search through all generic functions for this type param
        for type_params in self.generic_functions.values() {
            for tp_info in type_params {
                if tp_info.def_id == type_param_def_id && !tp_info.bounds.is_empty() {
                    return Some(tp_info.bounds.clone());
                }
            }
        }
        None
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// === Typed AST ===

#[derive(Debug)]
pub struct TypedProgram {
    pub ctx: TypeContext,
    pub structs: Vec<ResolvedStruct>,
    pub enums: Vec<ResolvedEnum>,
    pub functions: Vec<TypedFunction>,
    pub extern_functions: Vec<TypedExternFunction>,
    pub extern_statics: Vec<TypedExternStatic>,
    pub impls: Vec<TypedImpl>,
    /// Collected generic instantiations for monomorphization
    pub generic_instantiations: HashSet<GenericInstantiation>,
}

/// Typed extern function declaration
#[derive(Debug)]
pub struct TypedExternFunction {
    pub def_id: DefId,
    pub name: String,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
}

/// Typed extern static declaration
#[derive(Debug)]
pub struct TypedExternStatic {
    pub def_id: DefId,
    pub name: String,
    pub ty: Type,
}

impl TypedProgram {
    // === LSP Query Methods ===
    
    /// Get type string at a given offset (for hover)
    pub fn type_at_offset(&self, offset: usize) -> Option<&String> {
        self.ctx.type_at_offset(offset)
    }
    
    /// Get definition DefId at a given offset (for go-to-definition)
    pub fn definition_at_offset(&self, offset: usize) -> Option<DefId> {
        self.ctx.definition_at_offset(offset)
    }
    
    /// Get the span of a definition by DefId
    pub fn definition_span(&self, def_id: DefId) -> Option<Span> {
        // Check functions
        for f in &self.functions {
            if f.def_id == def_id {
                return Some(f.span);
            }
            for p in &f.params {
                if p.def_id == def_id {
                    return Some(p.span);
                }
            }
        }
        // Check impl methods
        for imp in &self.impls {
            for m in &imp.methods {
                if m.def_id == def_id {
                    return Some(m.span);
                }
                for p in &m.params {
                    if p.def_id == def_id {
                        return Some(p.span);
                    }
                }
            }
        }
        // Check structs
        for s in &self.structs {
            if s.def_id == def_id {
                return Some(s.span);
            }
            for f in &s.fields {
                if f.def_id == def_id {
                    return Some(f.span);
                }
            }
        }
        // Check enums
        for e in &self.enums {
            if e.def_id == def_id {
                return Some(e.span);
            }
        }
        // Check extern functions
        for f in &self.extern_functions {
            if f.def_id == def_id {
                // Extern functions don't have spans stored, return None
                return None;
            }
        }
        None
    }
    
    /// Get function signature by name (for hover on function calls)
    pub fn get_function_signature(&self, name: &str) -> Option<String> {
        for f in &self.functions {
            if f.name == name {
                let params: Vec<_> = f.params.iter()
                    .map(|p| format!("{}: {}", p.name, p.ty.display(&self.ctx)))
                    .collect();
                return Some(format!("fn {}({}) -> {}", f.name, params.join(", "), f.return_type.display(&self.ctx)));
            }
        }
        for imp in &self.impls {
            for m in &imp.methods {
                if m.name == name {
                    let params: Vec<_> = m.params.iter()
                        .map(|p| format!("{}: {}", p.name, p.ty.display(&self.ctx)))
                        .collect();
                    return Some(format!("fn {}({}) -> {}", m.name, params.join(", "), m.return_type.display(&self.ctx)));
                }
            }
        }
        None
    }
    
    /// Get struct definition string (for hover on struct names)
    pub fn get_struct_definition(&self, name: &str) -> Option<String> {
        for s in &self.structs {
            if s.name == name {
                if let Some(fields) = self.ctx.get_struct_fields(s.def_id) {
                    let field_strs: Vec<_> = fields.iter()
                        .map(|(n, t)| format!("    {}: {}", n, t.display(&self.ctx)))
                        .collect();
                    return Some(format!("struct {} {{\n{}\n}}", s.name, field_strs.join(",\n")));
                }
            }
        }
        None
    }
    
    /// Get all span types (for iteration)
    pub fn all_span_types(&self) -> &std::collections::HashMap<(usize, usize), String> {
        self.ctx.all_span_types()
    }
    
    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        
        out.push_str("=== Typed Program ===\n\n");
        
        out.push_str("--- Structs ---\n");
        for s in &self.structs {
            out.push_str(&format!("  struct {} {{\n", s.name));
            if let Some(fields) = self.ctx.get_struct_fields(s.def_id) {
                for (name, ty) in fields {
                    out.push_str(&format!("    {}: {}\n", name, ty.display(&self.ctx)));
                }
            }
            out.push_str("  }\n");
        }
        
        out.push_str("\n--- Enums ---\n");
        for e in &self.enums {
            out.push_str(&format!("  enum {} {{\n", e.name));
            if let Some(variants) = self.ctx.get_enum_variants(e.def_id) {
                for (name, _, fields) in variants {
                    if fields.is_empty() {
                        out.push_str(&format!("    {}\n", name));
                    } else {
                        let field_strs: Vec<_> = fields.iter().map(|t| t.display(&self.ctx)).collect();
                        out.push_str(&format!("    {}({})\n", name, field_strs.join(", ")));
                    }
                }
            }
            out.push_str("  }\n");
        }
        
        out.push_str("\n--- Functions ---\n");
        for f in &self.functions {
            let params: Vec<_> = f.params.iter()
                .map(|p| format!("{}: {}", p.name, p.ty.display(&self.ctx)))
                .collect();
            out.push_str(&format!("  fn {}({}) -> {}\n", 
                f.name, 
                params.join(", "),
                f.return_type.display(&self.ctx)
            ));
        }
        
        out.push_str("\n--- Impls ---\n");
        for i in &self.impls {
            out.push_str(&format!("  impl {}\n", i.target_type.display(&self.ctx)));
            for m in &i.methods {
                let params: Vec<_> = m.params.iter()
                    .map(|p| format!("{}: {}", p.name, p.ty.display(&self.ctx)))
                    .collect();
                out.push_str(&format!("    fn {}({}) -> {}\n",
                    m.name,
                    params.join(", "),
                    m.return_type.display(&self.ctx)
                ));
            }
        }
        
        out
    }
}

#[derive(Debug)]
pub struct TypedImpl {
    pub trait_def: Option<DefId>,
    pub target_type: Type,
    pub methods: Vec<TypedFunction>,
}

#[derive(Debug)]
pub struct TypedFunction {
    pub def_id: DefId,
    pub name: String,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: Option<TypedBlock>,
    pub span: Span,
    /// Span of just the function name (for hover)
    pub name_span: Span,
}

#[derive(Debug)]
pub struct TypedParam {
    pub def_id: DefId,
    pub name: String,
    pub is_mut: bool,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypedBlock {
    pub stmts: Vec<TypedStmt>,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub enum TypedStmt {
    Let {
        def_id: DefId,
        name: String,
        is_mut: bool,
        ty: Type,
        init: Option<TypedExpr>,
        span: Span,
    },
    Expr(TypedExpr),
}

#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedExprKind {
    IntLiteral(i64),
    FloatLiteral(f64),
    BoolLiteral(bool),
    StringLiteral(String),
    Var { name: String, def_id: DefId },
    Binary { left: Box<TypedExpr>, op: wisp_ast::BinOp, right: Box<TypedExpr> },
    /// Operator call desugared from binary expression (e.g., `a + b` -> `a.add(b)`)
    OperatorCall { method_def_id: DefId, method_name: String, left: Box<TypedExpr>, right: Box<TypedExpr> },
    Unary { op: wisp_ast::UnaryOp, expr: Box<TypedExpr> },
    Call { callee: Box<TypedExpr>, args: Vec<TypedExpr> },
    /// Call to a generic function with inferred type arguments
    GenericCall { func_def_id: DefId, type_args: Vec<Type>, args: Vec<TypedExpr> },
    MethodCall { receiver: Box<TypedExpr>, method: String, method_def_id: DefId, method_span: Span, is_mut_self: bool, args: Vec<TypedExpr> },
    /// Method call on a type parameter via trait bounds
    TraitMethodCall { receiver: Box<TypedExpr>, method: String, method_span: Span, is_mut_self: bool, trait_bounds: Vec<DefId>, args: Vec<TypedExpr> },
    /// Associated function call: Type.function(args) where function has no self
    AssociatedFunctionCall { type_id: DefId, function: String, function_def_id: DefId, function_span: Span, args: Vec<TypedExpr> },
    /// Method call on a primitive type (i32, bool, str, etc.)
    PrimitiveMethodCall { receiver: Box<TypedExpr>, method: String, method_def_id: DefId, method_span: Span, is_mut_self: bool, args: Vec<TypedExpr> },
    Field { expr: Box<TypedExpr>, field: String, field_span: Span },
    StructLit { struct_def: DefId, fields: Vec<(String, TypedExpr)> },
    If { cond: Box<TypedExpr>, then_block: TypedBlock, else_block: Option<TypedElse> },
    While { cond: Box<TypedExpr>, body: TypedBlock },
    For { binding: DefId, binding_name: String, start: Box<TypedExpr>, end: Box<TypedExpr>, body: TypedBlock },
    Block(TypedBlock),
    Assign { target: Box<TypedExpr>, value: Box<TypedExpr> },
    Ref { is_mut: bool, expr: Box<TypedExpr> },
    Deref(Box<TypedExpr>),
    Match { scrutinee: Box<TypedExpr>, arms: Vec<TypedMatchArm> },
    Index { expr: Box<TypedExpr>, index: Box<TypedExpr> },
    ArrayLit(Vec<TypedExpr>),
    Lambda { params: Vec<TypedLambdaParam>, body: Box<TypedExpr> },
    Cast { expr: Box<TypedExpr>, target_type: Type },
    StringInterp { parts: Vec<TypedStringInterpPart> },
    Error,
}

/// Part of an interpolated string (typed)
#[derive(Debug, Clone)]
pub enum TypedStringInterpPart {
    Literal(String),
    Expr(TypedExpr),
}

#[derive(Debug, Clone)]
pub struct TypedLambdaParam {
    pub def_id: DefId,
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypedElse {
    Block(TypedBlock),
    If(Box<TypedExpr>),
}

#[derive(Debug, Clone)]
pub struct TypedMatchArm {
    pub pattern: TypedPattern,
    pub body: TypedExpr,
}

#[derive(Debug, Clone)]
pub enum TypedPattern {
    Wildcard,
    Binding { def_id: DefId, name: String, ty: Type },
    Literal(TypedExpr),
    Variant { variant_def: DefId, fields: Vec<TypedPattern> },
}

