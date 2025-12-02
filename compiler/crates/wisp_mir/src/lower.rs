//! Lower typed HIR to MIR

use crate::mir::*;
use wisp_hir::DefId;
use wisp_types::*;
use std::collections::HashMap;

/// Generate a mangled name for a monomorphized generic function
fn mangle_generic_name(base_name: &str, type_args: &[Type]) -> String {
    let type_strs: Vec<String> = type_args.iter().map(|t| mangle_type(t)).collect();
    format!("{}<{}>", base_name, type_strs.join(","))
}

/// Check if a type contains any type parameters
fn has_type_param(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_, _) => true,
        Type::Ref { inner, .. } => has_type_param(inner),
        Type::Slice(elem) => has_type_param(elem),
        Type::Array(elem, _) => has_type_param(elem),
        Type::Tuple(elems) => elems.iter().any(has_type_param),
        Type::Function { params, ret } => {
            params.iter().any(has_type_param) || has_type_param(ret)
        }
        _ => false,
    }
}

/// Substitute type parameters with concrete types
fn substitute_type(ty: &Type, type_args: &[Type], type_param_ids: &[DefId]) -> Type {
    match ty {
        Type::TypeParam(def_id, _) => {
            // Find the index of this type param
            for (i, tp_id) in type_param_ids.iter().enumerate() {
                if tp_id == def_id {
                    return type_args.get(i).cloned().unwrap_or(ty.clone());
                }
            }
            ty.clone()
        }
        Type::Ref { is_mut, inner } => Type::Ref {
            is_mut: *is_mut,
            inner: Box::new(substitute_type(inner, type_args, type_param_ids)),
        },
        Type::Slice(elem) => Type::Slice(Box::new(substitute_type(elem, type_args, type_param_ids))),
        Type::Array(elem, size) => Type::Array(
            Box::new(substitute_type(elem, type_args, type_param_ids)),
            *size,
        ),
        Type::Tuple(elems) => Type::Tuple(
            elems.iter().map(|e| substitute_type(e, type_args, type_param_ids)).collect()
        ),
        Type::Function { params, ret } => Type::Function {
            params: params.iter().map(|p| substitute_type(p, type_args, type_param_ids)).collect(),
            ret: Box::new(substitute_type(ret, type_args, type_param_ids)),
        },
        _ => ty.clone(),
    }
}

/// Mangle a type into a string suitable for function names
fn mangle_type(ty: &Type) -> String {
    match ty {
        Type::I8 => "i8".to_string(),
        Type::I16 => "i16".to_string(),
        Type::I32 => "i32".to_string(),
        Type::I64 => "i64".to_string(),
        Type::I128 => "i128".to_string(),
        Type::U8 => "u8".to_string(),
        Type::U16 => "u16".to_string(),
        Type::U32 => "u32".to_string(),
        Type::U64 => "u64".to_string(),
        Type::U128 => "u128".to_string(),
        Type::F32 => "f32".to_string(),
        Type::F64 => "f64".to_string(),
        Type::Bool => "bool".to_string(),
        Type::Char => "char".to_string(),
        Type::String => "String".to_string(),
        Type::Str => "str".to_string(),
        Type::Unit => "unit".to_string(),
        Type::Never => "never".to_string(),
        Type::Struct(def_id) => format!("S{}", def_id.0),
        Type::Enum(def_id) => format!("E{}", def_id.0),
        Type::Ref { is_mut, inner } => {
            let m = if *is_mut { "m" } else { "" };
            format!("R{}{}", m, mangle_type(inner))
        }
        Type::Slice(elem) => format!("Sl{}", mangle_type(elem)),
        Type::Array(elem, size) => format!("A{}_{}", size, mangle_type(elem)),
        Type::Tuple(elems) => {
            let parts: Vec<_> = elems.iter().map(|e| mangle_type(e)).collect();
            format!("T{}", parts.join("_"))
        }
        Type::Function { params, ret } => {
            let params_str: Vec<_> = params.iter().map(|p| mangle_type(p)).collect();
            format!("F{}_{}", params_str.join("_"), mangle_type(ret))
        }
        Type::Var(id) => format!("V{}", id),
        Type::TypeParam(def_id, _) => format!("P{}", def_id.0),
        Type::Error => "error".to_string(),
    }
}

/// Lower a typed program to MIR
pub fn lower_program(program: &TypedProgram) -> MirProgram {
    let mut mir = MirProgram::new();

    // Register structs
    for s in &program.structs {
        let fields: Vec<_> = program.ctx.get_struct_fields(s.def_id)
            .map(|f| f.to_vec())
            .unwrap_or_default();
        mir.structs.insert(s.def_id, MirStruct {
            def_id: s.def_id,
            name: s.name.clone(),
            fields,
        });
    }

    // Collect extern statics for lookup during lowering
    let mut extern_statics: HashMap<DefId, (String, Type)> = HashMap::new();
    for ext in &program.extern_statics {
        extern_statics.insert(ext.def_id, (ext.name.clone(), ext.ty.clone()));
    }

    // Build a map of generic functions by DefId
    let mut generic_funcs: HashMap<DefId, &TypedFunction> = HashMap::new();
    
    // Lower non-generic functions directly
    for func in &program.functions {
        // Check if this function has type parameters (is generic)
        let is_generic = func.params.iter().any(|p| has_type_param(&p.ty)) 
            || has_type_param(&func.return_type);
        
        if is_generic {
            generic_funcs.insert(func.def_id, func);
        } else {
            if let Some(mir_func) = lower_function(func, &program.ctx, &extern_statics, None) {
                mir.functions.push(mir_func);
            }
        }
    }
    
    // Generate monomorphized versions for each instantiation
    for inst in &program.generic_instantiations {
        if let Some(func) = generic_funcs.get(&inst.func_def_id) {
            // Create a specialized version of the function
            if let Some(mir_func) = lower_monomorphized_function(
                func, 
                &inst.type_args,
                &program.ctx, 
                &extern_statics
            ) {
                mir.functions.push(mir_func);
            }
        }
    }

    // Lower impl methods - mangle names with the impl type
    for imp in &program.impls {
        // Get the type name for name mangling
        let impl_type_name = get_type_name(&imp.target_type, &program.ctx);
        
        for method in &imp.methods {
            if let Some(mir_func) = lower_function(method, &program.ctx, &extern_statics, Some(&impl_type_name)) {
                mir.functions.push(mir_func);
            }
        }
    }
    
    // Register extern functions
    for ext in &program.extern_functions {
        mir.extern_functions.push(MirExternFunction {
            def_id: ext.def_id,
            name: ext.name.clone(),
            params: ext.params.iter().map(|p| p.ty.clone()).collect(),
            return_type: ext.return_type.clone(),
        });
    }

    // Register extern statics
    for ext in &program.extern_statics {
        mir.extern_statics.push(MirExternStatic {
            def_id: ext.def_id,
            name: ext.name.clone(),
            ty: ext.ty.clone(),
        });
    }

    mir
}

/// Get a display name for a type (for name mangling)
fn get_type_name(ty: &Type, ctx: &TypeContext) -> String {
    match ty {
        Type::Struct(def_id) => {
            ctx.get_type_name(*def_id).unwrap_or_else(|| format!("struct_{}", def_id.0))
        }
        Type::Enum(def_id) => {
            ctx.get_type_name(*def_id).unwrap_or_else(|| format!("enum_{}", def_id.0))
        }
        _ => format!("{:?}", ty),
    }
}

/// Lower a single function to MIR
/// If `impl_type_name` is provided, the function name will be mangled as `TypeName::method_name`
fn lower_function(func: &TypedFunction, ctx: &TypeContext, extern_statics: &HashMap<DefId, (String, Type)>, impl_type_name: Option<&str>) -> Option<MirFunction> {
    let body = func.body.as_ref()?;

    let mut lowerer = FunctionLowerer::new(func, ctx, extern_statics, impl_type_name, None);
    lowerer.lower_body(body);

    Some(lowerer.finish())
}

/// Lower a monomorphized version of a generic function
fn lower_monomorphized_function(
    func: &TypedFunction, 
    type_args: &[Type],
    ctx: &TypeContext, 
    extern_statics: &HashMap<DefId, (String, Type)>
) -> Option<MirFunction> {
    let body = func.body.as_ref()?;

    // Create substitution info
    let subst = TypeSubstitution {
        type_args: type_args.to_vec(),
        // We need to get the type param DefIds from the function
        // For now, we'll extract them from the parameter types
        type_param_ids: extract_type_param_ids(func),
    };

    let mut lowerer = FunctionLowerer::new(func, ctx, extern_statics, None, Some(subst));
    lowerer.lower_body(body);

    Some(lowerer.finish())
}

/// Extract type parameter DefIds from a function's signature
fn extract_type_param_ids(func: &TypedFunction) -> Vec<DefId> {
    let mut ids = Vec::new();
    for param in &func.params {
        collect_type_param_ids(&param.ty, &mut ids);
    }
    collect_type_param_ids(&func.return_type, &mut ids);
    ids
}

fn collect_type_param_ids(ty: &Type, ids: &mut Vec<DefId>) {
    match ty {
        Type::TypeParam(def_id, _) => {
            if !ids.contains(def_id) {
                ids.push(*def_id);
            }
        }
        Type::Ref { inner, .. } => collect_type_param_ids(inner, ids),
        Type::Slice(elem) => collect_type_param_ids(elem, ids),
        Type::Array(elem, _) => collect_type_param_ids(elem, ids),
        Type::Tuple(elems) => {
            for e in elems {
                collect_type_param_ids(e, ids);
            }
        }
        Type::Function { params, ret } => {
            for p in params {
                collect_type_param_ids(p, ids);
            }
            collect_type_param_ids(ret, ids);
        }
        _ => {}
    }
}

/// Type substitution info for monomorphization
#[derive(Clone)]
struct TypeSubstitution {
    type_args: Vec<Type>,
    type_param_ids: Vec<DefId>,
}

/// State for lowering a single function
struct FunctionLowerer<'a> {
    func: &'a TypedFunction,
    ctx: &'a TypeContext,
    /// Map from extern static DefId to (name, type)
    extern_statics: &'a HashMap<DefId, (String, Type)>,
    /// Optional impl type name for name mangling
    impl_type_name: Option<String>,
    /// Optional type substitution for monomorphization
    type_subst: Option<TypeSubstitution>,
    
    /// All locals (including params and temporaries)
    locals: Vec<MirLocal>,
    /// Map from DefId to local index
    def_to_local: HashMap<DefId, u32>,
    /// Next local ID
    next_local: u32,
    
    /// Basic blocks
    blocks: Vec<BasicBlock>,
    /// Current block we're building
    current_block: u32,
    /// Statements for current block
    current_stmts: Vec<Statement>,
    
    /// Return place (local 0)
    return_place: u32,
}

impl<'a> FunctionLowerer<'a> {
    fn new(
        func: &'a TypedFunction, 
        ctx: &'a TypeContext, 
        extern_statics: &'a HashMap<DefId, (String, Type)>, 
        impl_type_name: Option<&str>,
        type_subst: Option<TypeSubstitution>,
    ) -> Self {
        // Substitute types if we're monomorphizing
        let return_type = if let Some(ref subst) = type_subst {
            substitute_type(&func.return_type, &subst.type_args, &subst.type_param_ids)
        } else {
            func.return_type.clone()
        };
        
        let mut lowerer = Self {
            func,
            ctx,
            extern_statics,
            impl_type_name: impl_type_name.map(|s| s.to_string()),
            type_subst,
            locals: Vec::new(),
            def_to_local: HashMap::new(),
            next_local: 0,
            blocks: Vec::new(),
            current_block: 0,
            current_stmts: Vec::new(),
            return_place: 0,
        };

        // Local 0 is the return place
        lowerer.return_place = lowerer.new_local("_return".to_string(), return_type, false);

        // Add parameters as locals
        for param in &func.params {
            let param_ty = lowerer.subst_type(&param.ty);
            let local = lowerer.new_local(param.name.clone(), param_ty, true);
            lowerer.def_to_local.insert(param.def_id, local);
        }

        // Start with block 0
        lowerer.current_block = 0;

        lowerer
    }

    fn new_local(&mut self, name: String, ty: Type, is_arg: bool) -> u32 {
        let id = self.next_local;
        self.next_local += 1;
        self.locals.push(MirLocal { id, name, ty, is_arg });
        id
    }

    fn new_temp(&mut self, ty: Type) -> u32 {
        let subst_ty = self.subst_type(&ty);
        self.new_local(format!("_t{}", self.next_local), subst_ty, false)
    }
    
    /// Substitute type parameters with concrete types if monomorphizing
    fn subst_type(&self, ty: &Type) -> Type {
        if let Some(ref subst) = self.type_subst {
            substitute_type(ty, &subst.type_args, &subst.type_param_ids)
        } else {
            ty.clone()
        }
    }

    fn new_block(&mut self) -> u32 {
        let id = self.blocks.len() as u32;
        self.blocks.push(BasicBlock {
            id,
            statements: Vec::new(),
            terminator: Terminator::Unreachable, // Placeholder
        });
        id
    }

    fn push_stmt(&mut self, kind: StatementKind) {
        self.current_stmts.push(Statement { kind });
    }

    fn assign(&mut self, place: Place, rvalue: Rvalue) {
        self.push_stmt(StatementKind::Assign { place, rvalue });
    }

    fn terminate(&mut self, terminator: Terminator) {
        // Finish current block
        let block_id = self.current_block as usize;
        if block_id >= self.blocks.len() {
            self.blocks.push(BasicBlock {
                id: self.current_block,
                statements: std::mem::take(&mut self.current_stmts),
                terminator,
            });
        } else {
            self.blocks[block_id].statements = std::mem::take(&mut self.current_stmts);
            self.blocks[block_id].terminator = terminator;
        }
    }

    fn switch_to_block(&mut self, block: u32) {
        self.current_block = block;
        self.current_stmts.clear();
    }

    fn lower_body(&mut self, body: &TypedBlock) {
        // Create the entry block
        self.new_block();
        self.switch_to_block(0);

        // Lower the body
        let result = self.lower_block(body);

        // Assign result to return place and return
        if let Some(result) = result {
            self.assign(Place::local(self.return_place), Rvalue::Use(result));
        }
        self.terminate(Terminator::Return);
    }

    fn lower_block(&mut self, block: &TypedBlock) -> Option<Operand> {
        let mut last_value = None;

        for stmt in &block.stmts {
            last_value = self.lower_stmt(stmt);
        }

        last_value
    }

    fn lower_stmt(&mut self, stmt: &TypedStmt) -> Option<Operand> {
        match stmt {
            TypedStmt::Let { def_id, name, ty, init, .. } => {
                let local = self.new_local(name.clone(), ty.clone(), false);
                self.def_to_local.insert(*def_id, local);

                if let Some(init_expr) = init {
                    let init_val = self.lower_expr(init_expr);
                    self.assign(Place::local(local), Rvalue::Use(init_val));
                }

                None
            }
            TypedStmt::Expr(expr) => {
                Some(self.lower_expr(expr))
            }
        }
    }

    fn lower_expr(&mut self, expr: &TypedExpr) -> Operand {
        match &expr.kind {
            TypedExprKind::IntLiteral(n) => {
                Operand::Constant(Constant::Int(*n, expr.ty.clone()))
            }
            TypedExprKind::FloatLiteral(n) => {
                Operand::Constant(Constant::Float(*n, expr.ty.clone()))
            }
            TypedExprKind::BoolLiteral(b) => {
                Operand::Constant(Constant::Bool(*b))
            }
            TypedExprKind::StringLiteral(s) => {
                Operand::Constant(Constant::Str(s.clone()))
            }

            TypedExprKind::Var { def_id, .. } => {
                if let Some(&local) = self.def_to_local.get(def_id) {
                    // Check if type is Copy
                    if self.is_copy_type(&expr.ty) {
                        Operand::Copy(Place::local(local))
                    } else {
                        Operand::Move(Place::local(local))
                    }
                } else if let Some((name, ty)) = self.extern_statics.get(def_id) {
                    // Extern static reference
                    Operand::Constant(Constant::ExternStatic(*def_id, name.clone(), ty.clone()))
                } else {
                    // Might be a function reference
                    if let Some(name) = self.ctx.get_type_name(*def_id) {
                        Operand::Constant(Constant::FnPtr(*def_id, name))
                    } else {
                        Operand::Constant(Constant::Unit)
                    }
                }
            }

            TypedExprKind::Binary { left, op, right } => {
                let left_op = self.lower_expr(left);
                let right_op = self.lower_expr(right);
                let mir_op = convert_binop(*op);
                
                let temp = self.new_temp(expr.ty.clone());
                self.assign(
                    Place::local(temp),
                    Rvalue::BinaryOp { op: mir_op, left: left_op, right: right_op }
                );
                Operand::Copy(Place::local(temp))
            }

            TypedExprKind::Unary { op, expr: inner } => {
                let inner_op = self.lower_expr(inner);
                let mir_op = convert_unaryop(*op);
                
                let temp = self.new_temp(expr.ty.clone());
                self.assign(
                    Place::local(temp),
                    Rvalue::UnaryOp { op: mir_op, operand: inner_op }
                );
                Operand::Copy(Place::local(temp))
            }

            TypedExprKind::Call { callee, args } => {
                let func_op = self.lower_expr(callee);
                let arg_ops: Vec<_> = args.iter().map(|a| self.lower_expr(a)).collect();

                let temp = self.new_temp(expr.ty.clone());
                
                // Create continuation block
                let cont_block = self.new_block();
                
                self.terminate(Terminator::Call {
                    func: func_op,
                    args: arg_ops,
                    destination: Place::local(temp),
                    target: cont_block,
                });

                self.switch_to_block(cont_block);
                Operand::Copy(Place::local(temp))
            }
            
            TypedExprKind::GenericCall { func_def_id, type_args, args } => {
                // Generate a mangled name for the monomorphized function
                let base_name = self.ctx.get_type_name(*func_def_id).unwrap_or_default();
                let mangled_name = mangle_generic_name(&base_name, type_args);
                
                let arg_ops: Vec<_> = args.iter().map(|a| self.lower_expr(a)).collect();

                let temp = self.new_temp(expr.ty.clone());
                
                // Create continuation block
                let cont_block = self.new_block();
                
                // Use a special FnPtr constant with the mangled name
                let func_op = Operand::Constant(Constant::MonomorphizedFn(*func_def_id, mangled_name, type_args.clone()));
                
                self.terminate(Terminator::Call {
                    func: func_op,
                    args: arg_ops,
                    destination: Place::local(temp),
                    target: cont_block,
                });

                self.switch_to_block(cont_block);
                Operand::Copy(Place::local(temp))
            }
            
            TypedExprKind::MethodCall { receiver, method: _, method_def_id, args } => {
                // Lower the receiver
                let receiver_op = self.lower_expr(receiver);
                
                // Create a reference to the receiver for &self parameter
                // The receiver needs to be a place to take a reference
                let receiver_ref = match receiver_op {
                    Operand::Copy(place) | Operand::Move(place) => {
                        // Create a temp to hold the reference
                        let ref_ty = Type::Ref { 
                            is_mut: false, 
                            inner: Box::new(receiver.ty.clone()) 
                        };
                        let ref_temp = self.new_temp(ref_ty);
                        self.assign(
                            Place::local(ref_temp),
                            Rvalue::Ref { is_mut: false, place }
                        );
                        Operand::Copy(Place::local(ref_temp))
                    }
                    Operand::Constant(_) => {
                        // Constants can't be referenced directly - store to temp first
                        let temp = self.new_temp(receiver.ty.clone());
                        self.assign(Place::local(temp), Rvalue::Use(receiver_op));
                        let ref_ty = Type::Ref { 
                            is_mut: false, 
                            inner: Box::new(receiver.ty.clone()) 
                        };
                        let ref_temp = self.new_temp(ref_ty);
                        self.assign(
                            Place::local(ref_temp),
                            Rvalue::Ref { is_mut: false, place: Place::local(temp) }
                        );
                        Operand::Copy(Place::local(ref_temp))
                    }
                };
                
                // Lower the other arguments
                let mut arg_ops: Vec<_> = vec![receiver_ref];
                arg_ops.extend(args.iter().map(|a| self.lower_expr(a)));
                
                // Get method name for the function reference
                let method_name = self.ctx.get_type_name(*method_def_id).unwrap_or_default();
                let func_op = Operand::Constant(Constant::FnPtr(*method_def_id, method_name));

                let temp = self.new_temp(expr.ty.clone());
                
                // Create continuation block
                let cont_block = self.new_block();
                
                self.terminate(Terminator::Call {
                    func: func_op,
                    args: arg_ops,
                    destination: Place::local(temp),
                    target: cont_block,
                });

                self.switch_to_block(cont_block);
                Operand::Copy(Place::local(temp))
            }
            
            TypedExprKind::TraitMethodCall { receiver, method, trait_bounds, args } => {
                // For trait method calls on type parameters, we need to resolve the actual
                // method based on the concrete type. During monomorphization, the receiver's
                // type will be substituted with a concrete type.
                
                // Lower the receiver
                let receiver_op = self.lower_expr(receiver);
                
                // Get the receiver's concrete type (after substitution)
                let receiver_ty = self.subst_type(&receiver.ty);
                
                // Create a reference to the receiver for &self parameter
                let receiver_ref = match receiver_op {
                    Operand::Copy(place) | Operand::Move(place) => {
                        let ref_ty = Type::Ref { 
                            is_mut: false, 
                            inner: Box::new(receiver_ty.clone()) 
                        };
                        let ref_temp = self.new_temp(ref_ty);
                        self.assign(
                            Place::local(ref_temp),
                            Rvalue::Ref { is_mut: false, place }
                        );
                        Operand::Copy(Place::local(ref_temp))
                    }
                    Operand::Constant(_) => {
                        let temp = self.new_temp(receiver_ty.clone());
                        self.assign(Place::local(temp), Rvalue::Use(receiver_op));
                        let ref_ty = Type::Ref { 
                            is_mut: false, 
                            inner: Box::new(receiver_ty.clone()) 
                        };
                        let ref_temp = self.new_temp(ref_ty);
                        self.assign(
                            Place::local(ref_temp),
                            Rvalue::Ref { is_mut: false, place: Place::local(temp) }
                        );
                        Operand::Copy(Place::local(ref_temp))
                    }
                };
                
                // Lower the other arguments
                let mut arg_ops: Vec<_> = vec![receiver_ref];
                arg_ops.extend(args.iter().map(|a| self.lower_expr(a)));
                
                // Use a TraitMethodCall constant that will be resolved at codegen
                let func_op = Operand::Constant(Constant::TraitMethodCall {
                    receiver_type: receiver_ty,
                    method_name: method.clone(),
                    trait_bounds: trait_bounds.clone(),
                });

                let temp = self.new_temp(self.subst_type(&expr.ty));
                
                // Create continuation block
                let cont_block = self.new_block();
                
                self.terminate(Terminator::Call {
                    func: func_op,
                    args: arg_ops,
                    destination: Place::local(temp),
                    target: cont_block,
                });

                self.switch_to_block(cont_block);
                Operand::Copy(Place::local(temp))
            }

            TypedExprKind::Field { expr: base, field } => {
                let base_op = self.lower_expr(base);
                
                // Get field index
                let field_idx = self.get_field_index(&base.ty, field).unwrap_or(0);
                
                // Create a temp for the field access
                let temp = self.new_temp(expr.ty.clone());
                
                // If base is a place, we can do a field projection
                if let Operand::Copy(place) | Operand::Move(place) = base_op {
                    let field_place = place.field(field_idx, field.clone());
                    if self.is_copy_type(&expr.ty) {
                        return Operand::Copy(field_place);
                    } else {
                        return Operand::Move(field_place);
                    }
                }
                
                // Otherwise, need to store to temp first
                self.assign(Place::local(temp), Rvalue::Use(base_op));
                Operand::Copy(Place::local(temp).field(field_idx, field.clone()))
            }

            TypedExprKind::StructLit { struct_def, fields } => {
                let operands: Vec<_> = fields.iter()
                    .map(|(_, e)| self.lower_expr(e))
                    .collect();
                
                let name = self.ctx.get_type_name(*struct_def).unwrap_or_default();
                let temp = self.new_temp(expr.ty.clone());
                self.assign(
                    Place::local(temp),
                    Rvalue::Aggregate {
                        kind: AggregateKind::Struct(*struct_def, name),
                        operands,
                    }
                );
                Operand::Copy(Place::local(temp))
            }

            TypedExprKind::If { cond, then_block, else_block } => {
                let cond_op = self.lower_expr(cond);
                
                let then_bb = self.new_block();
                let else_bb = self.new_block();
                let merge_bb = self.new_block();
                
                // Result temp
                let result = self.new_temp(expr.ty.clone());
                
                // Branch on condition
                self.terminate(Terminator::SwitchInt {
                    discr: cond_op,
                    targets: vec![(1, then_bb)], // true -> then
                    otherwise: else_bb,          // false -> else
                });

                // Then block
                self.switch_to_block(then_bb);
                let then_val = self.lower_block(then_block);
                if let Some(val) = then_val {
                    self.assign(Place::local(result), Rvalue::Use(val));
                }
                self.terminate(Terminator::Goto { target: merge_bb });

                // Else block
                self.switch_to_block(else_bb);
                if let Some(else_b) = else_block {
                    let else_val = self.lower_else(else_b);
                    if let Some(val) = else_val {
                        self.assign(Place::local(result), Rvalue::Use(val));
                    }
                }
                self.terminate(Terminator::Goto { target: merge_bb });

                // Continue in merge block
                self.switch_to_block(merge_bb);
                Operand::Copy(Place::local(result))
            }

            TypedExprKind::While { cond, body } => {
                let cond_bb = self.new_block();
                let body_bb = self.new_block();
                let exit_bb = self.new_block();

                // Jump to condition check
                self.terminate(Terminator::Goto { target: cond_bb });

                // Condition block
                self.switch_to_block(cond_bb);
                let cond_op = self.lower_expr(cond);
                self.terminate(Terminator::SwitchInt {
                    discr: cond_op,
                    targets: vec![(1, body_bb)],
                    otherwise: exit_bb,
                });

                // Body block
                self.switch_to_block(body_bb);
                self.lower_block(body);
                self.terminate(Terminator::Goto { target: cond_bb });

                // Exit block
                self.switch_to_block(exit_bb);
                Operand::Constant(Constant::Unit)
            }

            TypedExprKind::Block(block) => {
                self.lower_block(block).unwrap_or(Operand::Constant(Constant::Unit))
            }

            TypedExprKind::Assign { target, value } => {
                let value_op = self.lower_expr(value);
                
                if let Some(place) = self.expr_to_place(target) {
                    self.assign(place, Rvalue::Use(value_op));
                }
                
                Operand::Constant(Constant::Unit)
            }

            TypedExprKind::Ref { is_mut, expr: inner } => {
                if let Some(place) = self.expr_to_place(inner) {
                    let temp = self.new_temp(expr.ty.clone());
                    self.assign(
                        Place::local(temp),
                        Rvalue::Ref { is_mut: *is_mut, place }
                    );
                    Operand::Copy(Place::local(temp))
                } else {
                    // Need to create a temp for the inner expression
                    let inner_op = self.lower_expr(inner);
                    let inner_temp = self.new_temp(inner.ty.clone());
                    self.assign(Place::local(inner_temp), Rvalue::Use(inner_op));
                    
                    let temp = self.new_temp(expr.ty.clone());
                    self.assign(
                        Place::local(temp),
                        Rvalue::Ref { is_mut: *is_mut, place: Place::local(inner_temp) }
                    );
                    Operand::Copy(Place::local(temp))
                }
            }

            TypedExprKind::Deref(inner) => {
                let inner_op = self.lower_expr(inner);
                
                if let Operand::Copy(place) | Operand::Move(place) = inner_op {
                    let deref_place = place.deref();
                    if self.is_copy_type(&expr.ty) {
                        Operand::Copy(deref_place)
                    } else {
                        Operand::Move(deref_place)
                    }
                } else {
                    // Store to temp first
                    let temp = self.new_temp(inner.ty.clone());
                    self.assign(Place::local(temp), Rvalue::Use(inner_op));
                    Operand::Copy(Place::local(temp).deref())
                }
            }

            TypedExprKind::Index { expr: base, index } => {
                let base_op = self.lower_expr(base);
                let index_op = self.lower_expr(index);
                
                if let Operand::Copy(place) | Operand::Move(place) = base_op {
                    let indexed = place.index(index_op);
                    if self.is_copy_type(&expr.ty) {
                        Operand::Copy(indexed)
                    } else {
                        Operand::Move(indexed)
                    }
                } else {
                    let temp = self.new_temp(base.ty.clone());
                    self.assign(Place::local(temp), Rvalue::Use(base_op));
                    Operand::Copy(Place::local(temp).index(index_op))
                }
            }

            TypedExprKind::Match { scrutinee, arms } => {
                let scrut_op = self.lower_expr(scrutinee);
                let result = self.new_temp(expr.ty.clone());
                let merge_bb = self.new_block();
                
                // For now, simple pattern matching on enum discriminants
                // This is a simplified version - full pattern matching is complex
                let mut targets = Vec::new();
                let mut arm_blocks = Vec::new();
                
                for (i, _arm) in arms.iter().enumerate() {
                    let arm_bb = self.new_block();
                    targets.push((i as i64, arm_bb));
                    arm_blocks.push(arm_bb);
                }
                
                let otherwise = arm_blocks.last().copied().unwrap_or(merge_bb);
                
                self.terminate(Terminator::SwitchInt {
                    discr: scrut_op,
                    targets: targets[..targets.len().saturating_sub(1)].to_vec(),
                    otherwise,
                });
                
                for (i, arm) in arms.iter().enumerate() {
                    self.switch_to_block(arm_blocks[i]);
                    let arm_val = self.lower_expr(&arm.body);
                    self.assign(Place::local(result), Rvalue::Use(arm_val));
                    self.terminate(Terminator::Goto { target: merge_bb });
                }
                
                self.switch_to_block(merge_bb);
                Operand::Copy(Place::local(result))
            }

            TypedExprKind::Error => {
                Operand::Constant(Constant::Unit)
            }
        }
    }

    fn lower_else(&mut self, else_branch: &TypedElse) -> Option<Operand> {
        match else_branch {
            TypedElse::Block(block) => self.lower_block(block),
            TypedElse::If(if_expr) => Some(self.lower_expr(if_expr)),
        }
    }

    fn expr_to_place(&self, expr: &TypedExpr) -> Option<Place> {
        match &expr.kind {
            TypedExprKind::Var { def_id, .. } => {
                self.def_to_local.get(def_id).map(|&local| Place::local(local))
            }
            TypedExprKind::Field { expr: base, field } => {
                let base_place = self.expr_to_place(base)?;
                let field_idx = self.get_field_index(&base.ty, field)?;
                Some(base_place.field(field_idx, field.clone()))
            }
            TypedExprKind::Deref(inner) => {
                let inner_place = self.expr_to_place(inner)?;
                Some(inner_place.deref())
            }
            TypedExprKind::Index { expr: base, index } => {
                let base_place = self.expr_to_place(base)?;
                let index_op = self.lower_expr_pure(index)?;
                Some(base_place.index(index_op))
            }
            _ => None,
        }
    }

    fn lower_expr_pure(&self, _expr: &TypedExpr) -> Option<Operand> {
        // For pure expressions that don't have side effects
        // Used for index expressions in places
        // For now, return None to force temp allocation
        None
    }

    fn get_field_index(&self, ty: &Type, field_name: &str) -> Option<usize> {
        // Handle both direct struct types and references to structs
        let struct_def_id = match ty {
            Type::Struct(def_id) => Some(*def_id),
            Type::Ref { inner, .. } => {
                if let Type::Struct(def_id) = inner.as_ref() {
                    Some(*def_id)
                } else {
                    None
                }
            }
            _ => None,
        };
        
        if let Some(def_id) = struct_def_id {
            if let Some(fields) = self.ctx.get_struct_fields(def_id) {
                return fields.iter().position(|(name, _)| name == field_name);
            }
        }
        None
    }

    fn is_copy_type(&self, ty: &Type) -> bool {
        matches!(ty,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64 | Type::Bool | Type::Char | Type::Unit |
            Type::Ref { .. }
        )
    }

    fn finish(mut self) -> MirFunction {
        // Separate params from other locals
        let params: Vec<_> = self.locals.iter()
            .filter(|l| l.is_arg)
            .cloned()
            .collect();
        let locals: Vec<_> = self.locals.iter()
            .filter(|l| !l.is_arg)
            .cloned()
            .collect();

        // Determine the function name
        let name = if let Some(ref subst) = self.type_subst {
            // Monomorphized generic function - use mangled name
            mangle_generic_name(&self.func.name, &subst.type_args)
        } else if let Some(ref type_name) = self.impl_type_name {
            // Impl method - mangle with type name
            format!("{}::{}", type_name, self.func.name)
        } else {
            self.func.name.clone()
        };
        
        // Get the return type (possibly substituted)
        let return_type = self.subst_type(&self.func.return_type);
        
        MirFunction {
            def_id: self.func.def_id,
            name,
            params,
            return_type,
            locals,
            blocks: self.blocks,
        }
    }
}

fn convert_binop(op: wisp_ast::BinOp) -> BinOp {
    use wisp_ast::BinOp as AstOp;
    match op {
        AstOp::Add => BinOp::Add,
        AstOp::Sub => BinOp::Sub,
        AstOp::Mul => BinOp::Mul,
        AstOp::Div => BinOp::Div,
        AstOp::Mod => BinOp::Rem,
        AstOp::Eq => BinOp::Eq,
        AstOp::NotEq => BinOp::Ne,
        AstOp::Lt => BinOp::Lt,
        AstOp::LtEq => BinOp::Le,
        AstOp::Gt => BinOp::Gt,
        AstOp::GtEq => BinOp::Ge,
        AstOp::And => BinOp::And,
        AstOp::Or => BinOp::Or,
    }
}

fn convert_unaryop(op: wisp_ast::UnaryOp) -> UnaryOp {
    use wisp_ast::UnaryOp as AstOp;
    match op {
        AstOp::Neg => UnaryOp::Neg,
        AstOp::Not => UnaryOp::Not,
    }
}

