//! Cranelift code generation

use cranelift_codegen::ir::{
    types, AbiParam, Block, Function, InstBuilder, Signature, Value, FuncRef,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{Linkage, Module, FuncId, DataDescription, DataId};
use cranelift_object::{ObjectBuilder, ObjectModule};
use wisp_mir::*;
use wisp_types::Type;
use wisp_hir::DefId;
use std::collections::HashMap;

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Code generator using Cranelift
pub struct Codegen {
    module: ObjectModule,
    ctx: Context,
    /// Map from function DefId to Cranelift FuncId
    func_ids: HashMap<DefId, FuncId>,
    /// Map from function name to (DefId, FuncId) for lookups
    func_by_name: HashMap<String, (DefId, FuncId)>,
    /// Function signatures by DefId
    func_sigs: HashMap<DefId, Signature>,
    /// Map from string literal to DataId
    string_data: HashMap<String, DataId>,
    /// Counter for generating unique string names
    string_counter: u32,
    /// Map from extern static DefId to DataId
    extern_static_data: HashMap<DefId, DataId>,
}

impl Codegen {
    pub fn new() -> Result<Self, CodegenError> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").map_err(|e| CodegenError {
            message: format!("Failed to set opt_level: {}", e),
        })?;
        // Enable PIC for macOS compatibility
        flag_builder.set("is_pic", "true").map_err(|e| CodegenError {
            message: format!("Failed to set is_pic: {}", e),
        })?;
        
        let isa_builder = cranelift_native::builder().map_err(|e| CodegenError {
            message: format!("Failed to create ISA builder: {}", e),
        })?;
        
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| CodegenError {
                message: format!("Failed to create ISA: {}", e),
            })?;

        let builder = ObjectBuilder::new(
            isa,
            "wisp_output",
            cranelift_module::default_libcall_names(),
        ).map_err(|e| CodegenError {
            message: format!("Failed to create object builder: {}", e),
        })?;

        let module = ObjectModule::new(builder);

        Ok(Self {
            module,
            ctx: Context::new(),
            func_ids: HashMap::new(),
            func_by_name: HashMap::new(),
            func_sigs: HashMap::new(),
            string_data: HashMap::new(),
            string_counter: 0,
            extern_static_data: HashMap::new(),
        })
    }
    
    /// Get or create a data ID for a string literal
    fn get_or_create_string(&mut self, s: &str) -> Result<DataId, CodegenError> {
        if let Some(&data_id) = self.string_data.get(s) {
            return Ok(data_id);
        }
        
        // Create a unique name for this string
        let name = format!(".str.{}", self.string_counter);
        self.string_counter += 1;
        
        // Declare the data
        let data_id = self.module
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| CodegenError {
                message: format!("Failed to declare string data: {}", e),
            })?;
        
        // Define the data (string bytes + null terminator)
        let mut data_desc = DataDescription::new();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0); // null terminator for C compatibility
        data_desc.define(bytes.into_boxed_slice());
        
        self.module
            .define_data(data_id, &data_desc)
            .map_err(|e| CodegenError {
                message: format!("Failed to define string data: {}", e),
            })?;
        
        self.string_data.insert(s.to_string(), data_id);
        Ok(data_id)
    }

    /// Compile a MIR program
    pub fn compile(&mut self, program: &MirProgram) -> Result<(), CodegenError> {
        // First pass: declare all extern functions
        for ext in &program.extern_functions {
            self.declare_extern_function(ext)?;
        }
        
        // Declare all extern statics
        for ext in &program.extern_statics {
            self.declare_extern_static(ext)?;
        }
        
        // Second pass: declare all functions
        for func in &program.functions {
            self.declare_function(func)?;
        }
        
        // Third pass: collect and create all string literals
        for func in &program.functions {
            self.collect_strings(func)?;
        }
        
        // Build map of function return types
        let mut func_return_types: HashMap<DefId, Type> = HashMap::new();
        for func in &program.functions {
            func_return_types.insert(func.def_id, func.return_type.clone());
        }

        // Fourth pass: define all functions
        for func in &program.functions {
            self.define_function(func, &program.structs, &func_return_types)?;
        }

        Ok(())
    }
    
    fn declare_extern_static(&mut self, ext: &MirExternStatic) -> Result<(), CodegenError> {
        // Declare an imported data symbol
        let data_id = self.module
            .declare_data(&ext.name, Linkage::Import, false, false)
            .map_err(|e| CodegenError {
                message: format!("Failed to declare extern static '{}': {}", ext.name, e),
            })?;
        
        self.extern_static_data.insert(ext.def_id, data_id);
        Ok(())
    }
    
    /// Collect all string literals from a function and create data entries
    fn collect_strings(&mut self, func: &MirFunction) -> Result<(), CodegenError> {
        use wisp_mir::StatementKind;
        
        for block in &func.blocks {
            for stmt in &block.statements {
                if let StatementKind::Assign { rvalue, .. } = &stmt.kind {
                    self.collect_strings_from_rvalue(rvalue)?;
                }
            }
            // Also check terminator for function call arguments
            if let Terminator::Call { args, .. } = &block.terminator {
                for arg in args {
                    if let Operand::Constant(Constant::Str(s)) = arg {
                        self.get_or_create_string(s)?;
                    }
                }
            }
        }
        Ok(())
    }
    
    fn collect_strings_from_rvalue(&mut self, rvalue: &Rvalue) -> Result<(), CodegenError> {
        match rvalue {
            Rvalue::Use(Operand::Constant(Constant::Str(s))) => {
                self.get_or_create_string(s)?;
            }
            _ => {}
        }
        Ok(())
    }
    
    fn declare_extern_function(&mut self, ext: &MirExternFunction) -> Result<(), CodegenError> {
        let mut sig = Signature::new(CallConv::SystemV);

        // Add parameters
        for param_ty in &ext.params {
            let ty = self.convert_type(param_ty);
            sig.params.push(AbiParam::new(ty));
        }

        // Add return type
        let ret_ty = self.convert_type(&ext.return_type);
        if ret_ty != types::INVALID {
            sig.returns.push(AbiParam::new(ret_ty));
        }

        // Extern functions use Import linkage
        let func_id = self.module
            .declare_function(&ext.name, Linkage::Import, &sig)
            .map_err(|e| CodegenError {
                message: format!("Failed to declare extern function '{}': {}", ext.name, e),
            })?;

        self.func_ids.insert(ext.def_id, func_id);
        self.func_by_name.insert(ext.name.clone(), (ext.def_id, func_id));
        self.func_sigs.insert(ext.def_id, sig);

        Ok(())
    }

    fn declare_function(&mut self, func: &MirFunction) -> Result<(), CodegenError> {
        let mut sig = Signature::new(CallConv::SystemV);
        
        // Check if this function returns a struct (needs sret handling)
        let returns_struct = matches!(&func.return_type, Type::Struct(_));
        
        // If returning a struct, add an implicit sret pointer as first parameter
        if returns_struct {
            sig.params.push(AbiParam::new(types::I64)); // sret pointer
        }

        // Add parameters
        for param in &func.params {
            let ty = self.convert_type(&param.ty);
            sig.params.push(AbiParam::new(ty));
        }

        // Add return type (only if not a struct - struct returns via sret)
        if !returns_struct {
            let ret_ty = self.convert_type(&func.return_type);
            if ret_ty != types::INVALID {
                sig.returns.push(AbiParam::new(ret_ty));
            }
        }

        let linkage = if func.name == "main" {
            Linkage::Export
        } else {
            Linkage::Local
        };

        let func_id = self.module
            .declare_function(&func.name, linkage, &sig)
            .map_err(|e| CodegenError {
                message: format!("Failed to declare function '{}': {}", func.name, e),
            })?;

        self.func_ids.insert(func.def_id, func_id);
        self.func_by_name.insert(func.name.clone(), (func.def_id, func_id));
        self.func_sigs.insert(func.def_id, sig);

        Ok(())
    }

    fn define_function(&mut self, func: &MirFunction, structs: &HashMap<DefId, MirStruct>, func_return_types: &HashMap<DefId, Type>) -> Result<(), CodegenError> {
        // Look up by name to handle monomorphized functions (which share the same def_id)
        let func_id = self.func_by_name.get(&func.name)
            .map(|(_, id)| *id)
            .ok_or_else(|| CodegenError {
                message: format!("Function '{}' not declared", func.name),
            })?;

        // Check if this function returns a struct (needs sret handling)
        let returns_struct = matches!(&func.return_type, Type::Struct(_));
        
        // Build signature
        let mut sig = Signature::new(CallConv::SystemV);
        
        // If returning a struct, add an implicit sret pointer as first parameter
        if returns_struct {
            sig.params.push(AbiParam::new(types::I64)); // sret pointer
        }
        
        for param in &func.params {
            let ty = self.convert_type(&param.ty);
            sig.params.push(AbiParam::new(ty));
        }
        
        // Only add return type if not a struct (struct returns via sret)
        if !returns_struct {
            let ret_ty = self.convert_type(&func.return_type);
            if ret_ty != types::INVALID {
                sig.returns.push(AbiParam::new(ret_ty));
            }
        }

        self.ctx.func = Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        // Import all functions we might call (including self for recursion)
        let mut func_refs: HashMap<DefId, FuncRef> = HashMap::new();
        let mut func_refs_by_name: HashMap<String, FuncRef> = HashMap::new();
        for (&def_id, &callee_id) in &self.func_ids {
            let func_ref = self.module.declare_func_in_func(callee_id, &mut self.ctx.func);
            func_refs.insert(def_id, func_ref);
        }
        // Also create name-based lookup for monomorphized functions
        for (name, &(_, callee_id)) in &self.func_by_name {
            let func_ref = self.module.declare_func_in_func(callee_id, &mut self.ctx.func);
            func_refs_by_name.insert(name.clone(), func_ref);
        }
        
        // Import all string data references
        use cranelift_codegen::ir::GlobalValue;
        let mut string_gvs: HashMap<String, GlobalValue> = HashMap::new();
        for (s, &data_id) in &self.string_data {
            let gv = self.module.declare_data_in_func(data_id, &mut self.ctx.func);
            string_gvs.insert(s.clone(), gv);
        }
        
        // Import all extern static data references
        let mut extern_static_gvs: HashMap<DefId, GlobalValue> = HashMap::new();
        for (&def_id, &data_id) in &self.extern_static_data {
            let gv = self.module.declare_data_in_func(data_id, &mut self.ctx.func);
            extern_static_gvs.insert(def_id, gv);
        }

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut builder_ctx);

        // Build struct names map
        let struct_names: HashMap<DefId, String> = structs.iter()
            .map(|(id, s)| (*id, s.name.clone()))
            .collect();

        // Create a function compiler
        let mut compiler = FunctionCompiler::new(
            &mut builder,
            &self.func_ids,
            &func_refs,
            &func_refs_by_name,
            &string_gvs,
            &extern_static_gvs,
            structs,
            &struct_names,
            func_return_types,
            func,
            returns_struct,
        );

        compiler.compile()?;

        builder.finalize();

        // Verify the function
        if let Err(errors) = cranelift_codegen::verify_function(&self.ctx.func, self.module.isa()) {
            eprintln!("Cranelift verification errors:\n{}", errors);
        }

        // Define the function
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| CodegenError {
                message: format!("Failed to define function '{}': {}", func.name, e),
            })?;

        self.module.clear_context(&mut self.ctx);

        Ok(())
    }

    fn convert_type(&self, ty: &Type) -> types::Type {
        match ty {
            Type::I8 => types::I8,
            Type::I16 => types::I16,
            Type::I32 => types::I32,
            Type::I64 => types::I64,
            Type::I128 => types::I128,
            Type::U8 => types::I8,
            Type::U16 => types::I16,
            Type::U32 => types::I32,
            Type::U64 => types::I64,
            Type::U128 => types::I128,
            Type::F32 => types::F32,
            Type::F64 => types::F64,
            Type::Bool => types::I8,
            Type::Char => types::I32,
            Type::Str => types::I64, // str is a pointer to C string
            Type::Unit => types::INVALID, // Unit is zero-sized
            Type::Ref { .. } => types::I64, // Pointers are 64-bit
            Type::Struct(_) => types::I64, // Structs passed as pointers for now
            _ => types::I64, // Default to 64-bit
        }
    }

    /// Finish compilation and return the object code
    pub fn finish(self) -> Vec<u8> {
        let product = self.module.finish();
        product.emit().expect("Failed to emit object code")
    }
}

impl Default for Codegen {
    fn default() -> Self {
        Self::new().expect("Failed to create codegen")
    }
}

/// Compiles a single function
struct FunctionCompiler<'a, 'b> {
    builder: &'a mut FunctionBuilder<'b>,
    func_ids: &'a HashMap<DefId, FuncId>,
    func_refs: &'a HashMap<DefId, FuncRef>,
    func_refs_by_name: &'a HashMap<String, FuncRef>,
    string_gvs: &'a HashMap<String, cranelift_codegen::ir::GlobalValue>,
    extern_static_gvs: &'a HashMap<DefId, cranelift_codegen::ir::GlobalValue>,
    structs: &'a HashMap<DefId, MirStruct>,
    struct_names: &'a HashMap<DefId, String>,
    /// Map from function DefId to return type
    func_return_types: &'a HashMap<DefId, Type>,
    mir_func: &'a MirFunction,
    
    /// Map from MIR local to Cranelift Variable (for scalar types)
    locals: HashMap<u32, Variable>,
    /// Map from MIR local to (stack slot, struct DefId) for aggregate types
    struct_slots: HashMap<u32, (cranelift_codegen::ir::StackSlot, DefId)>,
    /// Map from MIR local to (stack slot, elem type, length) for arrays
    array_slots: HashMap<u32, (cranelift_codegen::ir::StackSlot, Type, usize)>,
    /// Map from MIR block to Cranelift Block
    blocks: HashMap<u32, Block>,
    /// Next variable index
    next_var: usize,
    /// If function returns struct, this holds the sret pointer variable
    sret_ptr: Option<Variable>,
    /// If function returns struct, this holds the struct DefId
    sret_def_id: Option<DefId>,
}

impl<'a, 'b> FunctionCompiler<'a, 'b> {
    fn new(
        builder: &'a mut FunctionBuilder<'b>,
        func_ids: &'a HashMap<DefId, FuncId>,
        func_refs: &'a HashMap<DefId, FuncRef>,
        func_refs_by_name: &'a HashMap<String, FuncRef>,
        string_gvs: &'a HashMap<String, cranelift_codegen::ir::GlobalValue>,
        extern_static_gvs: &'a HashMap<DefId, cranelift_codegen::ir::GlobalValue>,
        structs: &'a HashMap<DefId, MirStruct>,
        struct_names: &'a HashMap<DefId, String>,
        func_return_types: &'a HashMap<DefId, Type>,
        mir_func: &'a MirFunction,
        returns_struct: bool,
    ) -> Self {
        let sret_def_id = if returns_struct {
            if let Type::Struct(def_id) = &mir_func.return_type {
                Some(*def_id)
            } else {
                None
            }
        } else {
            None
        };
        
        Self {
            builder,
            func_ids,
            func_refs,
            func_refs_by_name,
            string_gvs,
            extern_static_gvs,
            structs,
            struct_names,
            func_return_types,
            mir_func,
            locals: HashMap::new(),
            struct_slots: HashMap::new(),
            array_slots: HashMap::new(),
            blocks: HashMap::new(),
            sret_ptr: None,
            sret_def_id,
            next_var: 0,
        }
    }

    fn compile(&mut self) -> Result<(), CodegenError> {
        // Create blocks
        for block in &self.mir_func.blocks {
            let cl_block = self.builder.create_block();
            self.blocks.insert(block.id, cl_block);
        }

        // Set up entry block with parameters
        let entry_block = *self.blocks.get(&0).ok_or_else(|| CodegenError {
            message: "No entry block".to_string(),
        })?;
        
        self.builder.append_block_params_for_function_params(entry_block);
        self.builder.switch_to_block(entry_block);
        // Don't seal entry block yet - seal all blocks at the end
        
        // Track the parameter offset (sret takes the first slot if present)
        let param_offset = if self.sret_def_id.is_some() { 1 } else { 0 };
        
        // If we have sret, capture the sret pointer from the first parameter
        if self.sret_def_id.is_some() {
            let sret_var = Variable::from_u32(self.next_var as u32);
            self.next_var += 1;
            self.builder.declare_var(sret_var, types::I64);
            let sret_val = self.builder.block_params(entry_block)[0];
            self.builder.def_var(sret_var, sret_val);
            self.sret_ptr = Some(sret_var);
        }

        // Declare all locals as variables or stack slots
        for local in &self.mir_func.locals {
            if let Type::Struct(def_id) = &local.ty {
                // Structs get stack slots
                if let Some(mir_struct) = self.structs.get(def_id) {
                    let size = self.struct_size(mir_struct);
                    let slot = self.builder.create_sized_stack_slot(
                        cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            size,
                            3, // align to 8 bytes (2^3)
                        )
                    );
                    self.struct_slots.insert(local.id, (slot, *def_id));
                }
            } else if let Type::Array(elem_ty, len) = &local.ty {
                // Arrays get stack slots
                let elem_size = self.type_size(elem_ty);
                let size = elem_size * (*len as u32);
                let slot = self.builder.create_sized_stack_slot(
                    cranelift_codegen::ir::StackSlotData::new(
                        cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                        size,
                        3, // align to 8 bytes
                    )
                );
                self.array_slots.insert(local.id, (slot, elem_ty.as_ref().clone(), *len));
            } else {
                // Scalars get variables
                let var = Variable::from_u32(self.next_var as u32);
                self.next_var += 1;
                let ty = self.convert_type(&local.ty);
                if ty != types::INVALID {
                    self.builder.declare_var(var, ty);
                    self.locals.insert(local.id, var);
                }
            }
        }

        // Map parameters to their entry block values
        for (i, param) in self.mir_func.params.iter().enumerate() {
            let block_param_idx = i + param_offset;
            match &param.ty {
                Type::Struct(def_id) => {
                    // Struct parameters are passed as pointers
                    // Create a stack slot and copy the data from the pointer
                    if let Some(mir_struct) = self.structs.get(def_id) {
                        let size = self.struct_size(mir_struct);
                        let slot = self.builder.create_sized_stack_slot(
                            cranelift_codegen::ir::StackSlotData::new(
                                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                                size,
                                3, // align to 8 bytes
                            )
                        );
                        
                        // Get the pointer parameter (using block_param_idx to account for sret)
                        let ptr_val = self.builder.block_params(entry_block)[block_param_idx];
                        
                        // Copy each field from the source pointer to our stack slot
                        for (field_idx, (_, field_ty)) in mir_struct.fields.iter().enumerate() {
                            let offset = self.field_offset(mir_struct, field_idx);
                            let cl_ty = self.convert_type(field_ty);
                            // Load from source pointer
                            let val = self.builder.ins().load(cl_ty, cranelift_codegen::ir::MemFlags::new(), ptr_val, offset as i32);
                            // Store to our stack slot
                            self.builder.ins().stack_store(val, slot, offset as i32);
                        }
                        
                        self.struct_slots.insert(param.id, (slot, *def_id));
                    }
                }
                Type::Ref { inner, .. } if matches!(inner.as_ref(), Type::Struct(_)) => {
                    // Reference to struct - the parameter IS the pointer, store it as a variable
                    // When accessing fields, we'll load through this pointer
                    let var = Variable::from_u32(self.next_var as u32);
                    self.next_var += 1;
                    self.builder.declare_var(var, types::I64); // pointer type
                    let param_val = self.builder.block_params(entry_block)[block_param_idx];
                    self.builder.def_var(var, param_val);
                    self.locals.insert(param.id, var);
                }
                _ => {
                    // Scalar parameters
                    let var = Variable::from_u32(self.next_var as u32);
                    self.next_var += 1;
                    let ty = self.convert_type(&param.ty);
                    if ty != types::INVALID {
                        self.builder.declare_var(var, ty);
                        let param_val = self.builder.block_params(entry_block)[block_param_idx];
                        self.builder.def_var(var, param_val);
                        self.locals.insert(param.id, var);
                    }
                }
            }
        }

        // Compile each block (don't seal yet - wait until all predecessors are known)
        for mir_block in &self.mir_func.blocks {
            self.compile_block(mir_block)?;
        }

        // Seal all blocks at the end (all predecessors are now known)
        self.builder.seal_all_blocks();

        Ok(())
    }
    
    /// Calculate the size of a struct in bytes
    fn struct_size(&self, mir_struct: &MirStruct) -> u32 {
        let mut size = 0u32;
        for (_, field_ty) in &mir_struct.fields {
            size += self.type_size(field_ty);
        }
        // Align to 8 bytes
        (size + 7) & !7
    }
    
    /// Get the size of a type in bytes
    fn type_size(&self, ty: &Type) -> u32 {
        match ty {
            Type::I8 | Type::U8 | Type::Bool => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::Char | Type::F32 => 4,
            Type::I64 | Type::U64 | Type::F64 | Type::Ref { .. } | Type::Str => 8,
            Type::Struct(def_id) => {
                if let Some(s) = self.structs.get(def_id) {
                    self.struct_size(s)
                } else {
                    8 // default
                }
            }
            _ => 8,
        }
    }
    
    /// Get the offset of a field in a struct
    fn field_offset(&self, mir_struct: &MirStruct, field_idx: usize) -> u32 {
        let mut offset = 0u32;
        for (i, (_, field_ty)) in mir_struct.fields.iter().enumerate() {
            if i == field_idx {
                return offset;
            }
            offset += self.type_size(field_ty);
        }
        offset
    }

    fn compile_block(&mut self, block: &BasicBlock) -> Result<(), CodegenError> {
        let cl_block = *self.blocks.get(&block.id).unwrap();
        
        // Only switch if not already in this block
        if block.id != 0 {
            self.builder.switch_to_block(cl_block);
        }

        // Compile statements
        for stmt in &block.statements {
            self.compile_statement(stmt)?;
        }

        // Compile terminator
        self.compile_terminator(&block.terminator)?;

        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), CodegenError> {
        match &stmt.kind {
            StatementKind::Assign { place, rvalue } => {
                // Check if this is a struct aggregate assignment
                if let Rvalue::Aggregate { kind: AggregateKind::Struct(def_id, _), operands } = rvalue {
                    // Get the destination stack slot
                    if let Some(&(slot, _)) = self.struct_slots.get(&place.local) {
                        // Get struct info
                        if let Some(mir_struct) = self.structs.get(def_id) {
                            // Store each field
                            for (i, operand) in operands.iter().enumerate() {
                                if let Some(val) = self.compile_operand(operand)? {
                                    let offset = self.field_offset(mir_struct, i);
                                    let _field_ty = &mir_struct.fields[i].1;
                                    self.builder.ins().stack_store(val, slot, offset as i32);
                                }
                            }
                        }
                    }
                } else if let Rvalue::Use(Operand::Copy(src_place) | Operand::Move(src_place)) = rvalue {
                    // Check if this is a struct copy
                    if let Some(&(src_slot, src_def_id)) = self.struct_slots.get(&src_place.local) {
                        if let Some(&(dst_slot, _)) = self.struct_slots.get(&place.local) {
                            // Copy struct by copying each field using the stored def_id
                            if let Some(mir_struct) = self.structs.get(&src_def_id) {
                                for (i, (_, field_ty)) in mir_struct.fields.iter().enumerate() {
                                    let offset = self.field_offset(mir_struct, i);
                                    let cl_ty = self.convert_type(field_ty);
                                    let val = self.builder.ins().stack_load(cl_ty, src_slot, offset as i32);
                                    self.builder.ins().stack_store(val, dst_slot, offset as i32);
                                }
                            }
                            return Ok(());
                        }
                    }
                    // Check if this is an array copy
                    if let Some(&(src_slot, ref src_elem_ty, src_len)) = self.array_slots.get(&src_place.local) {
                        let src_elem_ty = src_elem_ty.clone();
                        if let Some(&(dst_slot, _, _)) = self.array_slots.get(&place.local) {
                            // Copy array by copying each element
                            let elem_size = self.type_size(&src_elem_ty);
                            let cl_ty = self.convert_type(&src_elem_ty);
                            for i in 0..src_len {
                                let offset = (i as u32 * elem_size) as i32;
                                let val = self.builder.ins().stack_load(cl_ty, src_slot, offset);
                                self.builder.ins().stack_store(val, dst_slot, offset);
                            }
                            return Ok(());
                        }
                    }
                    // Fall through to normal handling
                    let value = self.compile_rvalue(rvalue)?;
                    if let Some(value) = value {
                        self.store_to_place(place, value)?;
                    }
                } else {
                    let value = self.compile_rvalue(rvalue)?;
                    if let Some(value) = value {
                        self.store_to_place(place, value)?;
                    }
                }
            }
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                // No-op for now
            }
        }
        Ok(())
    }

    fn compile_rvalue(&mut self, rvalue: &Rvalue) -> Result<Option<Value>, CodegenError> {
        match rvalue {
            Rvalue::Use(operand) => self.compile_operand(operand),
            
            Rvalue::BinaryOp { op, left, right } => {
                let left_val = self.compile_operand(left)?.ok_or_else(|| CodegenError {
                    message: "Binary op left operand has no value".to_string(),
                })?;
                let right_val = self.compile_operand(right)?.ok_or_else(|| CodegenError {
                    message: "Binary op right operand has no value".to_string(),
                })?;

                let result = match op {
                    BinOp::Add => self.builder.ins().iadd(left_val, right_val),
                    BinOp::Sub => self.builder.ins().isub(left_val, right_val),
                    BinOp::Mul => self.builder.ins().imul(left_val, right_val),
                    BinOp::Div => self.builder.ins().sdiv(left_val, right_val),
                    BinOp::Rem => self.builder.ins().srem(left_val, right_val),
                    BinOp::Eq => {
                        // icmp returns i8, no need to extend
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::Equal,
                            left_val, right_val
                        )
                    }
                    BinOp::Ne => {
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
                            left_val, right_val
                        )
                    }
                    BinOp::Lt => {
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
                            left_val, right_val
                        )
                    }
                    BinOp::Le => {
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedLessThanOrEqual,
                            left_val, right_val
                        )
                    }
                    BinOp::Gt => {
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThan,
                            left_val, right_val
                        )
                    }
                    BinOp::Ge => {
                        self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::SignedGreaterThanOrEqual,
                            left_val, right_val
                        )
                    }
                    BinOp::And => self.builder.ins().band(left_val, right_val),
                    BinOp::Or => self.builder.ins().bor(left_val, right_val),
                    BinOp::BitAnd => self.builder.ins().band(left_val, right_val),
                    BinOp::BitOr => self.builder.ins().bor(left_val, right_val),
                    BinOp::BitXor => self.builder.ins().bxor(left_val, right_val),
                    BinOp::Shl => self.builder.ins().ishl(left_val, right_val),
                    BinOp::Shr => self.builder.ins().sshr(left_val, right_val),
                };

                Ok(Some(result))
            }

            Rvalue::UnaryOp { op, operand } => {
                let val = self.compile_operand(operand)?.ok_or_else(|| CodegenError {
                    message: "Unary op operand has no value".to_string(),
                })?;

                let result = match op {
                    UnaryOp::Neg => self.builder.ins().ineg(val),
                    UnaryOp::Not => self.builder.ins().bnot(val),
                };

                Ok(Some(result))
            }

            Rvalue::Ref { place, .. } => {
                // Compute the address of the place
                // For locals, we use stack_addr; for struct fields, we compute the offset
                if let Some(&(slot, def_id)) = self.struct_slots.get(&place.local) {
                    // Reference to a struct or struct field
                    if place.projections.is_empty() {
                        // Reference to the whole struct
                        let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                        Ok(Some(addr))
                    } else {
                        // Reference to a struct field
                        if let Some(mir_struct) = self.structs.get(&def_id) {
                            for proj in &place.projections {
                                if let PlaceProjection::Field(idx, _) = proj {
                                    let offset = self.field_offset(mir_struct, *idx);
                                    let addr = self.builder.ins().stack_addr(types::I64, slot, offset as i32);
                                    return Ok(Some(addr));
                                }
                            }
                        }
                        Ok(None)
                    }
                } else if let Some(&var) = self.locals.get(&place.local) {
                    // Check if this is a reference to a struct field through a pointer
                    if !place.projections.is_empty() {
                        // Find the local's type to check if it's a reference to a struct
                        let local_ty = self.mir_func.params.iter()
                            .find(|p| p.id == place.local)
                            .map(|p| &p.ty)
                            .or_else(|| self.mir_func.locals.iter().find(|l| l.id == place.local).map(|l| &l.ty));
                        
                        if let Some(Type::Ref { inner, .. }) = local_ty {
                            if let Type::Struct(def_id) = inner.as_ref() {
                                if let Some(mir_struct) = self.structs.get(def_id) {
                                    // Get the pointer from the variable
                                    let ptr = self.builder.use_var(var);
                                    
                                    // Handle field projections
                                    for proj in &place.projections {
                                        if let PlaceProjection::Field(idx, _) = proj {
                                            let offset = self.field_offset(mir_struct, *idx);
                                            // Compute address of field within the struct
                                            let field_addr = self.builder.ins().iadd_imm(ptr, offset as i64);
                                            return Ok(Some(field_addr));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Reference to a scalar local - need to spill to stack first
                    // Create a stack slot, store the value, and return its address
                    let val = self.builder.use_var(var);
                    let val_ty = self.builder.func.dfg.value_type(val);
                    let size = val_ty.bytes();
                    let slot = self.builder.create_sized_stack_slot(
                        cranelift_codegen::ir::StackSlotData::new(
                            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                            size,
                            0, // default alignment
                        )
                    );
                    self.builder.ins().stack_store(val, slot, 0);
                    let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                    Ok(Some(addr))
                } else {
                    Ok(None)
                }
            }

            Rvalue::Aggregate { operands, .. } => {
                // For now, just return the first operand
                // A real implementation would allocate and fill a struct
                if let Some(first) = operands.first() {
                    self.compile_operand(first)
                } else {
                    Ok(None)
                }
            }

            Rvalue::Discriminant(_) => {
                // Not fully implemented yet
                Ok(Some(self.builder.ins().iconst(types::I32, 0)))
            }
            
            Rvalue::Cast { operand, ty } => {
                let val = self.compile_operand(operand)?.ok_or_else(|| CodegenError {
                    message: "Cast operand has no value".to_string(),
                })?;
                
                let from_ty = self.builder.func.dfg.value_type(val);
                let to_ty = self.convert_type(ty);
                
                // If types are the same, no conversion needed
                if from_ty == to_ty {
                    return Ok(Some(val));
                }
                
                // Handle various cast scenarios
                let result = if from_ty.is_int() && to_ty.is_int() {
                    // Integer to integer cast
                    if from_ty.bits() < to_ty.bits() {
                        // Widening - use sign extension for signed, zero extension otherwise
                        // For simplicity, use uextend (zero extension)
                        self.builder.ins().uextend(to_ty, val)
                    } else if from_ty.bits() > to_ty.bits() {
                        // Narrowing - use ireduce
                        self.builder.ins().ireduce(to_ty, val)
                    } else {
                        // Same size, just use the value
                        val
                    }
                } else if from_ty.is_float() && to_ty.is_float() {
                    // Float to float cast
                    if from_ty.bits() < to_ty.bits() {
                        self.builder.ins().fpromote(to_ty, val)
                    } else {
                        self.builder.ins().fdemote(to_ty, val)
                    }
                } else if from_ty.is_int() && to_ty.is_float() {
                    // Int to float
                    self.builder.ins().fcvt_from_sint(to_ty, val)
                } else if from_ty.is_float() && to_ty.is_int() {
                    // Float to int
                    self.builder.ins().fcvt_to_sint(to_ty, val)
                } else {
                    // For other cases (pointers, etc.), just bitcast or use the value directly
                    val
                };
                
                Ok(Some(result))
            }
        }
    }

    fn compile_operand(&mut self, operand: &Operand) -> Result<Option<Value>, CodegenError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.load_from_place(place)
            }
            Operand::Constant(constant) => {
                match constant {
                    Constant::Int(n, ty) => {
                        let cl_ty = self.convert_type(ty);
                        Ok(Some(self.builder.ins().iconst(cl_ty, *n)))
                    }
                    Constant::Float(n, _ty) => {
                        Ok(Some(self.builder.ins().f64const(*n)))
                    }
                    Constant::Bool(b) => {
                        Ok(Some(self.builder.ins().iconst(types::I8, if *b { 1 } else { 0 })))
                    }
                    Constant::Str(s) => {
                        // Get the global value for this string and return its address
                        if let Some(&gv) = self.string_gvs.get(s) {
                            let addr = self.builder.ins().global_value(types::I64, gv);
                            Ok(Some(addr))
                        } else {
                            Err(CodegenError {
                                message: format!("String literal not found: {:?}", s),
                            })
                        }
                    }
                    Constant::Unit => Ok(None),
                    Constant::FnPtr(def_id, name) => {
                        // Get the function reference and its address
                        let func_ref = self.func_refs.get(def_id).copied()
                            .or_else(|| self.func_refs_by_name.get(name).copied());
                        if let Some(func_ref) = func_ref {
                            let addr = self.builder.ins().func_addr(types::I64, func_ref);
                            Ok(Some(addr))
                        } else {
                            // Function not found - return null pointer
                            Ok(Some(self.builder.ins().iconst(types::I64, 0)))
                        }
                    }
                    Constant::ExternStatic(def_id, name, ty) => {
                        // Get the global value for this extern static and load from it
                        if let Some(&gv) = self.extern_static_gvs.get(def_id) {
                            let addr = self.builder.ins().global_value(types::I64, gv);
                            // Load the value from the address
                            let cl_ty = self.convert_type(ty);
                            let val = self.builder.ins().load(cl_ty, cranelift_codegen::ir::MemFlags::new(), addr, 0);
                            Ok(Some(val))
                        } else {
                            Err(CodegenError {
                                message: format!("Extern static not found: {}", name),
                            })
                        }
                    }
                    Constant::MonomorphizedFn(_, _, _) => {
                        // Monomorphized function reference - similar to FnPtr
                        // The actual function lookup happens in Terminator::Call
                        Ok(Some(self.builder.ins().iconst(types::I64, 0)))
                    }
                    Constant::TraitMethodCall { .. } => {
                        // Trait method call - resolved in Terminator::Call
                        Ok(Some(self.builder.ins().iconst(types::I64, 0)))
                    }
                }
            }
        }
    }

    fn load_from_place(&mut self, place: &Place) -> Result<Option<Value>, CodegenError> {
        // Check if this is a struct field access (use stored def_id)
        if let Some(&(slot, def_id)) = self.struct_slots.get(&place.local) {
            if let Some(mir_struct) = self.structs.get(&def_id) {
                // Handle field projections
                for proj in &place.projections {
                    if let PlaceProjection::Field(idx, _) = proj {
                        let offset = self.field_offset(mir_struct, *idx);
                        let field_ty = &mir_struct.fields[*idx].1;
                        let cl_ty = self.convert_type(field_ty);
                        let val = self.builder.ins().stack_load(cl_ty, slot, offset as i32);
                        return Ok(Some(val));
                    }
                }
            }
            // Loading whole struct - return address
            let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
            return Ok(Some(addr));
        }
        
        // Check if this is an array access
        if let Some(&(slot, ref elem_ty, _len)) = self.array_slots.get(&place.local) {
            let elem_ty = elem_ty.clone();  // Clone to avoid borrow issues
            for proj in &place.projections {
                if let PlaceProjection::Index(idx_operand) = proj {
                    // Compile the index operand
                    let idx_val = self.compile_operand(idx_operand)?
                        .ok_or_else(|| CodegenError { message: "Invalid array index".to_string() })?;
                    
                    // Compute element offset: index * element_size
                    let elem_size = self.type_size(&elem_ty);
                    let elem_size_val = self.builder.ins().iconst(types::I64, elem_size as i64);
                    // Extend index to i64 for address calculation
                    let idx_i64 = self.builder.ins().sextend(types::I64, idx_val);
                    let offset = self.builder.ins().imul(idx_i64, elem_size_val);
                    
                    // Get base address of array
                    let base_addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                    let elem_addr = self.builder.ins().iadd(base_addr, offset);
                    
                    // Load the element
                    let cl_ty = self.convert_type(&elem_ty);
                    let val = self.builder.ins().load(cl_ty, cranelift_codegen::ir::MemFlags::new(), elem_addr, 0);
                    return Ok(Some(val));
                }
            }
            // Loading whole array - return address
            let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
            return Ok(Some(addr));
        }
        
        if let Some(&var) = self.locals.get(&place.local) {
            // Check if this is a reference to a struct with field access
            if !place.projections.is_empty() {
                // Find the local's type
                let local_ty = self.mir_func.params.iter()
                    .find(|p| p.id == place.local)
                    .map(|p| &p.ty)
                    .or_else(|| self.mir_func.locals.iter().find(|l| l.id == place.local).map(|l| &l.ty));
                
                if let Some(Type::Ref { inner, .. }) = local_ty {
                    // Get the pointer from the variable
                    let ptr = self.builder.use_var(var);
                    
                    match inner.as_ref() {
                        Type::Struct(def_id) => {
                            if let Some(mir_struct) = self.structs.get(def_id) {
                                // Handle field projections on struct references
                                for proj in &place.projections {
                                    if let PlaceProjection::Field(idx, _) = proj {
                                        let offset = self.field_offset(mir_struct, *idx);
                                        let field_ty = &mir_struct.fields[*idx].1;
                                        let cl_ty = self.convert_type(field_ty);
                                        let val = self.builder.ins().load(cl_ty, cranelift_codegen::ir::MemFlags::new(), ptr, offset as i32);
                                        return Ok(Some(val));
                                    }
                                }
                            }
                        }
                        _ => {
                            // Handle dereference of primitive references
                            for proj in &place.projections {
                                if let PlaceProjection::Deref = proj {
                                    let inner_ty = self.convert_type(inner);
                                    let val = self.builder.ins().load(inner_ty, cranelift_codegen::ir::MemFlags::new(), ptr, 0);
                                    return Ok(Some(val));
                                }
                            }
                        }
                    }
                }
            }
            
            // For simple locals or unhandled projections, use the variable
            Ok(Some(self.builder.use_var(var)))
        } else {
            Ok(None)
        }
    }

    fn store_to_place(&mut self, place: &Place, value: Value) -> Result<(), CodegenError> {
        // Check if this is a struct field store (use stored def_id)
        if let Some(&(slot, def_id)) = self.struct_slots.get(&place.local) {
            if let Some(mir_struct) = self.structs.get(&def_id) {
                for proj in &place.projections {
                    if let PlaceProjection::Field(idx, _) = proj {
                        let offset = self.field_offset(mir_struct, *idx);
                        self.builder.ins().stack_store(value, slot, offset as i32);
                        return Ok(());
                    }
                }
            }
            // Storing to whole struct slot not supported this way
            return Ok(());
        }
        
        // Check if this is an array element store
        if let Some(&(slot, ref elem_ty, _len)) = self.array_slots.get(&place.local) {
            let elem_ty = elem_ty.clone();  // Clone to avoid borrow issues
            for proj in &place.projections {
                if let PlaceProjection::Index(idx_operand) = proj {
                    // Compile the index operand
                    let idx_val = self.compile_operand(idx_operand)?
                        .ok_or_else(|| CodegenError { message: "Invalid array index".to_string() })?;
                    
                    // Compute element offset: index * element_size
                    let elem_size = self.type_size(&elem_ty);
                    let elem_size_val = self.builder.ins().iconst(types::I64, elem_size as i64);
                    // Extend index to i64 for address calculation
                    let idx_i64 = self.builder.ins().sextend(types::I64, idx_val);
                    let offset = self.builder.ins().imul(idx_i64, elem_size_val);
                    
                    // Get base address of array
                    let base_addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                    let elem_addr = self.builder.ins().iadd(base_addr, offset);
                    
                    // Store the element
                    self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), value, elem_addr, 0);
                    return Ok(());
                }
            }
            // Storing to whole array not supported
            return Ok(());
        }
        
        if let Some(&var) = self.locals.get(&place.local) {
            if place.projections.is_empty() {
                self.builder.def_var(var, value);
            } else {
                // Handle storing through a reference (e.g., self.field = value where self is &mut T)
                // The var holds a pointer, we need to store at the appropriate offset
                let ptr = self.builder.use_var(var);
                
                // Calculate total offset from projections
                let mut offset: i32 = 0;
                for proj in &place.projections {
                    if let PlaceProjection::Field(idx, _) = proj {
                        // We need to find the struct type to get field offset
                        // Look up the local's type from mir_func
                        if let Some(local) = self.mir_func.locals.iter().find(|l| l.id == place.local) {
                            if let Type::Ref { inner, .. } = &local.ty {
                                if let Type::Struct(def_id) = inner.as_ref() {
                                    if let Some(mir_struct) = self.structs.get(def_id) {
                                        offset += self.field_offset(mir_struct, *idx) as i32;
                                    }
                                }
                            }
                        }
                        // Also check params
                        if let Some(param) = self.mir_func.params.iter().find(|p| p.id == place.local) {
                            if let Type::Ref { inner, .. } = &param.ty {
                                if let Type::Struct(def_id) = inner.as_ref() {
                                    if let Some(mir_struct) = self.structs.get(def_id) {
                                        offset += self.field_offset(mir_struct, *idx) as i32;
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Store through the pointer at the computed offset
                self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), value, ptr, offset);
            }
        }
        Ok(())
    }

    fn compile_terminator(&mut self, term: &Terminator) -> Result<(), CodegenError> {
        match term {
            Terminator::Goto { target } => {
                let target_block = *self.blocks.get(target).unwrap();
                self.builder.ins().jump(target_block, &[]);
            }

            Terminator::SwitchInt { discr, targets, otherwise } => {
                let discr_val = self.compile_operand(discr)?.ok_or_else(|| CodegenError {
                    message: "Switch discriminant has no value".to_string(),
                })?;
                
                // Get the type of the discriminant value
                let discr_ty = self.builder.func.dfg.value_type(discr_val);

                let otherwise_block = *self.blocks.get(otherwise).unwrap();

                if targets.len() == 1 {
                    // Simple if-else
                    let (val, target) = &targets[0];
                    let target_block = *self.blocks.get(target).unwrap();
                    // Use the discriminant's type for the comparison value
                    let cmp_val = self.builder.ins().iconst(discr_ty, *val);
                    let cmp = self.builder.ins().icmp(
                        cranelift_codegen::ir::condcodes::IntCC::Equal,
                        discr_val, cmp_val
                    );
                    self.builder.ins().brif(cmp, target_block, &[], otherwise_block, &[]);
                } else if targets.is_empty() {
                    self.builder.ins().jump(otherwise_block, &[]);
                } else {
                    // Multiple targets - use a series of branches
                    for (val, target) in targets {
                        let target_block = *self.blocks.get(target).unwrap();
                        let cmp_val = self.builder.ins().iconst(discr_ty, *val);
                        let cmp = self.builder.ins().icmp(
                            cranelift_codegen::ir::condcodes::IntCC::Equal,
                            discr_val, cmp_val
                        );
                        let next_block = self.builder.create_block();
                        self.builder.ins().brif(cmp, target_block, &[], next_block, &[]);
                        self.builder.switch_to_block(next_block);
                        // Don't seal - will be sealed at the end
                    }
                    self.builder.ins().jump(otherwise_block, &[]);
                }
            }

            Terminator::Return => {
                // Check if we're returning a struct via sret
                if let (Some(sret_var), Some(def_id)) = (self.sret_ptr, self.sret_def_id) {
                    // Copy the return struct to the sret pointer
                    if let Some(&(slot, _)) = self.struct_slots.get(&0) {
                        // Copy from local _0's stack slot to sret pointer
                        if let Some(mir_struct) = self.structs.get(&def_id) {
                            let sret_ptr = self.builder.use_var(sret_var);
                            for (field_idx, (_, field_ty)) in mir_struct.fields.iter().enumerate() {
                                let offset = self.field_offset(mir_struct, field_idx);
                                let cl_ty = self.convert_type(field_ty);
                                let val = self.builder.ins().stack_load(cl_ty, slot, offset as i32);
                                self.builder.ins().store(cranelift_codegen::ir::MemFlags::new(), val, sret_ptr, offset as i32);
                            }
                        }
                    }
                    // Return void
                    self.builder.ins().return_(&[]);
                } else if let Some(&var) = self.locals.get(&0) {
                    // Scalar return
                    let ret_val = self.builder.use_var(var);
                    self.builder.ins().return_(&[ret_val]);
                } else {
                    self.builder.ins().return_(&[]);
                }
            }

            Terminator::Call { func, args, destination, target } => {
                // Get the callee's def_id, name, and check if it returns a struct
                let (callee_def_id, callee_name) = match func {
                    Operand::Constant(Constant::FnPtr(def_id, name)) => (Some(*def_id), Some(name.clone())),
                    Operand::Constant(Constant::MonomorphizedFn(def_id, name, _)) => (Some(*def_id), Some(name.clone())),
                    Operand::Constant(Constant::TraitMethodCall { receiver_type, method_name, .. }) => {
                        // Resolve the trait method to a concrete implementation
                        // The method is mangled as "TypeName::method_name"
                        // Unwrap references to get the underlying type
                        let inner_type = match receiver_type {
                            Type::Ref { inner, .. } => inner.as_ref(),
                            other => other,
                        };
                        
                        // Get the type name for mangling
                        let type_name = match inner_type {
                            Type::Struct(def_id) => self.struct_names.get(def_id).cloned(),
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
                        
                        if let Some(name) = type_name {
                            let mangled = format!("{}::{}", name, method_name);
                            (None, Some(mangled))
                        } else {
                            (None, None)
                        }
                    }
                    _ => (None, None),
                };
                
                // Check if the callee returns a struct
                let mut callee_returns_struct = callee_def_id
                    .and_then(|id| self.func_return_types.get(&id))
                    .map(|ty| matches!(ty, Type::Struct(_)))
                    .unwrap_or(false);
                
                // For trait method calls, we don't have def_id, so check if destination is a struct
                if !callee_returns_struct {
                    if let Some(local) = self.mir_func.locals.iter().find(|l| l.id == destination.local) {
                        if matches!(local.ty, Type::Struct(_)) {
                            callee_returns_struct = true;
                        }
                    }
                }
                
                // Compile arguments - handle struct args specially (pass as pointers)
                let mut arg_vals = Vec::new();
                
                // If callee returns struct, pass destination's address as first arg (sret)
                if callee_returns_struct {
                    if let Some(&(slot, _)) = self.struct_slots.get(&destination.local) {
                        let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                        arg_vals.push(addr);
                    }
                }
                
                for arg in args {
                    match arg {
                        Operand::Copy(place) | Operand::Move(place) => {
                            // Check if this is a whole struct argument (no projections)
                            if place.projections.is_empty() {
                                if let Some(&(slot, _)) = self.struct_slots.get(&place.local) {
                                    // Pass the address of the struct
                                    let addr = self.builder.ins().stack_addr(types::I64, slot, 0);
                                    arg_vals.push(addr);
                                } else if let Some(val) = self.compile_operand(arg).ok().flatten() {
                                    arg_vals.push(val);
                                }
                            } else {
                                // Has projections (e.g., field access) - load the value
                                if let Some(val) = self.compile_operand(arg).ok().flatten() {
                                    arg_vals.push(val);
                                }
                            }
                        }
                        _ => {
                            if let Some(val) = self.compile_operand(arg).ok().flatten() {
                                arg_vals.push(val);
                            }
                        }
                    }
                }

                // Get the function reference for direct calls, or function pointer for indirect calls
                let func_ref = match func {
                    Operand::Constant(Constant::FnPtr(def_id, name)) => {
                        // First try by def_id, then by name (for lambdas)
                        self.func_refs.get(def_id).copied()
                            .or_else(|| self.func_refs_by_name.get(name).copied())
                    }
                    Operand::Constant(Constant::MonomorphizedFn(_, name, _)) => {
                        // Look up by mangled name for monomorphized functions
                        self.func_refs_by_name.get(name).copied()
                    }
                    Operand::Constant(Constant::TraitMethodCall { .. }) => {
                        // Use the callee_name we resolved earlier
                        callee_name.as_ref().and_then(|name| self.func_refs_by_name.get(name).copied())
                    }
                    _ => None,
                };

                if let Some(func_ref) = func_ref {
                    // Direct call
                    let call = self.builder.ins().call(func_ref, &arg_vals);
                    
                    // Get the return value (if any) - skip for struct returns (handled via sret)
                    if !callee_returns_struct {
                        let results = self.builder.inst_results(call);
                        if !results.is_empty() {
                            if let Some(&var) = self.locals.get(&destination.local) {
                                self.builder.def_var(var, results[0]);
                            }
                        }
                    }
                } else if let Some(func_ptr) = self.compile_operand(func).ok().flatten() {
                    // Indirect call through function pointer
                    // Build the signature for the indirect call
                    let mut sig = Signature::new(CallConv::SystemV);
                    
                    // Add parameter types based on the arguments
                    for arg in &arg_vals {
                        sig.params.push(AbiParam::new(self.builder.func.dfg.value_type(*arg)));
                    }
                    
                    // Get return type from destination local
                    if let Some(local) = self.mir_func.locals.iter().find(|l| l.id == destination.local) {
                        let ret_ty = self.convert_type(&local.ty);
                        if ret_ty != types::INVALID {
                            sig.returns.push(AbiParam::new(ret_ty));
                        }
                    }
                    
                    let sig_ref = self.builder.import_signature(sig);
                    let call = self.builder.ins().call_indirect(sig_ref, func_ptr, &arg_vals);
                    
                    // Get the return value
                    let results = self.builder.inst_results(call);
                    if !results.is_empty() {
                        if let Some(&var) = self.locals.get(&destination.local) {
                            self.builder.def_var(var, results[0]);
                        }
                    }
                } else {
                    // Fallback: store zero for unknown calls
                    if let Some(&var) = self.locals.get(&destination.local) {
                        let zero = self.builder.ins().iconst(types::I32, 0);
                        self.builder.def_var(var, zero);
                    }
                }

                let target_block = *self.blocks.get(target).unwrap();
                self.builder.ins().jump(target_block, &[]);
            }

            Terminator::Unreachable => {
                self.builder.ins().trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            }
        }

        Ok(())
    }

    fn convert_type(&self, ty: &Type) -> types::Type {
        match ty {
            Type::I8 => types::I8,
            Type::I16 => types::I16,
            Type::I32 => types::I32,
            Type::I64 => types::I64,
            Type::I128 => types::I128,
            Type::U8 => types::I8,
            Type::U16 => types::I16,
            Type::U32 => types::I32,
            Type::U64 => types::I64,
            Type::U128 => types::I128,
            Type::F32 => types::F32,
            Type::F64 => types::F64,
            Type::Bool => types::I8,
            Type::Char => types::I32,
            Type::Unit => types::INVALID,
            Type::Ref { .. } => types::I64,
            Type::Struct(_) => types::I64,
            _ => types::I64,
        }
    }
}

