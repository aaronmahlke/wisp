//! Type representation for Wisp

use wisp_hir::DefId;
use std::collections::HashMap;

/// Interned type ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

/// The type of a value in Wisp
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Primitive integer types
    I8, I16, I32, I64, I128,
    U8, U16, U32, U64, U128,
    /// Floating point
    F32, F64,
    /// Boolean
    Bool,
    /// Character
    Char,
    /// String slice (pointer to null-terminated C string)
    Str,
    /// Unit type ()
    Unit,
    /// Never type (for diverging expressions)
    Never,
    
    /// User-defined struct with type arguments
    Struct { def_id: DefId, type_args: Vec<Type> },
    /// User-defined enum with type arguments
    Enum { def_id: DefId, type_args: Vec<Type> },
    
    /// Reference type
    Ref { is_mut: bool, inner: Box<Type> },
    /// Slice type
    Slice(Box<Type>),
    /// Array type with known size
    Array(Box<Type>, usize),
    /// Tuple type
    Tuple(Vec<Type>),
    /// Function type
    Function { params: Vec<Type>, ret: Box<Type> },
    
    /// Type variable (for inference)
    Var(u32),
    /// Type parameter (generic)
    TypeParam(DefId, String),
    /// Error type (for recovery)
    Error,
}

impl Type {
    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, 
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64
        )
    }

    /// Check if this is an integer type
    pub fn is_integer(&self) -> bool {
        matches!(self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128
        )
    }

    /// Check if this is a signed integer
    pub fn is_signed(&self) -> bool {
        matches!(self, Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
    }

    /// Check if this is a floating point type
    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }
    
    /// Check if this is a primitive type (not a user-defined struct/enum)
    pub fn is_primitive(&self) -> bool {
        matches!(self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 |
            Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 |
            Type::F32 | Type::F64 | Type::Bool | Type::Char | Type::Str |
            Type::Unit | Type::Never
        )
    }

    /// Check if this is a reference type
    pub fn is_ref(&self) -> bool {
        matches!(self, Type::Ref { .. })
    }

    /// Check if this is a mutable reference
    pub fn is_mut_ref(&self) -> bool {
        matches!(self, Type::Ref { is_mut: true, .. })
    }

    /// Get the inner type of a reference
    pub fn deref(&self) -> Option<&Type> {
        match self {
            Type::Ref { inner, .. } => Some(inner),
            _ => None,
        }
    }

    /// Pretty print the type
    pub fn display(&self, ctx: &TypeContext) -> String {
        match self {
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
            Type::Str => "str".to_string(),
            Type::Unit => "()".to_string(),
            Type::Never => "!".to_string(),
            Type::Struct { def_id, type_args } => {
                let name = ctx.get_type_name(*def_id).unwrap_or_else(|| format!("struct#{}", def_id.0));
                if type_args.is_empty() {
                    name
                } else {
                    let args: Vec<_> = type_args.iter().map(|t| t.display(ctx)).collect();
                    format!("{}<{}>", name, args.join(", "))
                }
            }
            Type::Enum { def_id, type_args } => {
                let name = ctx.get_type_name(*def_id).unwrap_or_else(|| format!("enum#{}", def_id.0));
                if type_args.is_empty() {
                    name
                } else {
                    let args: Vec<_> = type_args.iter().map(|t| t.display(ctx)).collect();
                    format!("{}<{}>", name, args.join(", "))
                }
            }
            Type::Ref { is_mut, inner } => {
                let mut_str = if *is_mut { "mut " } else { "" };
                format!("&{}{}", mut_str, inner.display(ctx))
            }
            Type::Slice(elem) => format!("[{}]", elem.display(ctx)),
            Type::Array(elem, size) => format!("[{}; {}]", elem.display(ctx), size),
            Type::Tuple(elems) => {
                let parts: Vec<_> = elems.iter().map(|t| t.display(ctx)).collect();
                format!("({})", parts.join(", "))
            }
            Type::Function { params, ret } => {
                let params_str: Vec<_> = params.iter().map(|t| t.display(ctx)).collect();
                format!("fn({}) -> {}", params_str.join(", "), ret.display(ctx))
            }
            Type::Var(id) => format!("?{}", id),
            Type::TypeParam(_, name) => name.clone(),
            Type::Error => "<error>".to_string(),
        }
    }
}

/// Context for type information
#[derive(Debug)]
pub struct TypeContext {
    /// Map from DefId to type name
    type_names: HashMap<DefId, String>,
    /// Map from DefId to its type
    def_types: HashMap<DefId, Type>,
    /// Struct field types: struct DefId -> [(field name, field type)]
    struct_fields: HashMap<DefId, Vec<(String, Type)>>,
    /// Enum variant types: enum DefId -> [(variant name, variant DefId, field types)]
    enum_variants: HashMap<DefId, Vec<(String, DefId, Vec<Type>)>>,
    /// Set of DefIds that are type parameters
    type_params: std::collections::HashSet<DefId>,
    /// Next type variable ID
    next_var: u32,
    /// Type variable substitutions
    substitutions: HashMap<u32, Type>,
    
    // === LSP Query Support ===
    /// Span -> type string for hover (only for current file, filtered by source_len)
    span_types: HashMap<(usize, usize), String>,
    /// Span -> definition DefId for go-to-definition
    span_definitions: HashMap<(usize, usize), DefId>,
}

impl TypeContext {
    pub fn new() -> Self {
        Self {
            type_names: HashMap::new(),
            def_types: HashMap::new(),
            struct_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            type_params: std::collections::HashSet::new(),
            next_var: 0,
            substitutions: HashMap::new(),
            span_types: HashMap::new(),
            span_definitions: HashMap::new(),
        }
    }
    
    // === LSP Query Methods ===
    
    /// Record a span -> type mapping for hover support
    pub fn record_span_type(&mut self, start: usize, end: usize, type_str: String) {
        self.span_types.insert((start, end), type_str);
    }
    
    /// Record a span -> definition mapping for go-to-definition
    pub fn record_span_definition(&mut self, start: usize, end: usize, def_id: DefId) {
        self.span_definitions.insert((start, end), def_id);
    }
    
    /// Get the type string for a span (exact match)
    pub fn get_span_type(&self, start: usize, end: usize) -> Option<&String> {
        self.span_types.get(&(start, end))
    }
    
    /// Get the definition for a span (exact match)
    pub fn get_span_definition(&self, start: usize, end: usize) -> Option<DefId> {
        self.span_definitions.get(&(start, end)).copied()
    }
    
    /// Find the smallest span containing the given offset and return its type
    pub fn type_at_offset(&self, offset: usize) -> Option<&String> {
        let mut best: Option<((usize, usize), &String)> = None;
        for ((start, end), type_str) in &self.span_types {
            if offset >= *start && offset <= *end {
                let span_size = end - start;
                if best.is_none() || span_size < (best.as_ref().unwrap().0.1 - best.as_ref().unwrap().0.0) {
                    best = Some(((*start, *end), type_str));
                }
            }
        }
        best.map(|(_, s)| s)
    }
    
    /// Find the smallest span containing the given offset and return its definition
    pub fn definition_at_offset(&self, offset: usize) -> Option<DefId> {
        let mut best: Option<((usize, usize), DefId)> = None;
        for ((start, end), def_id) in &self.span_definitions {
            if offset >= *start && offset <= *end {
                let span_size = end - start;
                if best.is_none() || span_size < (best.as_ref().unwrap().0.1 - best.as_ref().unwrap().0.0) {
                    best = Some(((*start, *end), *def_id));
                }
            }
        }
        best.map(|(_, d)| d)
    }
    
    /// Get all span types (for debugging/iteration)
    pub fn all_span_types(&self) -> &HashMap<(usize, usize), String> {
        &self.span_types
    }
    
    /// Get all span definitions (for debugging/iteration)
    pub fn all_span_definitions(&self) -> &HashMap<(usize, usize), DefId> {
        &self.span_definitions
    }

    /// Register a type parameter
    pub fn register_type_param(&mut self, def_id: DefId, name: String) {
        self.type_params.insert(def_id);
        self.type_names.insert(def_id, name);
    }
    
    /// Check if a DefId is a type parameter
    pub fn is_type_param(&self, def_id: DefId) -> bool {
        self.type_params.contains(&def_id)
    }

    /// Register a type name
    pub fn register_type_name(&mut self, def_id: DefId, name: String) {
        self.type_names.insert(def_id, name);
    }

    /// Get type name for a DefId
    pub fn get_type_name(&self, def_id: DefId) -> Option<String> {
        self.type_names.get(&def_id).cloned()
    }
    
    /// Look up a type by name (returns Struct type if found)
    pub fn lookup_type_by_name(&self, name: &str) -> Option<Type> {
        for (def_id, type_name) in &self.type_names {
            if type_name == name {
                // Check if this is a struct
                if self.struct_fields.contains_key(def_id) {
                    return Some(Type::Struct { def_id: *def_id, type_args: vec![] });
                }
            }
        }
        None
    }

    /// Register a definition's type
    pub fn register_def_type(&mut self, def_id: DefId, ty: Type) {
        self.def_types.insert(def_id, ty);
    }

    /// Get type for a definition
    pub fn get_def_type(&self, def_id: DefId) -> Option<&Type> {
        self.def_types.get(&def_id)
    }

    /// Register struct fields
    pub fn register_struct_fields(&mut self, struct_id: DefId, fields: Vec<(String, Type)>) {
        self.struct_fields.insert(struct_id, fields);
    }

    /// Get struct fields
    pub fn get_struct_fields(&self, struct_id: DefId) -> Option<&[(String, Type)]> {
        self.struct_fields.get(&struct_id).map(|v| v.as_slice())
    }

    /// Get a struct field type by name
    pub fn get_struct_field(&self, struct_id: DefId, field_name: &str) -> Option<&Type> {
        self.struct_fields.get(&struct_id)?
            .iter()
            .find(|(name, _)| name == field_name)
            .map(|(_, ty)| ty)
    }

    /// Register enum variants
    pub fn register_enum_variants(&mut self, enum_id: DefId, variants: Vec<(String, DefId, Vec<Type>)>) {
        self.enum_variants.insert(enum_id, variants);
    }

    /// Get enum variants
    pub fn get_enum_variants(&self, enum_id: DefId) -> Option<&[(String, DefId, Vec<Type>)]> {
        self.enum_variants.get(&enum_id).map(|v| v.as_slice())
    }
    
    /// Check if a DefId is an enum variant constructor, and if so return (enum_def_id, variant_index)
    pub fn is_enum_variant(&self, variant_def_id: DefId) -> Option<(DefId, usize)> {
        for (&enum_id, variants) in &self.enum_variants {
            for (idx, (_, vdef_id, _)) in variants.iter().enumerate() {
                if *vdef_id == variant_def_id {
                    return Some((enum_id, idx));
                }
            }
        }
        None
    }

    /// Create a fresh type variable
    pub fn fresh_var(&mut self) -> Type {
        let id = self.next_var;
        self.next_var += 1;
        Type::Var(id)
    }

    /// Apply substitutions to a type
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(subst) = self.substitutions.get(id) {
                    self.apply(subst)
                } else {
                    ty.clone()
                }
            }
            Type::Ref { is_mut, inner } => Type::Ref {
                is_mut: *is_mut,
                inner: Box::new(self.apply(inner)),
            },
            Type::Slice(elem) => Type::Slice(Box::new(self.apply(elem))),
            Type::Array(elem, size) => Type::Array(Box::new(self.apply(elem)), *size),
            Type::Tuple(elems) => Type::Tuple(elems.iter().map(|t| self.apply(t)).collect()),
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(|t| self.apply(t)).collect(),
                ret: Box::new(self.apply(ret)),
            },
            _ => ty.clone(),
        }
    }

    /// Unify two types
    pub fn unify(&mut self, a: &Type, b: &Type) -> Result<(), String> {
        let a = self.apply(a);
        let b = self.apply(b);

        match (&a, &b) {
            // Same type
            _ if a == b => Ok(()),
            
            // Type variable
            (Type::Var(id), _) => {
                self.substitutions.insert(*id, b);
                Ok(())
            }
            (_, Type::Var(id)) => {
                self.substitutions.insert(*id, a);
                Ok(())
            }
            
            // Type parameter - treat like a type variable for now
            // In a full implementation, we'd track these substitutions separately
            (Type::TypeParam(_, _), _) | (_, Type::TypeParam(_, _)) => {
                // For now, accept any type for a type parameter
                // A proper implementation would track and verify consistency
                Ok(())
            }
            
            // Error type unifies with anything
            (Type::Error, _) | (_, Type::Error) => Ok(()),
            
            // Never type unifies with anything (diverging code can have any type)
            (Type::Never, _) | (_, Type::Never) => Ok(()),
            
            // Reference types
            (Type::Ref { is_mut: m1, inner: i1 }, Type::Ref { is_mut: m2, inner: i2 }) => {
                if m1 != m2 {
                    return Err(format!("mutability mismatch: &{} vs &{}", 
                        if *m1 { "mut" } else { "" },
                        if *m2 { "mut" } else { "" }));
                }
                self.unify(i1, i2)
            }
            
            // Slice types
            (Type::Slice(e1), Type::Slice(e2)) => self.unify(e1, e2),
            
            // Array types
            (Type::Array(e1, s1), Type::Array(e2, s2)) => {
                if s1 != s2 {
                    return Err(format!("array size mismatch: {} vs {}", s1, s2));
                }
                self.unify(e1, e2)
            }
            
            // Tuple types
            (Type::Tuple(t1), Type::Tuple(t2)) => {
                if t1.len() != t2.len() {
                    return Err(format!("tuple length mismatch: {} vs {}", t1.len(), t2.len()));
                }
                for (a, b) in t1.iter().zip(t2.iter()) {
                    self.unify(a, b)?;
                }
                Ok(())
            }
            
            // Function types
            (Type::Function { params: p1, ret: r1 }, Type::Function { params: p2, ret: r2 }) => {
                if p1.len() != p2.len() {
                    return Err(format!("function arity mismatch: {} vs {}", p1.len(), p2.len()));
                }
                for (a, b) in p1.iter().zip(p2.iter()) {
                    self.unify(a, b)?;
                }
                self.unify(r1, r2)
            }
            
            // Struct types with type arguments
            (Type::Struct { def_id: d1, type_args: a1 }, Type::Struct { def_id: d2, type_args: a2 }) => {
                if d1 != d2 {
                    return Err(format!("struct mismatch: {:?} vs {:?}", d1, d2));
                }
                if a1.len() != a2.len() {
                    return Err(format!("type argument count mismatch: {} vs {}", a1.len(), a2.len()));
                }
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    self.unify(arg1, arg2)?;
                }
                Ok(())
            }
            
            // Enum types with type arguments
            (Type::Enum { def_id: d1, type_args: a1 }, Type::Enum { def_id: d2, type_args: a2 }) => {
                if d1 != d2 {
                    return Err(format!("enum mismatch: {:?} vs {:?}", d1, d2));
                }
                if a1.len() != a2.len() {
                    return Err(format!("type argument count mismatch: {} vs {}", a1.len(), a2.len()));
                }
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    self.unify(arg1, arg2)?;
                }
                Ok(())
            }
            
            // Mismatch
            _ => Err(format!("type mismatch: {} vs {}", a.display(self), b.display(self))),
        }
    }
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a type name to a Type
pub fn parse_type_name(name: &str) -> Option<Type> {
    match name {
        "i8" => Some(Type::I8),
        "i16" => Some(Type::I16),
        "i32" => Some(Type::I32),
        "i64" => Some(Type::I64),
        "i128" => Some(Type::I128),
        "u8" => Some(Type::U8),
        "u16" => Some(Type::U16),
        "u32" => Some(Type::U32),
        "u64" => Some(Type::U64),
        "u128" => Some(Type::U128),
        "f32" => Some(Type::F32),
        "f64" => Some(Type::F64),
        "bool" => Some(Type::Bool),
        "char" => Some(Type::Char),
        "str" => Some(Type::Str),
        "Never" => Some(Type::Never),
        _ => None,
    }
}

