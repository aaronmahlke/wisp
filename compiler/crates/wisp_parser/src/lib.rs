use wisp_ast::*;
use wisp_lexer::{Lexer, Span, SpannedToken, Token};
use std::collections::{HashSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Parser<'src> {
    tokens: Vec<SpannedToken>,
    pos: usize,
    source: &'src str,
}

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}..{}", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for ParseError {}

pub type ParseResult<T> = Result<T, ParseError>;

/// Parse a file and recursively resolve imports
/// 
/// This function takes the source text and the file path, parses the source,
/// and recursively parses and includes any imported files.
pub fn parse_with_imports(source: &str, file_path: &Path) -> Result<SourceFile, String> {
    let base_dir = file_path.parent().unwrap_or(Path::new("."));
    let mut visited = HashSet::new();
    if let Ok(canonical) = file_path.canonicalize() {
        visited.insert(canonical);
    } else {
        visited.insert(file_path.to_path_buf());
    }
    
    parse_with_imports_recursive(source, base_dir, &mut visited)
}

/// Configuration for import resolution
pub struct ImportConfig {
    /// Path to the standard library
    pub std_path: PathBuf,
    /// Path to the project root (from wisp.toml or source dir)
    pub project_root: PathBuf,
}

impl ImportConfig {
    /// Create a new ImportConfig by detecting project root and std path
    pub fn detect(source_file: &Path) -> Self {
        let source_dir = source_file.parent().unwrap_or(Path::new("."));
        
        // Find project root by looking for wisp.toml
        let project_root = find_project_root(source_dir)
            .unwrap_or_else(|| source_dir.to_path_buf());
        
        // Find std path from WISP_STD_PATH env var or relative to project
        let std_path = std::env::var("WISP_STD_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| project_root.join("std"));
        
        Self { std_path, project_root }
    }
}

/// Find project root by walking up looking for wisp.toml
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join("wisp.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn parse_with_imports_recursive(
    source: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<SourceFile, String> {
    // For backwards compatibility, flatten the imports into a single SourceFile
    let with_imports = parse_with_imports_structured(source, base_dir, visited)?;
    
    let mut all_items = Vec::new();
    
    // Add imported module items first (they need to be defined before use)
    for module in with_imports.imported_modules {
        // Add the import declaration
        all_items.push(Item::Import(module.import));
        // Add all items from the module
        all_items.extend(module.items);
    }
    
    // Add local items
    all_items.extend(with_imports.local_items);
    
    Ok(SourceFile { items: all_items })
}

/// A cache of parsed modules keyed by canonical path
pub type ModuleCache = HashMap<PathBuf, Vec<Item>>;

/// Parse a file with imports, preserving namespace structure
/// Uses a module cache to ensure each module is only parsed once
/// but its items can be referenced by multiple namespaces
pub fn parse_with_imports_structured(
    source: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
) -> Result<SourceFileWithImports, String> {
    let mut module_cache: ModuleCache = HashMap::new();
    parse_with_imports_structured_cached(source, base_dir, visited, &mut module_cache)
}

/// Cache entry includes both items and the module's own imports
type ModuleCacheEntry = (Vec<Item>, Vec<ImportDecl>);
type ModuleCacheWithImports = HashMap<PathBuf, ModuleCacheEntry>;

fn parse_with_imports_structured_cached(
    source: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    module_cache: &mut ModuleCache,
) -> Result<SourceFileWithImports, String> {
    // Use a separate cache that includes import info
    let mut imports_cache: ModuleCacheWithImports = HashMap::new();
    parse_with_imports_impl(source, base_dir, visited, module_cache, &mut imports_cache)
}

fn parse_with_imports_impl(
    source: &str,
    base_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    module_cache: &mut ModuleCache,
    imports_cache: &mut ModuleCacheWithImports,
) -> Result<SourceFileWithImports, String> {
    let ast = Parser::parse(source).map_err(|e| format!("Parse error: {}", e))?;
    
    // Detect import config from base_dir
    let config = ImportConfig::detect(base_dir);
    
    let mut local_items = Vec::new();
    let mut local_imports = Vec::new();  // Track this module's imports
    let mut imported_modules = Vec::new();
    
    for item in ast.items {
        match item {
            Item::Import(import) => {
                // Track this import for scope resolution
                local_imports.push(import.clone());
                
                // Resolve import path based on type
                let import_path = resolve_import_path(&import.path, base_dir, &config)?;
                
                let canonical = match import_path.canonicalize() {
                    Ok(c) => c,
                    Err(e) => return Err(format!("Cannot find import '{}': {}", format_import_path(&import.path), e)),
                };
                
                // Check if we've already parsed this module
                if let Some((cached_items, cached_imports)) = imports_cache.get(&canonical) {
                    // Use cached items for this namespace
                    imported_modules.push(ImportedModule {
                        import,
                        items: cached_items.clone(),
                        module_imports: cached_imports.clone(),
                        is_transitive: false,
                    });
                    continue;
                }
                
                // Also check the simple cache (for backwards compatibility)
                if let Some(cached_items) = module_cache.get(&canonical) {
                    imported_modules.push(ImportedModule {
                        import,
                        items: cached_items.clone(),
                        module_imports: vec![],
                        is_transitive: false,
                    });
                    continue;
                }
                
                // Check if we're currently parsing this module (cycle detection)
                if visited.contains(&canonical) {
                    // Cycle detected - add empty module to break the cycle
                    imported_modules.push(ImportedModule {
                        import,
                        items: vec![],
                        module_imports: vec![],
                        is_transitive: false,
                    });
                    continue;
                }
                visited.insert(canonical.clone());
                
                // Read and parse the imported file
                let import_source = match fs::read_to_string(&import_path) {
                    Ok(s) => s,
                    Err(e) => return Err(format!("Cannot read import '{}': {}", format_import_path(&import.path), e)),
                };
                
                let import_dir = import_path.parent().unwrap_or(Path::new("."));
                let imported_ast = parse_with_imports_impl(&import_source, import_dir, visited, module_cache, imports_cache)?;
                
                // Get the module's local items and its own imports
                let module_items = imported_ast.local_items.clone();
                
                // Collect the imports that were declared in this module
                // These are stored in the imported_modules that came from parsing this file
                // Actually, we need to extract them from the original AST...
                // The local_imports from the recursive call would be empty since we're processing
                // the result, not the intermediate state.
                
                // Re-parse just to get imports (inefficient but correct for now)
                let module_own_imports: Vec<ImportDecl> = {
                    let temp_ast = Parser::parse(&import_source).map_err(|e| format!("Parse error: {}", e))?;
                    temp_ast.items.iter().filter_map(|item| {
                        if let Item::Import(imp) = item {
                            Some(imp.clone())
                        } else {
                            None
                        }
                    }).collect()
                };
                
                // Cache this module's items and imports
                module_cache.insert(canonical.clone(), module_items.clone());
                imports_cache.insert(canonical.clone(), (module_items.clone(), module_own_imports.clone()));
                
                // First, add the transitive imports as separate modules
                // Mark them as transitive so they don't create top-level namespaces
                for mut sub_module in imported_ast.imported_modules {
                    sub_module.is_transitive = true;
                    imported_modules.push(sub_module);
                }
                
                // Then add this module with its local items and its own imports
                imported_modules.push(ImportedModule {
                    import,
                    items: module_items,
                    module_imports: module_own_imports,
                    is_transitive: false,
                });
            }
            other => local_items.push(other),
        }
    }
    
    Ok(SourceFileWithImports {
        local_items,
        imported_modules,
    })
}

/// Resolve an import path to a file system path
fn resolve_import_path(path: &ImportPath, _base_dir: &Path, config: &ImportConfig) -> Result<PathBuf, String> {
    let resolved = match path {
        ImportPath::Std(segments) => {
            let mut p = config.std_path.clone();
            if segments.is_empty() {
                // `import std` -> look for std/mod.ws
                p.join("mod.ws")
            } else {
                for seg in segments {
                    p = p.join(seg);
                }
                p.with_extension("ws")
            }
        }
        ImportPath::Project(segments) => {
            let mut p = config.project_root.clone();
            if segments.is_empty() {
                // `import @/` -> look for mod.ws in project root
                p.join("mod.ws")
            } else {
                for seg in segments {
                    p = p.join(seg);
                }
                p.with_extension("ws")
            }
        }
        ImportPath::Package(name, segments) => {
            // Future: resolve from packages directory
            let mut p = config.project_root.join("packages").join(name);
            if segments.is_empty() {
                p.join("mod.ws")
            } else {
                for seg in segments {
                    p = p.join(seg);
                }
                p.with_extension("ws")
            }
        }
    };
    
    Ok(resolved)
}

/// Format an import path for error messages
fn format_import_path(path: &ImportPath) -> String {
    match path {
        ImportPath::Std(segs) => format!("std/{}", segs.join("/")),
        ImportPath::Project(segs) => format!("@/{}", segs.join("/")),
        ImportPath::Package(name, segs) => {
            if segs.is_empty() {
                format!("pkg/{}", name)
            } else {
                format!("pkg/{}/{}", name, segs.join("/"))
            }
        }
    }
}

impl<'src> Parser<'src> {
    pub fn new(source: &'src str) -> ParseResult<Self> {
        let tokens = Lexer::tokenize(source)
            .map_err(|e| ParseError { message: e.message, span: e.span })?;
        Ok(Self { tokens, pos: 0, source })
    }

    pub fn parse(source: &str) -> ParseResult<SourceFile> {
        let mut parser = Parser::new(source)?;
        parser.parse_source_file()
    }

    // === Token Access ===

    fn current(&self) -> &SpannedToken {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek(&self) -> &Token {
        &self.current().token
    }

    fn peek_span(&self) -> Span {
        self.current().span
    }

    fn advance(&mut self) -> &SpannedToken {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), Token::Eof)
    }

    fn check(&self, token: &Token) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(token)
    }

    fn expect(&mut self, expected: Token) -> ParseResult<SpannedToken> {
        if self.check(&expected) {
            Ok(self.advance().clone())
        } else {
            Err(ParseError {
                message: format!("expected '{}', found '{}'", expected, self.peek()),
                span: self.peek_span(),
            })
        }
    }

    fn expect_ident(&mut self) -> ParseResult<Ident> {
        match self.peek().clone() {
            Token::Ident(name) => {
                let span = self.peek_span();
                self.advance();
                Ok(Ident::new(name, span))
            }
            Token::SelfLower => {
                let span = self.peek_span();
                self.advance();
                Ok(Ident::new("self".to_string(), span))
            }
            Token::SelfUpper => {
                let span = self.peek_span();
                self.advance();
                Ok(Ident::new("Self".to_string(), span))
            }
            _ => Err(ParseError {
                message: format!("expected identifier, found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        }
    }

    // === Parsing ===

    fn parse_source_file(&mut self) -> ParseResult<SourceFile> {
        let mut items = Vec::new();
        
        while !self.is_at_end() {
            items.push(self.parse_item()?);
        }
        
        Ok(SourceFile { items })
    }

    fn parse_item(&mut self) -> ParseResult<Item> {
        // Check for optional pub keyword
        let is_pub = if self.check(&Token::Pub) {
            self.advance();
            true
        } else {
            false
        };
        
        match self.peek() {
            Token::Import => self.parse_import(is_pub).map(Item::Import),
            Token::Fn => self.parse_fn_def(is_pub).map(Item::Function),
            Token::Extern => self.parse_extern_item(is_pub),
            Token::Struct => self.parse_struct_def(is_pub).map(Item::Struct),
            Token::Enum => self.parse_enum_def(is_pub).map(Item::Enum),
            Token::Trait => self.parse_trait_def(is_pub).map(Item::Trait),
            Token::Impl => {
                if is_pub {
                    return Err(ParseError {
                        message: "impl blocks cannot be public (methods inside can be)".to_string(),
                        span: self.peek_span(),
                    });
                }
                self.parse_impl_block().map(Item::Impl)
            }
            _ => Err(ParseError {
                message: format!("expected item (import, fn, extern, struct, enum, trait, impl), found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        }
    }
    
    fn parse_extern_item(&mut self, is_pub: bool) -> ParseResult<Item> {
        let start = self.peek_span();
        self.expect(Token::Extern)?;
        
        match self.peek() {
            Token::Fn => {
                // extern fn ...
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::LParen)?;
                let params = self.parse_param_list()?;
                self.expect(Token::RParen)?;
                
                let return_type = if self.check(&Token::Arrow) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                
                let end_span = return_type.as_ref().map(|t| t.span).unwrap_or(start);
                let span = Span::new(start.start, end_span.end);
                
                Ok(Item::ExternFunction(ExternFnDef { is_pub, name, params, return_type, span }))
            }
            Token::Static => {
                // extern static NAME: TYPE
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                
                let span = Span::new(start.start, ty.span.end);
                
                Ok(Item::ExternStatic(ExternStaticDef { is_pub, name, ty, span }))
            }
            _ => Err(ParseError {
                message: format!("expected 'fn' or 'static' after 'extern', found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        }
    }
    
    fn parse_import(&mut self, is_pub: bool) -> ParseResult<ImportDecl> {
        let start = self.peek_span();
        self.expect(Token::Import)?;
        
        // Check for destructure-only syntax: import { items } from path
        if self.check(&Token::LBrace) {
            return self.parse_destructure_import(start, is_pub);
        }
        
        // Parse path: std/io, @/utils/math, or pkg/name/sub
        let path = self.parse_import_path()?;
        
        // Check for alias: import std.io as stdio
        let alias = if self.check(&Token::As) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };
        
        // Check for inline destructuring: import std.io { print, File } or import std.io.{ print, File }
        let items = if self.check(&Token::LBrace) {
            Some(self.parse_import_items()?)
        } else if self.check(&Token::Dot) {
            // Check if it's a dot followed by brace: .{
            if self.pos + 1 < self.tokens.len() && matches!(self.tokens[self.pos + 1].token, Token::LBrace) {
                self.advance(); // consume the dot
                Some(self.parse_import_items()?)
            } else {
                None
            }
        } else {
            None
        };
        
        let end = self.peek_span();
        Ok(ImportDecl {
            is_pub,
            path,
            alias,
            items,
            destructure_only: false,
            span: Span::new(start.start, end.start),
        })
    }
    
    /// Parse destructure-only import: import { print, File as F } from std/io
    fn parse_destructure_import(&mut self, start: Span, is_pub: bool) -> ParseResult<ImportDecl> {
        let items = self.parse_import_items()?;
        
        // Expect "from"
        if let Token::Ident(ref s) = self.peek().clone() {
            if s == "from" {
                self.advance();
            } else {
                return Err(ParseError {
                    message: format!("expected 'from' after import items, found '{}'", s),
                    span: self.peek_span(),
                });
            }
        } else {
            return Err(ParseError {
                message: format!("expected 'from' after import items, found '{}'", self.peek()),
                span: self.peek_span(),
            });
        }
        
        let path = self.parse_import_path()?;
        let end = self.peek_span();
        
        Ok(ImportDecl {
            is_pub,
            path,
            alias: None,
            items: Some(items),
            destructure_only: true,
            span: Span::new(start.start, end.start),
        })
    }
    
    /// Parse import path: std/io, @/utils/math, pkg/name/sub
    fn parse_import_path(&mut self) -> ParseResult<ImportPath> {
        // Check for @ prefix (project-relative)
        if self.check(&Token::At) {
            self.advance();
            self.expect(Token::Dot)?;
            let segments = self.parse_path_segments()?;
            return Ok(ImportPath::Project(segments));
        }
        
        // Otherwise, expect an identifier (std, pkg, or custom)
        let first = self.expect_ident()?;
        
        match first.name.as_str() {
            "std" => {
                // Check if there's a dot - if not, this is `import std` (the whole std lib)
                if self.check(&Token::Dot) {
                    self.advance();
                    let segments = self.parse_path_segments()?;
                    Ok(ImportPath::Std(segments))
                } else {
                    // `import std` with no subpath - will resolve to std/mod.ws
                    Ok(ImportPath::Std(vec![]))
                }
            }
            "pkg" => {
                self.expect(Token::Dot)?;
                let pkg_name = self.expect_ident()?.name;
                if self.check(&Token::Dot) {
                    self.advance();
                    let segments = self.parse_path_segments()?;
                    Ok(ImportPath::Package(pkg_name, segments))
                } else {
                    Ok(ImportPath::Package(pkg_name, vec![]))
                }
            }
            _ => {
                // Treat as std path for backwards compatibility: io -> std.io
                let mut segments = vec![first.name];
                while self.check(&Token::Dot) {
                    self.advance();
                    let seg = self.expect_ident()?;
                    segments.push(seg.name);
                }
                Ok(ImportPath::Std(segments))
            }
        }
    }
    
    /// Parse path segments: io, utils/math
    fn parse_path_segments(&mut self) -> ParseResult<Vec<String>> {
        let mut segments = vec![];
        let first = self.expect_ident()?;
        segments.push(first.name);
        
        while self.check(&Token::Dot) {
            // Look ahead to see if there's a { after the dot (for inline destructuring)
            // If so, don't consume the dot
            if self.pos + 1 < self.tokens.len() && matches!(self.tokens[self.pos + 1].token, Token::LBrace) {
                break;
            }
            self.advance();
            let seg = self.expect_ident()?;
            segments.push(seg.name);
        }
        
        Ok(segments)
    }
    
    /// Parse import items: { print, File as F }
    fn parse_import_items(&mut self) -> ParseResult<Vec<ImportItem>> {
        self.expect(Token::LBrace)?;
        let mut items = vec![];
        
        while !self.check(&Token::RBrace) && !self.check(&Token::Eof) {
            let start = self.peek_span();
            let name = self.expect_ident()?;
            
            let alias = if self.check(&Token::As) {
                self.advance();
                Some(self.expect_ident()?)
            } else {
                None
            };
            
            let end = self.peek_span();
            items.push(ImportItem {
                name,
                alias,
                span: Span::new(start.start, end.start),
            });
            
            if !self.check(&Token::RBrace) {
                self.expect(Token::Comma)?;
            }
        }
        
        self.expect(Token::RBrace)?;
        Ok(items)
    }
    
    fn parse_fn_def(&mut self, is_pub: bool) -> ParseResult<FnDef> {
        let start = self.peek_span();
        self.expect(Token::Fn)?;
        
        let name = self.expect_ident()?;
        
        // Parse optional generic parameters <T, U: Clone>
        let type_params = if self.check(&Token::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        
        self.expect(Token::LParen)?;
        let params = self.parse_param_list()?;
        self.expect(Token::RParen)?;
        
        let return_type = if self.check(&Token::Arrow) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        
        // Body is optional (for trait method signatures)
        let body = if self.check(&Token::LBrace) {
            Some(self.parse_block()?)
        } else {
            None
        };
        
        let end_span = body.as_ref().map(|b| b.span)
            .or(return_type.as_ref().map(|t| t.span))
            .unwrap_or(start);
        let span = Span::new(start.start, end_span.end);
        
        Ok(FnDef { is_pub, name, type_params, params, return_type, body, span })
    }
    
    /// Parse generic parameters: <T, U: Clone + Debug, V = i32>
    fn parse_generic_params(&mut self) -> ParseResult<Vec<GenericParam>> {
        self.expect(Token::Lt)?;
        
        let mut params = Vec::new();
        
        while !self.check(&Token::Gt) && !self.is_at_end() {
            let start = self.peek_span();
            let name = self.expect_ident()?;
            
            // Parse optional bounds: T: Clone + Debug
            let bounds = if self.check(&Token::Colon) {
                self.advance();
                self.parse_type_bounds()?
            } else {
                Vec::new()
            };
            
            // Parse optional default: T = i32
            let default = if self.check(&Token::Eq) {
                self.advance();
                Some(self.parse_type()?)
            } else {
                None
            };
            
            let span = Span::new(start.start, self.peek_span().end);
            params.push(GenericParam { name, bounds, default, span });
            
            if !self.check(&Token::Gt) {
                self.expect(Token::Comma)?;
            }
        }
        
        self.expect(Token::Gt)?;
        Ok(params)
    }
    
    /// Parse type bounds: Clone + Debug
    fn parse_type_bounds(&mut self) -> ParseResult<Vec<TypeExpr>> {
        let mut bounds = Vec::new();
        
        bounds.push(self.parse_type()?);
        
        while self.check(&Token::Plus) {
            self.advance();
            bounds.push(self.parse_type()?);
        }
        
        Ok(bounds)
    }

    fn parse_param_list(&mut self) -> ParseResult<Vec<Param>> {
        let mut params = Vec::new();
        
        while !self.check(&Token::RParen) && !self.is_at_end() {
            params.push(self.parse_param()?);
            
            if !self.check(&Token::RParen) {
                self.expect(Token::Comma)?;
            }
        }
        
        Ok(params)
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let start = self.peek_span();
        
        // Check for &self or &mut self
        if self.check(&Token::Amp) {
            self.advance();
            let is_mut = if self.check(&Token::Mut) {
                self.advance();
                true
            } else {
                false
            };
            if self.check(&Token::SelfLower) {
                let span = self.peek_span();
                self.advance();
                let name = Ident::new("self".to_string(), span);
                let ty = TypeExpr {
                    kind: TypeKind::Ref(is_mut, Box::new(TypeExpr {
                        kind: TypeKind::Named(Ident::new("Self".to_string(), span), Vec::new()),
                        span,
                    })),
                    span: Span::new(start.start, span.end),
                };
                return Ok(Param { name, is_mut: false, ty, span: Span::new(start.start, span.end) });
            }
        }
        
        // Check for bare `self` (by value)
        if self.check(&Token::SelfLower) {
            let span = self.peek_span();
            self.advance();
            let name = Ident::new("self".to_string(), span);
            let ty = TypeExpr {
                kind: TypeKind::Named(Ident::new("Self".to_string(), span), Vec::new()),
                span,
            };
            return Ok(Param { name, is_mut: false, ty, span });
        }
        
        let is_mut = if self.check(&Token::Mut) {
            self.advance();
            true
        } else {
            false
        };
        
        let name = self.expect_ident()?;
        self.expect(Token::Colon)?;
        let ty = self.parse_type()?;
        
        let span = Span::new(start.start, ty.span.end);
        
        Ok(Param { name, is_mut, ty, span })
    }

    fn parse_struct_def(&mut self, is_pub: bool) -> ParseResult<StructDef> {
        let start = self.peek_span();
        self.expect(Token::Struct)?;
        
        let name = self.expect_ident()?;
        
        // Parse optional generic parameters
        let type_params = if self.check(&Token::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        
        self.expect(Token::LBrace)?;
        let fields = self.parse_struct_fields()?;
        let end = self.expect(Token::RBrace)?;
        
        let span = Span::new(start.start, end.span.end);
        
        Ok(StructDef { is_pub, name, type_params, fields, span })
    }

    fn parse_struct_fields(&mut self) -> ParseResult<Vec<StructField>> {
        let mut fields = Vec::new();
        
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            let name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            
            let span = Span::new(start.start, ty.span.end);
            fields.push(StructField { name, ty, span });
            
            // Optional trailing comma
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        
        Ok(fields)
    }

    fn parse_enum_def(&mut self, is_pub: bool) -> ParseResult<EnumDef> {
        let start = self.peek_span();
        self.expect(Token::Enum)?;
        
        let name = self.expect_ident()?;
        
        // Parse optional generic parameters
        let type_params = if self.check(&Token::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        
        self.expect(Token::LBrace)?;
        let variants = self.parse_enum_variants()?;
        let end = self.expect(Token::RBrace)?;
        
        let span = Span::new(start.start, end.span.end);
        
        Ok(EnumDef { is_pub, name, type_params, variants, span })
    }

    fn parse_enum_variants(&mut self) -> ParseResult<Vec<EnumVariant>> {
        let mut variants = Vec::new();
        
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let start = self.peek_span();
            let name = self.expect_ident()?;
            
            let fields = if self.check(&Token::LParen) {
                self.advance();
                let fields = self.parse_struct_fields_in_parens()?;
                self.expect(Token::RParen)?;
                fields
            } else {
                Vec::new()
            };
            
            let end_span = if fields.is_empty() { name.span } else { self.tokens[self.pos - 1].span };
            let span = Span::new(start.start, end_span.end);
            variants.push(EnumVariant { name, fields, span });
            
            // Optional trailing comma
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        
        Ok(variants)
    }

    fn parse_struct_fields_in_parens(&mut self) -> ParseResult<Vec<StructField>> {
        let mut fields = Vec::new();
        
        while !self.check(&Token::RParen) && !self.is_at_end() {
            let start = self.peek_span();
            let name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            
            let span = Span::new(start.start, ty.span.end);
            fields.push(StructField { name, ty, span });
            
            if !self.check(&Token::RParen) {
                self.expect(Token::Comma)?;
            }
        }
        
        Ok(fields)
    }

    fn parse_trait_def(&mut self, is_pub: bool) -> ParseResult<TraitDef> {
        let start = self.peek_span();
        self.expect(Token::Trait)?;
        
        let name = self.expect_ident()?;
        
        // Parse optional generic parameters
        let type_params = if self.check(&Token::Lt) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        
        self.expect(Token::LBrace)?;
        
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Trait methods can also be pub (for documentation, defaults to trait visibility)
            let method_is_pub = if self.check(&Token::Pub) {
                self.advance();
                true
            } else {
                false
            };
            methods.push(self.parse_fn_def(method_is_pub)?);
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(TraitDef { is_pub, name, type_params, methods, span })
    }

    fn parse_impl_block(&mut self) -> ParseResult<ImplBlock> {
        let start = self.peek_span();
        self.expect(Token::Impl)?;
        
        let first_type = self.parse_type()?;
        
        // Check for "for Type" (trait impl)
        let (trait_name, trait_type_args, target_type) = if self.check(&Token::For) {
            self.advance();
            let target = self.parse_type()?;
            // first_type was the trait name (possibly with type args)
            let (trait_ident, type_args) = match first_type.kind {
                TypeKind::Named(id, args) => (id, args),
                _ => return Err(ParseError {
                    message: "expected trait name".to_string(),
                    span: first_type.span,
                }),
            };
            (Some(trait_ident), type_args, target)
        } else {
            (None, Vec::new(), first_type)
        };
        
        self.expect(Token::LBrace)?;
        
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            // Methods in impl blocks can be pub
            let method_is_pub = if self.check(&Token::Pub) {
                self.advance();
                true
            } else {
                false
            };
            methods.push(self.parse_fn_def(method_is_pub)?);
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(ImplBlock { trait_name, trait_type_args, target_type, methods, span })
    }

    fn parse_type(&mut self) -> ParseResult<TypeExpr> {
        let start = self.peek_span();
        
        // Reference type: &T or &mut T or &[T]
        if self.check(&Token::Amp) {
            self.advance();
            let is_mut = if self.check(&Token::Mut) {
                self.advance();
                true
            } else {
                false
            };
            
            // Check for slice: &[T]
            if self.check(&Token::LBracket) {
                self.advance();
                let elem = self.parse_type()?;
                let end = self.expect(Token::RBracket)?;
                let span = Span::new(start.start, end.span.end);
                return Ok(TypeExpr {
                    kind: TypeKind::Slice(Box::new(elem)),
                    span,
                });
            }
            
            let inner = self.parse_type()?;
            let span = Span::new(start.start, inner.span.end);
            return Ok(TypeExpr {
                kind: TypeKind::Ref(is_mut, Box::new(inner)),
                span,
            });
        }
        
        // Unit type or tuple: ()
        if self.check(&Token::LParen) {
            self.advance();
            if self.check(&Token::RParen) {
                let end = self.advance();
                return Ok(TypeExpr {
                    kind: TypeKind::Unit,
                    span: Span::new(start.start, end.span.end),
                });
            }
            // TODO: Tuple types
            return Err(ParseError {
                message: "tuple types not yet supported".to_string(),
                span: self.peek_span(),
            });
        }
        
        // Array type: [T; N]
        if self.check(&Token::LBracket) {
            self.advance();
            let elem = self.parse_type()?;
            self.expect(Token::Semi)?;
            let size = self.parse_expr()?;
            let end = self.expect(Token::RBracket)?;
            let span = Span::new(start.start, end.span.end);
            return Ok(TypeExpr {
                kind: TypeKind::Array(Box::new(elem), Box::new(size)),
                span,
            });
        }
        
        // Named type: identifier (including Self), optionally with type args: Vec<i32>
        let name = self.expect_ident()?;
        
        // Parse optional type arguments
        let type_args = if self.check(&Token::Lt) {
            self.parse_type_args()?
        } else {
            Vec::new()
        };
        
        let end_span = if type_args.is_empty() { name.span } else { self.tokens[self.pos - 1].span };
        let span = Span::new(name.span.start, end_span.end);
        
        Ok(TypeExpr {
            kind: TypeKind::Named(name, type_args),
            span,
        })
    }
    
    /// Parse type arguments: <i32, String>
    fn parse_type_args(&mut self) -> ParseResult<Vec<TypeExpr>> {
        self.expect(Token::Lt)?;
        
        let mut args = Vec::new();
        
        while !self.check(&Token::Gt) && !self.is_at_end() {
            args.push(self.parse_type()?);
            
            if !self.check(&Token::Gt) {
                self.expect(Token::Comma)?;
            }
        }
        
        self.expect(Token::Gt)?;
        Ok(args)
    }

    fn parse_block(&mut self) -> ParseResult<Block> {
        let start = self.peek_span();
        self.expect(Token::LBrace)?;
        
        let mut stmts = Vec::new();
        
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            stmts.push(self.parse_stmt()?);
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(Block { stmts, span })
    }

    fn parse_stmt(&mut self) -> ParseResult<Stmt> {
        let stmt = match self.peek() {
            Token::Let => self.parse_let_stmt(),
            _ => self.parse_expr_stmt(),
        }?;
        
        // Consume optional semicolon
        if self.check(&Token::Semi) {
            self.advance();
        }
        
        Ok(stmt)
    }

    fn parse_let_stmt(&mut self) -> ParseResult<Stmt> {
        let start = self.peek_span();
        self.expect(Token::Let)?;
        
        let is_mut = if self.check(&Token::Mut) {
            self.advance();
            true
        } else {
            false
        };
        
        let name = self.expect_ident()?;
        
        let ty = if self.check(&Token::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        
        let init = if self.check(&Token::Eq) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };
        
        let end_span = init.as_ref().map(|e| e.span)
            .or(ty.as_ref().map(|t| t.span))
            .unwrap_or(name.span);
        
        let span = Span::new(start.start, end_span.end);
        
        Ok(Stmt::Let(LetStmt { name, is_mut, ty, init, span }))
    }

    fn parse_expr_stmt(&mut self) -> ParseResult<Stmt> {
        let expr = self.parse_expr()?;
        let span = expr.span;
        Ok(Stmt::Expr(ExprStmt { expr, span }))
    }

    // === Expression Parsing (Pratt Parser) ===

    fn parse_expr(&mut self) -> ParseResult<Expr> {
        self.parse_expr_inner(true)
    }

    /// Parse expression, optionally allowing struct literals
    /// (struct literals are disallowed in if/while conditions to avoid ambiguity with blocks)
    fn parse_expr_inner(&mut self, allow_struct_lit: bool) -> ParseResult<Expr> {
        self.parse_assignment(allow_struct_lit)
    }

    /// Parse expression without struct literals (for if/while conditions)
    fn parse_expr_no_struct(&mut self) -> ParseResult<Expr> {
        self.parse_expr_inner(false)
    }

    fn parse_assignment(&mut self, allow_struct_lit: bool) -> ParseResult<Expr> {
        let expr = self.parse_binary_inner(0, allow_struct_lit)?;
        
        if self.check(&Token::Eq) {
            self.advance();
            let rhs = self.parse_assignment(allow_struct_lit)?;
            let span = Span::new(expr.span.start, rhs.span.end);
            return Ok(Expr {
                kind: ExprKind::Assign(Box::new(expr), Box::new(rhs)),
                span,
            });
        }
        
        Ok(expr)
    }

    fn parse_binary_inner(&mut self, min_prec: u8, allow_struct_lit: bool) -> ParseResult<Expr> {
        let mut left = self.parse_unary_inner(allow_struct_lit)?;
        
        while let Some(op) = self.peek_binop() {
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            
            self.advance(); // consume operator
            let right = self.parse_binary_inner(prec + 1, allow_struct_lit)?;
            
            let span = Span::new(left.span.start, right.span.end);
            left = Expr {
                kind: ExprKind::Binary(Box::new(left), op, Box::new(right)),
                span,
            };
        }
        
        Ok(left)
    }

    fn peek_binop(&self) -> Option<BinOp> {
        match self.peek() {
            Token::Plus => Some(BinOp::Add),
            Token::Minus => Some(BinOp::Sub),
            Token::Star => Some(BinOp::Mul),
            Token::Slash => Some(BinOp::Div),
            Token::Percent => Some(BinOp::Mod),
            Token::EqEq => Some(BinOp::Eq),
            Token::NotEq => Some(BinOp::NotEq),
            Token::Lt => Some(BinOp::Lt),
            Token::Gt => Some(BinOp::Gt),
            Token::LtEq => Some(BinOp::LtEq),
            Token::GtEq => Some(BinOp::GtEq),
            Token::AndAnd => Some(BinOp::And),
            Token::OrOr => Some(BinOp::Or),
            Token::DotDot => Some(BinOp::Range),
            _ => None,
        }
    }

    fn parse_unary_inner(&mut self, allow_struct_lit: bool) -> ParseResult<Expr> {
        let start = self.peek_span();
        
        if self.check(&Token::Minus) {
            self.advance();
            let expr = self.parse_unary_inner(allow_struct_lit)?;
            let span = Span::new(start.start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Unary(UnaryOp::Neg, Box::new(expr)),
                span,
            });
        }
        
        if self.check(&Token::Not) {
            self.advance();
            let expr = self.parse_unary_inner(allow_struct_lit)?;
            let span = Span::new(start.start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Unary(UnaryOp::Not, Box::new(expr)),
                span,
            });
        }
        
        if self.check(&Token::Amp) {
            self.advance();
            let is_mut = if self.check(&Token::Mut) {
                self.advance();
                true
            } else {
                false
            };
            let expr = self.parse_unary_inner(allow_struct_lit)?;
            let span = Span::new(start.start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Ref(is_mut, Box::new(expr)),
                span,
            });
        }
        
        if self.check(&Token::Star) {
            self.advance();
            let expr = self.parse_unary_inner(allow_struct_lit)?;
            let span = Span::new(start.start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Deref(Box::new(expr)),
                span,
            });
        }
        
        self.parse_postfix_inner(allow_struct_lit)
    }

    fn parse_postfix_inner(&mut self, allow_struct_lit: bool) -> ParseResult<Expr> {
        let mut expr = self.parse_primary_inner(allow_struct_lit)?;
        
        loop {
            if self.check(&Token::LParen) {
                // Function call
                self.advance();
                let args = self.parse_arg_list()?;
                let end = self.expect(Token::RParen)?;
                let span = Span::new(expr.span.start, end.span.end);
                expr = Expr {
                    kind: ExprKind::Call(Box::new(expr), args),
                    span,
                };
            } else if self.check(&Token::Dot) {
                // Field access
                self.advance();
                let field = self.expect_ident()?;
                let span = Span::new(expr.span.start, field.span.end);
                expr = Expr {
                    kind: ExprKind::Field(Box::new(expr), field),
                    span,
                };
            } else if self.check(&Token::LBracket) {
                // Index
                self.advance();
                let index = self.parse_expr()?;
                let end = self.expect(Token::RBracket)?;
                let span = Span::new(expr.span.start, end.span.end);
                expr = Expr {
                    kind: ExprKind::Index(Box::new(expr), Box::new(index)),
                    span,
                };
            } else if self.check(&Token::As) {
                // Type cast: expr as Type
                self.advance();
                let ty = self.parse_type()?;
                let span = Span::new(expr.span.start, ty.span.end);
                expr = Expr {
                    kind: ExprKind::Cast(Box::new(expr), ty),
                    span,
                };
            } else {
                break;
            }
        }
        
        Ok(expr)
    }

    fn parse_arg_list(&mut self) -> ParseResult<Vec<CallArg>> {
        let mut args = Vec::new();
        let mut seen_named = false;
        
        while !self.check(&Token::RParen) && !self.is_at_end() {
            let start = self.peek_span();
            
            // Check if this is a named argument: ident ':'
            let (name, value) = if let Token::Ident(id) = self.peek().clone() {
                // Peek ahead to see if there's a colon
                let current_pos = self.pos;
                self.advance();
                
                if self.check(&Token::Colon) {
                    // This is a named argument
                    self.advance(); // consume ':'
                    seen_named = true;
                    let name = Ident { name: id, span: start };
                    let value = self.parse_expr()?;
                    (Some(name), value)
                } else {
                    // Not a named argument, backtrack
                    self.pos = current_pos;
                    
                    if seen_named {
                        return Err(ParseError {
                            message: "positional arguments cannot follow named arguments".to_string(),
                            span: start,
                        });
                    }
                    
                    let value = self.parse_expr()?;
                    (None, value)
                }
            } else {
                if seen_named {
                    return Err(ParseError {
                        message: "positional arguments cannot follow named arguments".to_string(),
                        span: start,
                    });
                }
                
                let value = self.parse_expr()?;
                (None, value)
            };
            
            let end = value.span.end;
            args.push(CallArg {
                name,
                value,
                span: Span::new(start.start, end),
            });
            
            if !self.check(&Token::RParen) {
                self.expect(Token::Comma)?;
            }
        }
        
        Ok(args)
    }

    fn parse_primary_inner(&mut self, allow_struct_lit: bool) -> ParseResult<Expr> {
        let start = self.peek_span();
        
        match self.peek().clone() {
            Token::IntLiteral(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::IntLiteral(n),
                    span: start,
                })
            }
            Token::FloatLiteral(n) => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::FloatLiteral(n),
                    span: start,
                })
            }
            Token::True => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLiteral(true),
                    span: start,
                })
            }
            Token::False => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLiteral(false),
                    span: start,
                })
            }
            Token::StringLiteral(s) => {
                self.advance();
                // Check if this is an interpolated string
                if s.contains('{') {
                    self.parse_interpolated_string(&s, start)
                } else {
                    Ok(Expr {
                        kind: ExprKind::StringLiteral(s),
                        span: start,
                    })
                }
            }
            Token::SelfLower => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Ident(Ident::new("self".to_string(), start)),
                    span: start,
                })
            }
            Token::Ident(name) => {
                self.advance();
                let ident = Ident::new(name, start);
                
                // Check for struct literal: Ident { ... }
                // Only allowed when allow_struct_lit is true (not in if/while conditions)
                if allow_struct_lit && self.check(&Token::LBrace) {
                    return self.parse_struct_literal(ident);
                }
                
                Ok(Expr {
                    kind: ExprKind::Ident(ident),
                    span: start,
                })
            }
            Token::If => self.parse_if_expr(),
            Token::While => self.parse_while_expr(),
            Token::For => self.parse_for_expr(),
            Token::Match => self.parse_match_expr(),
            Token::LBrace => {
                let block = self.parse_block()?;
                let span = block.span;
                Ok(Expr {
                    kind: ExprKind::Block(block),
                    span,
                })
            }
            Token::LParen => {
                // Could be grouped expression or lambda
                // Try to parse as lambda first by looking ahead
                if let Some(lambda) = self.try_parse_lambda(start)? {
                    Ok(lambda)
                } else {
                    self.advance();
                    let expr = self.parse_expr()?;
                    self.expect(Token::RParen)?;
                    Ok(expr)
                }
            }
            Token::LBracket => {
                // Array literal: [1, 2, 3]
                self.advance();
                let mut elements = Vec::new();
                
                while !self.check(&Token::RBracket) && !self.is_at_end() {
                    elements.push(self.parse_expr()?);
                    
                    if !self.check(&Token::RBracket) {
                        self.expect(Token::Comma)?;
                    }
                }
                
                let end = self.expect(Token::RBracket)?;
                let span = Span::new(start.start, end.span.end);
                
                Ok(Expr {
                    kind: ExprKind::ArrayLit(elements),
                    span,
                })
            }
            _ => Err(ParseError {
                message: format!("expected expression, found '{}'", self.peek()),
                span: start,
            }),
        }
    }

    /// Try to parse a lambda expression. Returns None if not a lambda.
    /// Lambda syntax: (params) -> body
    fn try_parse_lambda(&mut self, start: Span) -> ParseResult<Option<Expr>> {
        // Save position for backtracking
        let saved_pos = self.pos;
        
        self.advance(); // consume '('
        
        // Try to parse parameters
        let mut params = Vec::new();
        
        while !self.check(&Token::RParen) && !self.is_at_end() {
            let param_start = self.peek_span();
            
            // Lambda params can be: ident or ident: type
            if let Token::Ident(name) = self.peek().clone() {
                self.advance();
                let name_ident = Ident { name, span: param_start };
                
                let ty = if self.check(&Token::Colon) {
                    self.advance();
                    Some(self.parse_type()?)
                } else {
                    None
                };
                
                let span = Span::new(param_start.start, self.peek_span().start);
                params.push(LambdaParam { name: name_ident, ty, span });
                
                if !self.check(&Token::RParen) {
                    if !self.check(&Token::Comma) {
                        // Not a valid lambda param list
                        self.pos = saved_pos;
                        return Ok(None);
                    }
                    self.advance();
                }
            } else {
                // Not an identifier, not a lambda
                self.pos = saved_pos;
                return Ok(None);
            }
        }
        
        if !self.check(&Token::RParen) {
            self.pos = saved_pos;
            return Ok(None);
        }
        self.advance(); // consume ')'
        
        // Check for '->'
        if !self.check(&Token::Arrow) {
            self.pos = saved_pos;
            return Ok(None);
        }
        self.advance(); // consume '->'
        
        // Parse body
        let body = self.parse_expr()?;
        let span = Span::new(start.start, body.span.end);
        
        Ok(Some(Expr {
            kind: ExprKind::Lambda(params, Box::new(body)),
            span,
        }))
    }

    fn parse_struct_literal(&mut self, name: Ident) -> ParseResult<Expr> {
        let start = name.span;
        self.expect(Token::LBrace)?;
        
        let mut fields = Vec::new();
        
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            let field_start = self.peek_span();
            let field_name = self.expect_ident()?;
            self.expect(Token::Colon)?;
            let value = self.parse_expr()?;
            
            let span = Span::new(field_start.start, value.span.end);
            fields.push(FieldInit { name: field_name, value, span });
            
            if !self.check(&Token::RBrace) {
                self.expect(Token::Comma)?;
            }
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(Expr {
            kind: ExprKind::StructLit(name, fields),
            span,
        })
    }

    fn parse_if_expr(&mut self) -> ParseResult<Expr> {
        let start = self.peek_span();
        self.expect(Token::If)?;
        
        let cond = self.parse_expr_no_struct()?;
        let then_block = self.parse_block()?;
        
        let else_branch = if self.check(&Token::Else) {
            self.advance();
            if self.check(&Token::If) {
                // else if
                let else_if = self.parse_if_expr()?;
                Some(ElseBranch::If(Box::new(else_if)))
            } else {
                Some(ElseBranch::Block(self.parse_block()?))
            }
        } else {
            None
        };
        
        let end_span = match &else_branch {
            Some(ElseBranch::Block(b)) => b.span,
            Some(ElseBranch::If(e)) => e.span,
            None => then_block.span,
        };
        let span = Span::new(start.start, end_span.end);
        
        Ok(Expr {
            kind: ExprKind::If(Box::new(cond), then_block, else_branch),
            span,
        })
    }

    fn parse_while_expr(&mut self) -> ParseResult<Expr> {
        let start = self.peek_span();
        self.expect(Token::While)?;
        
        let cond = self.parse_expr_no_struct()?;
        let body = self.parse_block()?;
        
        let span = Span::new(start.start, body.span.end);
        
        Ok(Expr {
            kind: ExprKind::While(Box::new(cond), body),
            span,
        })
    }

    fn parse_for_expr(&mut self) -> ParseResult<Expr> {
        let start = self.peek_span();
        self.expect(Token::For)?;
        
        let binding = self.expect_ident()?;
        self.expect(Token::In)?;
        let iter = self.parse_expr_no_struct()?;
        let body = self.parse_block()?;
        
        let span = Span::new(start.start, body.span.end);
        
        Ok(Expr {
            kind: ExprKind::For(binding, Box::new(iter), body),
            span,
        })
    }

    fn parse_match_expr(&mut self) -> ParseResult<Expr> {
        let start = self.peek_span();
        self.expect(Token::Match)?;
        
        let scrutinee = self.parse_expr_no_struct()?;
        self.expect(Token::LBrace)?;
        
        let mut arms = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            arms.push(self.parse_match_arm()?);
            
            // Optional trailing comma
            if self.check(&Token::Comma) {
                self.advance();
            }
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(Expr {
            kind: ExprKind::Match(Box::new(scrutinee), arms),
            span,
        })
    }

    fn parse_match_arm(&mut self) -> ParseResult<MatchArm> {
        let start = self.peek_span();
        let pattern = self.parse_pattern()?;
        self.expect(Token::Arrow)?;
        let body = self.parse_expr()?;
        
        let span = Span::new(start.start, body.span.end);
        Ok(MatchArm { pattern, body, span })
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        let start = self.peek_span();
        
        match self.peek().clone() {
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern {
                    kind: PatternKind::Wildcard,
                    span: start,
                })
            }
            Token::Ident(name) => {
                self.advance();
                let ident = Ident::new(name, start);
                
                // Check for variant pattern: Ident(patterns)
                if self.check(&Token::LParen) {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&Token::RParen) && !self.is_at_end() {
                        fields.push(self.parse_pattern()?);
                        if !self.check(&Token::RParen) {
                            self.expect(Token::Comma)?;
                        }
                    }
                    let end = self.expect(Token::RParen)?;
                    let span = Span::new(start.start, end.span.end);
                    return Ok(Pattern {
                        kind: PatternKind::Variant(ident, fields),
                        span,
                    });
                }
                
                Ok(Pattern {
                    kind: PatternKind::Ident(ident),
                    span: start,
                })
            }
            Token::IntLiteral(n) => {
                self.advance();
                Ok(Pattern {
                    kind: PatternKind::Literal(Expr {
                        kind: ExprKind::IntLiteral(n),
                        span: start,
                    }),
                    span: start,
                })
            }
            Token::True => {
                self.advance();
                Ok(Pattern {
                    kind: PatternKind::Literal(Expr {
                        kind: ExprKind::BoolLiteral(true),
                        span: start,
                    }),
                    span: start,
                })
            }
            Token::False => {
                self.advance();
                Ok(Pattern {
                    kind: PatternKind::Literal(Expr {
                        kind: ExprKind::BoolLiteral(false),
                        span: start,
                    }),
                    span: start,
                })
            }
            _ => Err(ParseError {
                message: format!("expected pattern, found '{}'", self.peek()),
                span: start,
            }),
        }
    }
    
    /// Parse an interpolated string like "hello {name}!"
    /// The string content has already been extracted from quotes
    fn parse_interpolated_string(&mut self, s: &str, span: Span) -> ParseResult<Expr> {
        let mut parts = Vec::new();
        let mut current_literal = String::new();
        let mut chars = s.chars().peekable();
        
        while let Some(c) = chars.next() {
            if c == '{' {
                // Check for escaped brace {{
                if chars.peek() == Some(&'{') {
                    chars.next();
                    current_literal.push('{');
                    continue;
                }
                
                // Save the current literal if non-empty
                if !current_literal.is_empty() {
                    parts.push(StringInterpPart::Literal(current_literal.clone()));
                    current_literal.clear();
                }
                
                // Extract the expression inside { }
                let mut expr_str = String::new();
                let mut brace_depth = 1;
                
                while let Some(c) = chars.next() {
                    if c == '{' {
                        brace_depth += 1;
                        expr_str.push(c);
                    } else if c == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            break;
                        }
                        expr_str.push(c);
                    } else {
                        expr_str.push(c);
                    }
                }
                
                if brace_depth != 0 {
                    return Err(ParseError {
                        message: "unclosed '{' in string interpolation".to_string(),
                        span,
                    });
                }
                
                // Parse the expression
                let expr = Parser::parse_interpolation_expr(&expr_str).map_err(|e| ParseError {
                    message: format!("error in interpolated expression: {}", e.message),
                    span,
                })?;
                
                parts.push(StringInterpPart::Expr(expr));
            } else if c == '}' {
                // Check for escaped brace }}
                if chars.peek() == Some(&'}') {
                    chars.next();
                    current_literal.push('}');
                } else {
                    return Err(ParseError {
                        message: "unmatched '}' in string".to_string(),
                        span,
                    });
                }
            } else {
                current_literal.push(c);
            }
        }
        
        // Add any remaining literal
        if !current_literal.is_empty() {
            parts.push(StringInterpPart::Literal(current_literal));
        }
        
        // If there's only one literal part, return a regular string
        if parts.len() == 1 {
            if let StringInterpPart::Literal(s) = &parts[0] {
                return Ok(Expr {
                    kind: ExprKind::StringLiteral(s.clone()),
                    span,
                });
            }
        }
        
        Ok(Expr {
            kind: ExprKind::StringInterp(parts),
            span,
        })
    }
    
    /// Parse a single expression from a string (for interpolation)
    pub fn parse_interpolation_expr(source: &str) -> ParseResult<Expr> {
        let mut parser = Parser::new(source)?;
        parser.parse_expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_fn() {
        let source = "fn main() { let x = 5 }";
        let ast = Parser::parse(source).unwrap();
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn test_parse_struct() {
        let source = "struct Point { x: i32, y: i32 }";
        let ast = Parser::parse(source).unwrap();
        assert_eq!(ast.items.len(), 1);
    }

    #[test]
    fn test_parse_enum() {
        let source = "enum Option { Some(value: i32), None }";
        let ast = Parser::parse(source).unwrap();
        assert_eq!(ast.items.len(), 1);
    }
}
