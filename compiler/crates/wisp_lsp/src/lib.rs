//! Wisp Language Server Protocol implementation

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::env;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use wisp_ast::{Item, SourceFile, StructField};
use wisp_lexer::Span;
use wisp_parser::{parse_with_imports, parse_with_imports_structured, Parser};
use wisp_hir::{DefId, Resolver};
use wisp_borrowck::BorrowChecker;

/// Information about a function
#[derive(Debug, Clone)]
struct FunctionInfo {
    signature: String,
    params: Vec<(String, String)>, // (name, type)
    span: Span,
    file: String,
}

/// Information about a struct
#[derive(Debug, Clone)]
struct StructInfo {
    name: String,
    fields: Vec<(String, String)>, // (name, type)
    definition: String,
    span: Span,
    file: String,
}

#[derive(Debug, Clone)]
struct TraitInfo {
    name: String,
    methods: Vec<String>, // Method signatures
    type_params: Vec<(String, Option<String>)>, // (name, default_type)
    definition: String,
    span: Span,
    file: String,
}

/// Information about a namespace
#[derive(Debug, Clone, Default)]
struct NamespaceInfo {
    /// Items in this namespace: name -> (kind, detail)
    items: HashMap<String, (String, String)>, // (kind like "function"/"struct", detail/signature)
    /// Child namespaces
    children: HashMap<String, NamespaceInfo>,
}

/// Document state stored by the LSP
#[derive(Debug, Default)]
struct DocumentState {
    /// Source text
    source: String,
    /// Type information: (start, end) -> type string
    type_info: HashMap<(usize, usize), String>,
    /// Definition mappings: (start, end) -> DefId for go-to-definition
    span_definitions: HashMap<(usize, usize), DefId>,
    /// DefId -> definition span for resolving go-to-definition targets
    def_spans: HashMap<DefId, Span>,
    /// Functions: name -> info
    functions: HashMap<String, FunctionInfo>,
    /// Structs: name -> info
    structs: HashMap<String, StructInfo>,
    /// Traits: name -> info
    traits: HashMap<String, TraitInfo>,
    /// Variable definitions: name -> (definition span start, definition span end)
    variable_defs: HashMap<String, (usize, usize)>,
    /// Namespaces: namespace_name -> info
    namespaces: HashMap<String, NamespaceInfo>,
    /// Available symbols from std: name -> module path
    std_symbols: HashMap<String, String>,
    /// Currently imported symbols (to avoid duplicate imports)
    imported_symbols: HashSet<String>,
    /// Diagnostics published for this document
    diagnostics: Vec<Diagnostic>,
}

/// The Wisp LSP backend
pub struct WispLanguageServer {
    client: Client,
    documents: RwLock<HashMap<Url, DocumentState>>,
}

impl WispLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: RwLock::new(HashMap::new()),
        }
    }

    /// Analyze a document and update its state
    async fn analyze_document(&self, uri: &Url, text: &str) {
        let mut diagnostics = Vec::new();
        let mut type_info = HashMap::new();
        let mut span_definitions = HashMap::new();
        let mut def_spans = HashMap::new();
        let mut functions = HashMap::new();
        let mut structs = HashMap::new();
        let mut traits = HashMap::new();
        let mut variable_defs = HashMap::new();
        
        // Preserve namespaces from previous successful analysis
        let previous_namespaces = if let Ok(docs) = self.documents.read() {
            docs.get(uri).map(|d| d.namespaces.clone()).unwrap_or_default()
        } else {
            HashMap::new()
        };

        // Get file path from URI
        let file_path = uri.to_file_path().unwrap_or_else(|_| PathBuf::from("."));
        let file_str = file_path.to_string_lossy().to_string();

        // Run parser with import resolution (structured for proper namespace handling)
        let base_dir = file_path.parent().unwrap_or(Path::new("."));
        
        // Debug: log the paths being used
        self.client.log_message(MessageType::INFO, format!(
            "LSP: Analyzing {} (base_dir: {})", 
            file_str, 
            base_dir.display()
        )).await;
        
        // First, try to parse with error recovery to collect all parse errors
        if let Ok(parse_result) = Parser::parse_with_recovery(text) {
            // Add all parse errors as diagnostics
            for err in &parse_result.errors {
                diagnostics.push(Diagnostic {
                    range: offset_to_range(text, err.span.start, err.span.end),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("wisp".to_string()),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
        }
        
        let mut visited = std::collections::HashSet::new();
        let ast_with_imports = match parse_with_imports_structured(text, base_dir, &mut visited) {
            Ok(ast) => ast,
            Err(err) => {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: 0, character: 0 },
                        end: Position { line: 0, character: 1 },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("wisp".to_string()),
                    message: err,
                    ..Default::default()
                });
                if let Ok(mut docs) = self.documents.write() {
                    // Keep old document state but update source
                    if let Some(old_doc) = docs.get_mut(uri) {
                        old_doc.source = text.to_string();
                    } else {
                        docs.insert(uri.clone(), DocumentState {
                            source: text.to_string(),
                            type_info,
                            span_definitions,
                            def_spans,
                            functions,
                            structs,
                            traits,
                            variable_defs,
                            namespaces: previous_namespaces.clone(),
                            std_symbols: HashMap::new(),
                            imported_symbols: HashSet::new(),
                            diagnostics: Vec::new(),
                        });
                    }
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };
        
        // Extract namespace info from parser result (works even if resolution fails later)
        let parser_namespaces = extract_namespaces_from_imports(&ast_with_imports);
        
        // Collect traits from imported modules NOW (before the flat parse might fail)
        // This ensures traits are available even if the current file has parse errors
        // Also populate std_symbols with all available std items
        let mut std_symbols = HashMap::new();
        let mut imported_symbols = HashSet::new();
        
        // Proactively load std modules to populate std_symbols for auto-import
        // This allows us to suggest imports even if the modules aren't loaded yet
        self.client.log_message(MessageType::INFO, format!(
            "LSP: About to call populate_std_symbols_sync, base_dir: {}", 
            base_dir.display()
        )).await;
        
        populate_std_symbols_sync(&mut std_symbols, base_dir);
        
        self.client.log_message(MessageType::INFO, format!(
            "LSP: After populate_std_symbols, found {} symbols", 
            std_symbols.len()
        )).await;
        
        // Log a few examples
        for (symbol, path) in std_symbols.iter().take(5) {
            self.client.log_message(MessageType::INFO, format!(
                "LSP:   {} -> {}", 
                symbol, path
            )).await;
        }
        
        for module in &ast_with_imports.imported_modules {
            // Determine the module path for std symbols
            let module_path = match &module.import.path {
                wisp_ast::ImportPath::Std(segments) => {
                    Some(format!("std.{}", segments.join(".")))
                }
                _ => None,
            };
            
            // Track what's being imported (to avoid duplicate import suggestions)
            // Only track items that are actually imported by this module's import declaration
            if !module.is_transitive {
                // Check if this is a direct item import (e.g., import std.io.print)
                // vs a namespace import (e.g., import std.io)
                if let Some(ref import_items) = module.import.items {
                    // Destructured import: only the listed items are imported
                    for import_item in import_items {
                        imported_symbols.insert(import_item.name.name.clone());
                    }
                } else if !module.import.destructure_only {
                    // Namespace import: all public items from this module are accessible
                    // Mark all items as imported
                    for item in &module.items {
                        let item_name = match item {
                            Item::Trait(t) if t.is_pub => Some(&t.name.name),
                            Item::Struct(s) if s.is_pub => Some(&s.name.name),
                            Item::Function(f) if f.is_pub => Some(&f.name.name),
                            Item::ExternFunction(f) if f.is_pub => Some(&f.name.name),
                            _ => None,
                        };
                        
                        if let Some(name) = item_name {
                            imported_symbols.insert(name.clone());
                        }
                    }
                }
            }
            
            for item in &module.items {
                // For std modules, track all public items for auto-import
                if let Some(ref path) = module_path {
                    if let Item::Trait(t) = item {
                        if t.is_pub {
                            std_symbols.insert(t.name.name.clone(), path.clone());
                        }
                    } else if let Item::Struct(s) = item {
                        if s.is_pub {
                            std_symbols.insert(s.name.name.clone(), path.clone());
                        }
                    } else if let Item::Function(f) = item {
                        if f.is_pub {
                            std_symbols.insert(f.name.name.clone(), path.clone());
                        }
                    }
                }
                
                if let Item::Trait(t) = item {
                    if t.is_pub {
                        // Collect type parameters with their defaults
                        let type_params: Vec<(String, Option<String>)> = t.type_params.iter()
                            .map(|p| (p.name.name.clone(), p.default.as_ref().map(|ty| ty.pretty_print())))
                            .collect();
                        
                        let methods: Vec<String> = t.methods.iter()
                            .map(|m| {
                                let params: Vec<String> = m.params.iter()
                                    .map(|p| {
                                        if p.name.name == "self" {
                                            // Use shorthand: &Self -> &self, &mut Self -> &mut self, Self -> self
                                            let ty_str = p.ty.pretty_print();
                                            if ty_str.starts_with("&mut ") {
                                                "&mut self".to_string()
                                            } else if ty_str.starts_with("&") {
                                                "&self".to_string()
                                            } else {
                                                "self".to_string()
                                            }
                                        } else {
                                            format!("{}: {}", p.name.name, p.ty.pretty_print())
                                        }
                                    })
                                    .collect();
                                let ret = m.return_type.as_ref()
                                    .map(|ty| format!(" -> {}", ty.pretty_print()))
                                    .unwrap_or_default();
                                format!("    fn {}({}){}", m.name.name, params.join(", "), ret)
                            })
                            .collect();
                        let definition = format!("trait {} {{\n{}\n}}", t.name.name, methods.join("\n"));
                        traits.insert(
                            t.name.name.clone(),
                            TraitInfo {
                                name: t.name.name.clone(),
                                methods,
                                type_params,
                                definition,
                                span: t.name.span,
                                file: "imported".to_string(),
                            }
                        );
                    }
                }
            }
        }
        
        // Debug: log imported modules
        self.client.log_message(MessageType::INFO, format!(
            "LSP: Found {} imported modules, {} local items", 
            ast_with_imports.imported_modules.len(),
            ast_with_imports.local_items.len()
        )).await;
        for (i, module) in ast_with_imports.imported_modules.iter().enumerate() {
            self.client.log_message(MessageType::INFO, format!(
                "  Module {}: {:?} (transitive: {}, {} module_imports)", 
                i,
                module.import.path,
                module.is_transitive,
                module.module_imports.len()
            )).await;
            for imp in &module.module_imports {
                self.client.log_message(MessageType::INFO, format!(
                    "    - module_import: {:?} (alias: {:?})", 
                    imp.path,
                    imp.alias.as_ref().map(|a| &a.name)
                )).await;
            }
        }
        
        // Also get flat AST for collecting function/struct/trait info
        // If parsing fails, preserve previous document state but still show diagnostics
        let ast = match parse_with_imports(text, &file_path) {
            Ok(ast) => ast,
            Err(_) => {
                // Parse error - preserve previous document state so hover/completion still works
                // on the parts that were previously valid
                if let Ok(mut docs) = self.documents.write() {
                    // Keep the old document state but update the source
                    if let Some(old_doc) = docs.get_mut(uri) {
                        old_doc.source = text.to_string();
                    } else {
                        // No previous state, insert empty state
                        docs.insert(uri.clone(), DocumentState {
                            source: text.to_string(),
                            type_info: HashMap::new(),
                            span_definitions: HashMap::new(),
                            def_spans: HashMap::new(),
                            functions: HashMap::new(),
                            structs: HashMap::new(),
                            traits: HashMap::new(),
                            variable_defs: HashMap::new(),
                            namespaces: parser_namespaces.clone(),
                            std_symbols: HashMap::new(),
                            imported_symbols: HashSet::new(),
                            diagnostics: Vec::new(),
                        });
                    }
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

        // Collect function, struct, and trait info from current file only
        // (Traits from imports were already collected above)
        for item in &ast.items {
            match item {
                Item::Function(func) => {
                    let params: Vec<(String, String)> = func.params.iter()
                        .map(|p| (p.name.name.clone(), p.ty.pretty_print()))
                        .collect();
                    let params_str: Vec<String> = params.iter()
                        .map(|(n, t)| format!("{}: {}", n, t))
                        .collect();
                    let ret = func.return_type.as_ref()
                        .map(|t| format!(" -> {}", t.pretty_print()))
                        .unwrap_or_default();
                    functions.insert(
                        func.name.name.clone(),
                        FunctionInfo {
                            signature: format!("fn {}({}){}", func.name.name, params_str.join(", "), ret),
                            params,
                            span: func.name.span,
                            file: file_str.clone(),
                        }
                    );
                }
                Item::Struct(s) => {
                    let fields: Vec<(String, String)> = s.fields.iter()
                        .map(|f| (f.name.name.clone(), f.ty.pretty_print()))
                        .collect();
                    let fields_str: Vec<String> = fields.iter()
                        .map(|(n, t)| format!("    {}: {}", n, t))
                        .collect();
                    structs.insert(
                        s.name.name.clone(),
                        StructInfo {
                            name: s.name.name.clone(),
                            fields,
                            definition: format!("struct {} {{\n{}\n}}", s.name.name, fields_str.join(",\n")),
                            span: s.name.span,
                            file: file_str.clone(),
                        }
                    );
                }
                Item::Trait(t) => {
                    let methods: Vec<String> = t.methods.iter()
                        .map(|m| {
                            let params: Vec<String> = m.params.iter()
                                .map(|p| {
                                    if p.name.name == "self" {
                                        // Use shorthand: &Self -> &self, &mut Self -> &mut self, Self -> self
                                        let ty_str = p.ty.pretty_print();
                                        if ty_str.starts_with("&mut ") {
                                            "&mut self".to_string()
                                        } else if ty_str.starts_with("&") {
                                            "&self".to_string()
                                        } else {
                                            "self".to_string()
                                        }
                                    } else {
                                        format!("{}: {}", p.name.name, p.ty.pretty_print())
                                    }
                                })
                                .collect();
                            let ret = m.return_type.as_ref()
                                .map(|ty| format!(" -> {}", ty.pretty_print()))
                                .unwrap_or_default();
                            format!("    fn {}({}){}", m.name.name, params.join(", "), ret)
                        })
                        .collect();
                    let definition = format!("trait {} {{\n{}\n}}", t.name.name, methods.join("\n"));
                    traits.insert(
                        t.name.name.clone(),
                        TraitInfo {
                            name: t.name.name.clone(),
                            methods,
                            type_params: Vec::new(), // Not collecting type params for local traits yet
                            definition,
                            span: t.name.span,
                            file: file_str.clone(),
                        }
                    );
                }
                Item::ExternFunction(func) => {
                    let params: Vec<(String, String)> = func.params.iter()
                        .map(|p| (p.name.name.clone(), p.ty.pretty_print()))
                        .collect();
                    let params_str: Vec<String> = params.iter()
                        .map(|(n, t)| format!("{}: {}", n, t))
                        .collect();
                    let ret = func.return_type.as_ref()
                        .map(|t| format!(" -> {}", t.pretty_print()))
                        .unwrap_or_default();
                    functions.insert(
                        func.name.name.clone(),
                        FunctionInfo {
                            signature: format!("extern fn {}({}){}", func.name.name, params_str.join(", "), ret),
                            params,
                            span: func.name.span,
                            file: file_str.clone(),
                        }
                    );
                }
                Item::Impl(imp) => {
                    // Collect methods from impl blocks
                    for method in &imp.methods {
                        let type_name = imp.target_type.pretty_print();
                        let params: Vec<(String, String)> = method.params.iter()
                            .map(|p| (p.name.name.clone(), p.ty.pretty_print()))
                            .collect();
                        let params_str: Vec<String> = params.iter()
                            .map(|(n, t)| format!("{}: {}", n, t))
                            .collect();
                        let ret = method.return_type.as_ref()
                            .map(|t| format!(" -> {}", t.pretty_print()))
                            .unwrap_or_default();
                        let method_name = format!("{}::{}", type_name, method.name.name);
                        functions.insert(
                            method_name.clone(),
                            FunctionInfo {
                                signature: format!("fn {}({}){}", method_name, params_str.join(", "), ret),
                                params,
                                span: method.name.span,
                                file: file_str.clone(),
                            }
                        );
                    }
                }
                _ => {}
            }
        }

        // Run name resolution with structured imports for proper namespace handling
        let resolved = match Resolver::resolve_with_imports(&ast_with_imports) {
            Ok(resolved) => resolved,
            Err(errors) => {
                for err in errors {
                    diagnostics.push(span_to_diagnostic(text, err.span, &err.message, DiagnosticSeverity::ERROR));
                }
                if let Ok(mut docs) = self.documents.write() {
                    docs.insert(uri.clone(), DocumentState {
                        source: text.to_string(),
                        type_info,
                        span_definitions,
                        def_spans,
                        functions,
                        structs,
                        traits,
                        variable_defs,
                        namespaces: parser_namespaces.clone(),
                        std_symbols: std_symbols.clone(),
                        imported_symbols: imported_symbols.clone(),
                        diagnostics: diagnostics.clone(),
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };
        
        // Debug: log namespaces from resolved program
        self.client.log_message(MessageType::INFO, format!(
            "LSP: Resolved program has {} namespaces: {:?}", 
            resolved.namespaces.len(),
            resolved.namespaces.keys().collect::<Vec<_>>()
        )).await;
        
        // Debug: log namespace children
        for (ns_name, ns_data) in &resolved.namespaces {
            self.client.log_message(MessageType::INFO, format!(
                "LSP: Namespace '{}' has {} items, {} children: {:?}", 
                ns_name,
                ns_data.items.len(),
                ns_data.children.len(),
                ns_data.children.keys().collect::<Vec<_>>()
            )).await;
        }

        // Run type checker
        let typed = match wisp_types::TypeChecker::check(&resolved) {
            Ok(typed) => typed,
            Err(errors) => {
                for err in errors {
                    diagnostics.push(span_to_diagnostic(text, err.span, &err.message, DiagnosticSeverity::ERROR));
                }
                // Collect namespace info even on type error
                let mut namespaces = HashMap::new();
                for (ns_name, ns_data) in &resolved.namespaces {
                    namespaces.insert(ns_name.clone(), convert_namespace_data(ns_data, &resolved));
                }
                if let Ok(mut docs) = self.documents.write() {
                    docs.insert(uri.clone(), DocumentState {
                        source: text.to_string(),
                        type_info,
                        span_definitions,
                        def_spans,
                        functions,
                        structs,
                        traits,
                        variable_defs,
                        namespaces,
                        std_symbols: std_symbols.clone(),
                        imported_symbols: imported_symbols.clone(),
                        diagnostics: diagnostics.clone(),
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

        // Use compiler's type info directly (recorded during type checking)
        // Filter to only include spans from the current file
        let source_len = text.len();
        for ((start, end), type_str) in typed.all_span_types() {
            if *end <= source_len {
                type_info.insert((*start, *end), type_str.clone());
            }
        }
        
        // Collect span→definition mappings from compiler
        for ((start, end), def_id) in typed.ctx.all_span_definitions() {
            if *end <= source_len {
                span_definitions.insert((*start, *end), *def_id);
            }
        }
        
        // Collect definition spans for resolving go-to-definition targets
        for func in &typed.functions {
            def_spans.insert(func.def_id, func.name_span);
        }
        for imp in &typed.impls {
            for method in &imp.methods {
                def_spans.insert(method.def_id, method.name_span);
            }
        }
        for s in &typed.structs {
            def_spans.insert(s.def_id, s.span);
        }
        
        // Collect named argument info from AST (named arg labels are not in typed AST)
        collect_named_args_from_ast(&ast, &functions, &mut type_info, source_len);
        
        // Collect variable definitions for go-to-definition
        collect_variable_defs(&typed, &mut variable_defs, source_len);
        
        // Collect namespace info from resolved program
        let mut namespaces = HashMap::new();
        for (ns_name, ns_data) in &resolved.namespaces {
            namespaces.insert(ns_name.clone(), convert_namespace_data(ns_data, &resolved));
        }

        // Run borrow checker
        let checker = BorrowChecker::new(&typed);
        if let Err(borrow_errors) = checker.check() {
            for err in borrow_errors {
                diagnostics.push(span_to_diagnostic(text, err.span, &err.message, DiagnosticSeverity::ERROR));
            }
        }

        // Store document state
        if let Ok(mut docs) = self.documents.write() {
            docs.insert(uri.clone(), DocumentState {
                source: text.to_string(),
                type_info,
                span_definitions,
                def_spans,
                functions,
                structs,
                traits,
                variable_defs,
                namespaces,
                std_symbols,
                imported_symbols,
                diagnostics: diagnostics.clone(),
            });
        }

        // Publish diagnostics
        self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for WispLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
                    code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                    work_done_progress_options: Default::default(),
                    resolve_provider: None,
                })),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "wisp-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Wisp LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.analyze_document(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().next() {
            self.analyze_document(&uri, &change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Ok(mut docs) = self.documents.write() {
            docs.remove(&params.text_document.uri);
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                let offset = position_to_offset(&doc.source, position);
                let word = get_word_at_offset(&doc.source, offset);
                
                // Check for function
                if let Some(info) = doc.functions.get(&word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```wisp\n{}\n```", info.signature),
                        }),
                        range: None,
                    }));
                }
                
                // Check for struct
                if let Some(info) = doc.structs.get(&word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```wisp\n{}\n```", info.definition),
                        }),
                        range: None,
                    }));
                }
                
                // Check for trait
                if let Some(info) = doc.traits.get(&word) {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```wisp\n{}\n```", info.definition),
                        }),
                        range: None,
                    }));
                }
                
                // Check type info - find the smallest span containing the offset
                let mut best_match: Option<(usize, usize, &String)> = None;
                for ((start, end), type_str) in &doc.type_info {
                    if offset >= *start && offset <= *end {
                        let span_size = end - start;
                        if best_match.is_none() || span_size < (best_match.as_ref().unwrap().1 - best_match.as_ref().unwrap().0) {
                            best_match = Some((*start, *end, type_str));
                        }
                    }
                }
                
                if let Some((start, end, type_str)) = best_match {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: format!("```wisp\n{}\n```", type_str),
                        }),
                        range: Some(offset_to_range(&doc.source, start, end)),
                    }));
                }
            }
        }

        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                let offset = position_to_offset(&doc.source, position);
                let word = get_word_at_offset(&doc.source, offset);
                
                // First, try to use the compiler's span→definition mappings
                // Find the smallest span containing the cursor
                let mut best_span: Option<((usize, usize), DefId)> = None;
                for ((start, end), def_id) in &doc.span_definitions {
                    if offset >= *start && offset <= *end {
                        let span_size = end - start;
                        if best_span.is_none() || span_size < (best_span.as_ref().unwrap().0.1 - best_span.as_ref().unwrap().0.0) {
                            best_span = Some(((*start, *end), *def_id));
                        }
                    }
                }
                
                if let Some((_, def_id)) = best_span {
                    // Look up the definition span
                    if let Some(def_span) = doc.def_spans.get(&def_id) {
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                            uri: uri.clone(),
                            range: offset_to_range(&doc.source, def_span.start, def_span.end),
                        })));
                    }
                }
                
                // Helper to create goto response from FunctionInfo
                let make_response = |info: &FunctionInfo| -> Option<GotoDefinitionResponse> {
                    let target_uri = if info.file.is_empty() || info.file == uri.to_file_path().unwrap_or_default().to_string_lossy() {
                        uri.clone()
                    } else {
                        Url::from_file_path(&info.file).unwrap_or_else(|_| uri.clone())
                    };
                    
                    let target_source = if target_uri == *uri {
                        doc.source.clone()
                    } else {
                        fs::read_to_string(&info.file).unwrap_or_default()
                    };
                    
                    Some(GotoDefinitionResponse::Scalar(Location {
                        uri: target_uri,
                        range: offset_to_range(&target_source, info.span.start, info.span.end),
                    }))
                };
                
                // Fallback: Check for direct function definition by name
                if let Some(info) = doc.functions.get(&word) {
                    return Ok(make_response(info));
                }
                
                // Check for method/associated function: look for "Type::method" pattern
                // Find if there's a Type before the dot
                let type_name = get_type_before_dot(&doc.source, offset);
                if let Some(type_name) = type_name {
                    let qualified_name = format!("{}::{}", type_name, word);
                    if let Some(info) = doc.functions.get(&qualified_name) {
                        return Ok(make_response(info));
                    }
                }
                
                // Check for struct definition
                if let Some(info) = doc.structs.get(&word) {
                    let target_uri = if info.file.is_empty() || info.file == uri.to_file_path().unwrap_or_default().to_string_lossy() {
                        uri.clone()
                    } else {
                        Url::from_file_path(&info.file).unwrap_or_else(|_| uri.clone())
                    };
                    
                    let target_source = if target_uri == *uri {
                        doc.source.clone()
                    } else {
                        fs::read_to_string(&info.file).unwrap_or_default()
                    };
                    
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: target_uri,
                        range: offset_to_range(&target_source, info.span.start, info.span.end),
                    })));
                }
                
                // Check for local variable definition
                if let Some(&(start, end)) = doc.variable_defs.get(&word) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                        uri: uri.clone(),
                        range: offset_to_range(&doc.source, start, end),
                    })));
                }
            }
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        
        if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                let mut items = Vec::new();
                let offset = position_to_offset(&doc.source, position);
                
                // Check if we're inside a struct literal (look for StructName { before cursor)
                let before_cursor = &doc.source[..offset];
                
                // Debug: log completion context
                eprintln!("LSP Completion: offset={}, before_cursor ends with: {:?}", 
                    offset, 
                    before_cursor.chars().rev().take(20).collect::<String>().chars().rev().collect::<String>());
                eprintln!("LSP Completion: namespaces available: {:?}", doc.namespaces.keys().collect::<Vec<_>>());
                
                // Check if we're in an import statement
                if let Some(import_path) = find_import_context(before_cursor) {
                    eprintln!("LSP Completion: import path: {:?}", import_path);
                    let suggestions = get_import_suggestions(uri, &import_path);
                    for (idx, (name, kind)) in suggestions.iter().enumerate() {
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(*kind),
                            // Use leading zeros to sort imports first, then alphabetically
                            sort_text: Some(format!("00{:03}{}", idx, name)),
                            ..Default::default()
                        });
                    }
                    if !items.is_empty() {
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                }
                
                // Check if we're in an impl Trait for Type block
                // This triggers when typing anywhere in the impl block, especially after "fn "
                if let Some((trait_name, impl_type_args)) = find_impl_trait_context(before_cursor) {
                    // Check if we're in a position where we'd want to add a method
                    // (after "fn" keyword or at the start of a line in the impl block)
                    let last_line = before_cursor.lines().last().unwrap_or("");
                    let trimmed_line = last_line.trim();
                    
                    // Trigger if we see "fn" at the start of the line or after whitespace
                    if trimmed_line.starts_with("fn") || trimmed_line.is_empty() {
                        // Suggest trait methods
                        if let Some(trait_info) = doc.traits.get(&trait_name) {
                            for method_sig in &trait_info.methods {
                                // Substitute type parameters in the method signature
                                let substituted_sig = substitute_trait_type_params(
                                    method_sig,
                                    &trait_info.type_params,
                                    &impl_type_args,
                                );
                                
                                // Extract method name from signature like "    fn to_string(&self) -> String"
                                let method_name = substituted_sig.trim().strip_prefix("fn ")
                                    .and_then(|s| s.split('(').next())
                                    .unwrap_or("");
                                
                                // Strip "fn " prefix and add body with cursor inside
                                let method_without_fn = substituted_sig.trim().strip_prefix("fn ").unwrap_or(substituted_sig.trim());
                                
                                items.push(CompletionItem {
                                    label: method_name.to_string(),
                                    kind: Some(CompletionItemKind::METHOD),
                                    detail: Some(substituted_sig.trim().to_string()),
                                    insert_text: Some(format!("{} {{\n    $0\n}}", method_without_fn)),
                                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                                    sort_text: Some(format!("0{}", method_name)), // Sort first
                                    ..Default::default()
                                });
                            }
                            if !items.is_empty() {
                                return Ok(Some(CompletionResponse::Array(items)));
                            }
                        }
                    }
                }
                
                if let Some(struct_context) = find_struct_literal_context(before_cursor) {
                    // We're inside a struct literal, suggest fields
                    if let Some(struct_info) = doc.structs.get(&struct_context) {
                        for (field_name, field_type) in &struct_info.fields {
                            items.push(CompletionItem {
                                label: field_name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(field_type.clone()),
                                insert_text: Some(format!("{}: ", field_name)),
                                ..Default::default()
                            });
                        }
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                }
                
                // Check if we're accessing a namespace (e.g., std. or io.)
                let ns_context = find_namespace_context(before_cursor);
                eprintln!("LSP Completion: find_namespace_context returned: {:?}", ns_context);
                
                if let Some(ns_path) = ns_context {
                    eprintln!("LSP Completion: Looking up namespace path {:?}", ns_path);
                    // Look up the namespace
                    if let Some(ns_info) = lookup_namespace_path(&doc.namespaces, &ns_path) {
                        eprintln!("LSP Completion: Found namespace with {} items, {} children", 
                            ns_info.items.len(), ns_info.children.len());
                        // Suggest items from this namespace
                        for (name, (kind, _detail)) in &ns_info.items {
                            let completion_kind = match kind.as_str() {
                                "function" | "extern fn" => CompletionItemKind::FUNCTION,
                                "struct" => CompletionItemKind::STRUCT,
                                "enum" => CompletionItemKind::ENUM,
                                "trait" => CompletionItemKind::INTERFACE,
                                _ => CompletionItemKind::VARIABLE,
                            };
                            items.push(CompletionItem {
                                label: name.clone(),
                                kind: Some(completion_kind),
                                detail: Some(kind.clone()),
                                sort_text: Some(format!("0{}", name)), // Sort before keywords
                                ..Default::default()
                            });
                        }
                        // Suggest child namespaces
                        for child_name in ns_info.children.keys() {
                            items.push(CompletionItem {
                                label: child_name.clone(),
                                kind: Some(CompletionItemKind::MODULE),
                                detail: Some("namespace".to_string()),
                                sort_text: Some(format!("0{}", child_name)),
                                ..Default::default()
                            });
                        }
                        if !items.is_empty() {
                            return Ok(Some(CompletionResponse::Array(items)));
                        }
                    }
                }
                
                // Check if we're in a function call (look for function_name( before cursor)
                if let Some(func_context) = find_function_call_context(before_cursor) {
                    if let Some(func_info) = doc.functions.get(&func_context) {
                        // Show named argument suggestions
                        for (param_name, param_type) in &func_info.params {
                            items.push(CompletionItem {
                                label: format!("{}: ", param_name),
                                kind: Some(CompletionItemKind::VARIABLE),
                                detail: Some(param_type.clone()),
                                insert_text: Some(format!("{}: ", param_name)),
                                sort_text: Some(format!("0{}", param_name)), // Sort before other completions
                                ..Default::default()
                            });
                        }
                        if !items.is_empty() {
                            return Ok(Some(CompletionResponse::Array(items)));
                        }
                    }
                }
                
                // Suggest top-level namespaces
                for ns_name in doc.namespaces.keys() {
                    items.push(CompletionItem {
                        label: ns_name.clone(),
                        kind: Some(CompletionItemKind::MODULE),
                        detail: Some("namespace".to_string()),
                        ..Default::default()
                    });
                }
                
                // Default: suggest all functions, structs, and keywords
                for (name, info) in &doc.functions {
                    let (insert_text, insert_format) = if info.params.is_empty() {
                        (format!("{}()", name), None)
                    } else {
                        (format!("{}($0)", name), Some(InsertTextFormat::SNIPPET))
                    };
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(info.signature.clone()),
                        insert_text: Some(insert_text),
                        insert_text_format: insert_format,
                        ..Default::default()
                    });
                }
                
                for (name, info) in &doc.structs {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::STRUCT),
                        detail: Some(info.definition.lines().next().unwrap_or("").to_string()),
                        // Insert struct literal template with cursor inside
                        insert_text: Some(format!("{} {{ $0 }}", name)),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    });
                }
                
                // Keywords
                for kw in &["fn", "let", "mut", "if", "else", "while", "loop", "return", 
                           "break", "continue", "struct", "enum", "trait", "impl", 
                           "true", "false", "self", "pub", "extern", "import"] {
                    items.push(CompletionItem {
                        label: kw.to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        ..Default::default()
                    });
                }
                
                // Types
                for ty in &["i32", "i64", "f32", "f64", "bool", "str", "u8", "u16", "u32", "u64"] {
                    items.push(CompletionItem {
                        label: ty.to_string(),
                        kind: Some(CompletionItemKind::TYPE_PARAMETER),
                        ..Default::default()
                    });
                }
                
                return Ok(Some(CompletionResponse::Array(items)));
            }
        }

        Ok(None)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        
        if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                let offset = position_to_offset(&doc.source, position);
                let before_cursor = &doc.source[..offset];
                
                // Find function call context
                if let Some(func_name) = find_function_call_context(before_cursor) {
                    if let Some(func_info) = doc.functions.get(&func_name) {
                        let params: Vec<ParameterInformation> = func_info.params.iter()
                            .map(|(name, ty)| ParameterInformation {
                                label: ParameterLabel::Simple(format!("{}: {}", name, ty)),
                                documentation: None,
                            })
                            .collect();
                        
                        // Count commas to determine active parameter
                        let paren_start = before_cursor.rfind('(').unwrap_or(0);
                        let after_paren = &before_cursor[paren_start..];
                        let active_param = after_paren.matches(',').count() as u32;
                        
                        return Ok(Some(SignatureHelp {
                            signatures: vec![SignatureInformation {
                                label: func_info.signature.clone(),
                                documentation: None,
                                parameters: Some(params),
                                active_parameter: Some(active_param),
                            }],
                            active_signature: Some(0),
                            active_parameter: Some(active_param),
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        
        self.client.log_message(MessageType::INFO, format!(
            "LSP: code_action called with {} diagnostics, range: {:?}", 
            params.context.diagnostics.len(),
            params.range
        )).await;
        
        // Clone the data we need before doing any awaits to avoid holding the RwLock
        let (source, std_symbols, imported_symbols, stored_diagnostics) = if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                (doc.source.clone(), doc.std_symbols.clone(), doc.imported_symbols.clone(), doc.diagnostics.clone())
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        };
        
        self.client.log_message(MessageType::INFO, format!(
            "LSP: Found document, {} std_symbols available, {} imported, {} stored diagnostics", 
            std_symbols.len(),
            imported_symbols.len(),
            stored_diagnostics.len()
        )).await;
        
        // Log some example std_symbols
        for (symbol, path) in std_symbols.iter().take(3) {
            self.client.log_message(MessageType::INFO, format!(
                "LSP:   std_symbol: {} -> {}", 
                symbol, path
            )).await;
        }
        
        let mut actions = Vec::new();
        
        // Use diagnostics from context if provided, otherwise use stored diagnostics in range
        let diagnostics_to_check: Vec<&Diagnostic> = if !params.context.diagnostics.is_empty() {
            params.context.diagnostics.iter().collect()
        } else {
            // Filter stored diagnostics that overlap with the requested range
            stored_diagnostics.iter()
                .filter(|d| ranges_overlap(&d.range, &params.range))
                .collect()
        };
        
        if diagnostics_to_check.is_empty() {
            self.client.log_message(MessageType::INFO, 
                "LSP: No diagnostics in range for code actions".to_string()
            ).await;
            return Ok(None);
        }
        
        self.client.log_message(MessageType::INFO, format!(
            "LSP: Checking {} diagnostics for code actions", 
            diagnostics_to_check.len()
        )).await;
        
        // First, parse existing imports to find what modules are already imported
        let mut existing_imports: HashMap<String, (usize, usize, bool, Vec<String>)> = HashMap::new(); 
        // module_path -> (line_idx, line_start_offset, has_destructure, items)
        
        let mut current_offset = 0;
        for (line_idx, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") {
                // Parse: "import std.io.{ Display }" or "import std.io.Display"
                if let Some(rest) = trimmed.strip_prefix("import ") {
                    if let Some(brace_start) = rest.find(".{") {
                        // Destructured import: import std.io.{ Display, print }
                        let module_path = rest[..brace_start].trim().to_string();
                        if let Some(brace_end) = rest.find('}') {
                            let items_str = &rest[brace_start + 2..brace_end];
                            let items: Vec<String> = items_str.split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            existing_imports.insert(module_path, (line_idx, current_offset, true, items));
                        }
                    } else if let Some(last_dot) = rest.rfind('.') {
                        // Direct import: import std.io.Display
                        let module_path = rest[..last_dot].trim().to_string();
                        let item = rest[last_dot + 1..].trim().to_string();
                        existing_imports.entry(module_path)
                            .or_insert((line_idx, current_offset, false, vec![]))
                            .3.push(item);
                    }
                }
            }
            current_offset += line.len() + 1; // +1 for newline
        }
        
        // Check each diagnostic in the range for unresolved symbols
        for diagnostic in diagnostics_to_check {
            // Parse error messages like:
            // - "undefined trait 'Add'"
            // - "undefined type 'String'"
            // - "undefined variable 'print'"
            // - "undefined struct 'Point'"
            let message = &diagnostic.message;
            
            self.client.log_message(MessageType::INFO, format!(
                "LSP: Processing diagnostic: {}", 
                message
            )).await;
            
            // Extract symbol name from error message
            let symbol_name = if let Some(start) = message.find('\'') {
                if let Some(end) = message[start+1..].find('\'') {
                    Some(&message[start+1..start+1+end])
                } else {
                    None
                }
            } else {
                None
            };
            
            if let Some(symbol) = symbol_name {
                self.client.log_message(MessageType::INFO, format!(
                    "LSP: Extracted symbol: {}, imported: {}, in std: {}", 
                    symbol,
                    imported_symbols.contains(symbol),
                    std_symbols.contains_key(symbol)
                )).await;
                
                // Check if this symbol is available in std but not imported
                if !imported_symbols.contains(symbol) {
                    if let Some(module_path) = std_symbols.get(symbol) {
                        self.client.log_message(MessageType::INFO, format!(
                            "LSP: Creating code action for {} from {}", 
                            symbol,
                            module_path
                        )).await;
                        
                        // Check if we already have an import from this module
                        if let Some((line_idx, line_start, has_destructure, existing_items)) = existing_imports.get(module_path) {
                            self.client.log_message(MessageType::INFO, format!(
                                "LSP: Found existing import from {} at line {}", 
                                module_path, line_idx
                            )).await;
                            
                            // Calculate line end offset
                            let line = source.lines().nth(*line_idx).unwrap();
                            let line_end = line_start + line.len();
                            
                            let new_line = if *has_destructure {
                                // Already has destructure syntax: import std.io.{ Display } -> import std.io.{ Display, print }
                                let mut all_items = existing_items.clone();
                                all_items.push(symbol.to_string());
                                format!("import {}.{{ {} }}", module_path, all_items.join(", "))
                            } else {
                                // Multiple direct imports: convert to destructure
                                // import std.io.Display + import std.io.print -> import std.io.{ Display, print }
                                let mut all_items = existing_items.clone();
                                all_items.push(symbol.to_string());
                                format!("import {}.{{ {} }}", module_path, all_items.join(", "))
                            };
                            
                            let mut changes = HashMap::new();
                            changes.insert(
                                uri.clone(),
                                vec![TextEdit {
                                    range: Range {
                                        start: offset_to_position(&source, *line_start),
                                        end: offset_to_position(&source, line_end),
                                    },
                                    new_text: new_line,
                                }],
                            );
                            
                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("Add {} to existing import", symbol),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diagnostic.clone()]),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(changes),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }));
                        } else {
                            // No existing import from this module, create a new one
                            let insert_pos = find_import_insertion_point(&source);
                            let insert_line = offset_to_position(&source, insert_pos);
                            
                            // Create the import statement
                            let import_statement = format!("import {}.{}\n", module_path, symbol);
                            
                            // Create the code action
                            let mut changes = HashMap::new();
                            changes.insert(
                                uri.clone(),
                                vec![TextEdit {
                                    range: Range {
                                        start: insert_line,
                                        end: insert_line,
                                    },
                                    new_text: import_statement,
                                }],
                            );
                            
                            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                                title: format!("Import {}.{}", module_path, symbol),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diagnostic.clone()]),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(changes),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            }));
                        }
                    }
                }
            }
        }
        
        if !actions.is_empty() {
            return Ok(Some(actions));
        }
        
        Ok(None)
    }
}

/// Substitute trait type parameters in a method signature
/// 
/// - If impl provides type args (e.g., `Add<i32>`), use those
/// - Otherwise, use the trait's default type parameters
/// - Replace `Self` in return types with the implementing type
fn substitute_trait_type_params(
    method_sig: &str,
    trait_type_params: &[(String, Option<String>)],
    impl_type_args: &[String],
) -> String {
    let mut result = method_sig.to_string();
    
    // Substitute each type parameter
    for (i, (param_name, default_type)) in trait_type_params.iter().enumerate() {
        let replacement = if i < impl_type_args.len() {
            // Use the provided type arg
            &impl_type_args[i]
        } else if let Some(default) = default_type {
            // Use the default
            default
        } else {
            // No default, leave as-is
            continue;
        };
        
        // Replace the type parameter name with the concrete type
        // Match whole words only (to avoid replacing parts of identifiers)
        let pattern = format!(r"\b{}\b", regex::escape(param_name));
        if let Ok(re) = regex::Regex::new(&pattern) {
            result = re.replace_all(&result, replacement).to_string();
        }
    }
    
    result
}

/// Find if we're inside a struct literal and return the struct name
/// Check if we're inside an `impl Trait for Type` block
/// Returns the trait name and its type arguments if found
fn find_impl_trait_context(text: &str) -> Option<(String, Vec<String>)> {
    // Look for pattern: impl TraitName for TypeName { ... 
    // We need to find the most recent "impl" block we're inside
    
    // Count braces to make sure we're inside an impl block
    let mut brace_depth = 0;
    let mut impl_start: Option<usize> = None;
    
    for (i, ch) in text.char_indices().rev() {
        match ch {
            '}' => brace_depth += 1,
            '{' => {
                brace_depth -= 1;
                if brace_depth < 0 {
                    // Found the opening brace of a block, check if it's an impl block
                    impl_start = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    
    if let Some(start) = impl_start {
        // Look backwards from the brace to find "impl TraitName for TypeName"
        let before_brace = text[..start].trim_end();
        
        // Pattern: impl TraitName for TypeName or impl TraitName<TypeArgs> for TypeName
        if let Some(impl_pos) = before_brace.rfind("impl ") {
            let after_impl = &before_brace[impl_pos + 5..].trim();
            
            // Extract: TraitName for TypeName or TraitName<TypeArgs> for TypeName
            if let Some(for_pos) = after_impl.find(" for ") {
                let trait_part = after_impl[..for_pos].trim();
                
                // Check if trait has type arguments: TraitName<TypeArgs>
                if let Some(angle_start) = trait_part.find('<') {
                    let trait_name = trait_part[..angle_start].trim();
                    
                    // Extract type arguments from <TypeArgs>
                    if let Some(angle_end) = trait_part.rfind('>') {
                        let type_args_str = &trait_part[angle_start + 1..angle_end];
                        // Split by comma and trim each
                        let type_args: Vec<String> = type_args_str
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        
                        if !trait_name.is_empty() {
                            return Some((trait_name.to_string(), type_args));
                        }
                    }
                } else {
                    // No type arguments, just the trait name
                    if !trait_part.is_empty() && trait_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        return Some((trait_part.to_string(), Vec::new()));
                    }
                }
            }
        }
    }
    
    None
}

fn find_struct_literal_context(text: &str) -> Option<String> {
    // Look backwards for pattern: StructName {
    // But not inside a nested block
    let mut brace_depth = 0;
    let mut i = text.len();
    
    for ch in text.chars().rev() {
        i -= ch.len_utf8();
        match ch {
            '}' => brace_depth += 1,
            '{' => {
                if brace_depth == 0 {
                    // Found opening brace, look for struct name before it
                    let before_brace = text[..i].trim_end();
                    // Get the last word before the brace
                    let word_start = before_brace.rfind(|c: char| !c.is_alphanumeric() && c != '_')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let word = &before_brace[word_start..];
                    if !word.is_empty() && word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        return Some(word.to_string());
                    }
                    return None;
                }
                brace_depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Find if we're inside a function call and return the function name
fn find_function_call_context(text: &str) -> Option<String> {
    // Look backwards for pattern: function_name(
    let mut paren_depth = 0;
    let mut i = text.len();
    
    for ch in text.chars().rev() {
        i -= ch.len_utf8();
        match ch {
            ')' => paren_depth += 1,
            '(' => {
                if paren_depth == 0 {
                    // Found opening paren, look for function name before it
                    let before_paren = text[..i].trim_end();
                    let word_start = before_paren.rfind(|c: char| !c.is_alphanumeric() && c != '_')
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let word = &before_paren[word_start..];
                    if !word.is_empty() {
                        return Some(word.to_string());
                    }
                    return None;
                }
                paren_depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Find if we're after a namespace dot (e.g., "std." or "std.io.")
/// Returns the namespace path as a vector of segments
fn find_namespace_context(text: &str) -> Option<Vec<String>> {
    // Look for pattern: identifier. or identifier.identifier. etc. at the end
    let trimmed = text.trim_end();
    if !trimmed.ends_with('.') {
        return None;
    }
    
    // Get the part before the last dot
    let before_dot = &trimmed[..trimmed.len() - 1];
    
    // Collect the namespace path by going backwards through dots
    let mut path = Vec::new();
    let mut current = before_dot;
    
    loop {
        // Find the last identifier
        let word_start = current.rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &current[word_start..];
        
        if word.is_empty() {
            break;
        }
        
        path.push(word.to_string());
        
        // Check if there's a dot before this word
        let before_word = current[..word_start].trim_end();
        if before_word.ends_with('.') {
            current = &before_word[..before_word.len() - 1];
        } else {
            break;
        }
    }
    
    if path.is_empty() {
        None
    } else {
        path.reverse();
        Some(path)
    }
}

/// Look up a namespace by path
fn lookup_namespace_path<'a>(namespaces: &'a HashMap<String, NamespaceInfo>, path: &[String]) -> Option<&'a NamespaceInfo> {
    if path.is_empty() {
        return None;
    }
    
    let mut current = namespaces.get(&path[0])?;
    for segment in &path[1..] {
        current = current.children.get(segment)?;
    }
    Some(current)
}

/// Detect if we're in an import statement and return the import path being typed
fn find_import_context(text: &str) -> Option<String> {
    let trimmed = text.trim_end();
    
    // Check if we're after "import "
    if let Some(import_pos) = trimmed.rfind("import ") {
        let after_import = &trimmed[import_pos + 7..]; // Skip "import "
        
        // Skip if we're in destructure syntax: import { ... } from
        if after_import.starts_with('{') {
            // Check for "from" pattern
            if let Some(from_pos) = after_import.rfind("from ") {
                return Some(after_import[from_pos + 5..].trim().to_string());
            }
            return None;
        }
        
        return Some(after_import.trim().to_string());
    }
    
    None
}

/// Get import suggestions by scanning the filesystem and parsing modules for items
fn get_import_suggestions(uri: &Url, current_path: &str) -> Vec<(String, CompletionItemKind)> {
    let mut suggestions = Vec::new();
    
    // Get the source file path for resolving relative to project
    let file_path = uri.to_file_path().ok();
    
    if current_path.is_empty() {
        // Just typed "import " - suggest prefixes
        suggestions.push(("std".to_string(), CompletionItemKind::MODULE));
        suggestions.push(("@".to_string(), CompletionItemKind::MODULE));
        return suggestions;
    }
    
    // Check if the path ends with a dot (e.g., "std.io.")
    // In this case, suggest items from that module
    if current_path.ends_with('.') {
        if let Some(item_suggestions) = get_module_item_suggestions(file_path.as_ref(), current_path) {
            return item_suggestions;
        }
    }
    
    // Parse the current path to determine what to suggest
    if current_path.starts_with("std.") || current_path == "std" {
        // Scan std directory
        let std_path = get_std_path(file_path.as_ref());
        let subpath = if current_path.starts_with("std.") {
            &current_path[4..]
        } else {
            ""
        };
        suggestions.extend(scan_module_directory(&std_path, subpath));
    } else if current_path.starts_with("@.") || current_path == "@" {
        // Scan project directory
        if let Some(project_root) = file_path.as_ref().and_then(|p| find_project_root(p)) {
            let subpath = if current_path.starts_with("@.") {
                &current_path[2..]
            } else {
                ""
            };
            suggestions.extend(scan_module_directory(&project_root, subpath));
        }
    } else if current_path == "s" || current_path == "st" || current_path == "std" {
        // Partial "std" - suggest it
        if "std".starts_with(current_path) {
            suggestions.push(("std".to_string(), CompletionItemKind::MODULE));
        }
    } else if current_path == "@" {
        suggestions.push(("@".to_string(), CompletionItemKind::MODULE));
    }
    
    suggestions
}

/// Get item suggestions from a specific module
/// e.g., "import std.io." -> suggest print, Display, etc.
fn get_module_item_suggestions(file_path: Option<&PathBuf>, import_path: &str) -> Option<Vec<(String, CompletionItemKind)>> {
    // Remove trailing dot
    let path_without_dot = import_path.trim_end_matches('.');
    
    // Resolve the module file
    let module_file = resolve_import_path_for_completion(file_path, path_without_dot)?;
    
    // Parse the module to extract public items
    let source = fs::read_to_string(&module_file).ok()?;
    let ast = wisp_parser::Parser::parse(&source).ok()?;
    
    let mut items = Vec::new();
    
    for item in ast.items {
        let (name, kind, is_pub) = match item {
            wisp_ast::Item::Function(f) => (f.name.name, CompletionItemKind::FUNCTION, f.is_pub),
            wisp_ast::Item::Struct(s) => (s.name.name, CompletionItemKind::STRUCT, s.is_pub),
            wisp_ast::Item::Enum(e) => (e.name.name, CompletionItemKind::ENUM, e.is_pub),
            wisp_ast::Item::Trait(t) => (t.name.name, CompletionItemKind::INTERFACE, t.is_pub),
            wisp_ast::Item::Impl(_) => continue, // Skip impls
            wisp_ast::Item::Import(_) => continue, // Skip imports
            wisp_ast::Item::ExternFunction(f) => (f.name.name, CompletionItemKind::FUNCTION, f.is_pub),
            wisp_ast::Item::ExternStatic(s) => (s.name.name, CompletionItemKind::VARIABLE, s.is_pub),
        };
        
        if is_pub {
            items.push((name, kind));
        }
    }
    
    Some(items)
}

/// Resolve an import path string to a file path for completion purposes
fn resolve_import_path_for_completion(file_path: Option<&PathBuf>, import_path: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = import_path.split('.').collect();
    if parts.is_empty() {
        return None;
    }
    
    let base_path = if parts[0] == "std" {
        get_std_path(file_path)
    } else if parts[0] == "@" {
        file_path.and_then(|p| find_project_root(p))?
    } else {
        return None;
    };
    
    // Build the path
    let mut path = base_path;
    for part in &parts[1..] {
        path = path.join(part);
    }
    
    // Try with .ws extension
    let with_ext = path.with_extension("ws");
    if with_ext.exists() {
        return Some(with_ext);
    }
    
    // Try as mod.ws
    let as_mod = path.join("mod.ws");
    if as_mod.exists() {
        return Some(as_mod);
    }
    
    None
}

/// Get the std library path
fn get_std_path(file_path: Option<&PathBuf>) -> PathBuf {
    // Try WISP_STD_PATH env var
    if let Ok(std_path) = env::var("WISP_STD_PATH") {
        return PathBuf::from(std_path);
    }
    
    // Fall back to project_root/std
    if let Some(path) = file_path {
        if let Some(project_root) = find_project_root(path) {
            return project_root.join("std");
        }
    }
    
    // Last resort: assume std is in current directory
    PathBuf::from("std")
}

/// Find project root by looking for wisp.toml
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.parent()?.to_path_buf();
    loop {
        if current.join("wisp.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Scan a directory for .ws modules
fn scan_module_directory(base_path: &Path, subpath: &str) -> Vec<(String, CompletionItemKind)> {
    let mut suggestions = Vec::new();
    
    let dir_to_scan = if subpath.is_empty() {
        base_path.to_path_buf()
    } else {
        // If subpath has a partial name at the end, scan the parent directory
        let parts: Vec<&str> = subpath.split('.').collect();
        let mut dir = base_path.to_path_buf();
        
        // Add complete path segments (all but the last if it's partial)
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !part.is_empty() {
                dir = dir.join(part);
            }
        }
        dir
    };
    
    // Get the partial name being typed (last segment)
    let partial = subpath.split('.').last().unwrap_or("");
    
    if let Ok(entries) = fs::read_dir(&dir_to_scan) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().to_string();
                
                if file_type.is_file() && name.ends_with(".ws") && name != "mod.ws" {
                    // Module file (e.g., io.ws -> suggest "io")
                    let module_name = name.trim_end_matches(".ws");
                    if partial.is_empty() || module_name.starts_with(partial) {
                        suggestions.push((module_name.to_string(), CompletionItemKind::MODULE));
                    }
                } else if file_type.is_dir() && !name.starts_with('.') {
                    // Directory - might contain modules
                    if partial.is_empty() || name.starts_with(partial) {
                        suggestions.push((name, CompletionItemKind::FOLDER));
                    }
                }
            }
        }
    }
    
    suggestions.sort_by(|a, b| a.0.cmp(&b.0));
    suggestions
}

/// Convert NamespaceData from HIR to LSP NamespaceInfo
fn convert_namespace_data(data: &wisp_hir::NamespaceData, resolved: &wisp_hir::ResolvedProgram) -> NamespaceInfo {
    let mut items = HashMap::new();
    for (name, def_id) in &data.items {
        if let Some(def_info) = resolved.defs.get(def_id) {
            let kind = match def_info.kind {
                wisp_hir::DefKind::Function => "function",
                wisp_hir::DefKind::Struct => "struct",
                wisp_hir::DefKind::Enum => "enum",
                wisp_hir::DefKind::Trait => "trait",
                wisp_hir::DefKind::ExternFunction => "extern fn",
                wisp_hir::DefKind::ExternStatic => "extern static",
                _ => "item",
            };
            items.insert(name.clone(), (kind.to_string(), def_info.name.clone()));
        }
    }
    
    let children = data.children.iter()
        .map(|(k, v)| (k.clone(), convert_namespace_data(v, resolved)))
        .collect();
    
    NamespaceInfo { items, children }
}

/// Get the word at a given offset in the source
fn get_word_at_offset(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    let mut start = offset.min(bytes.len());
    let mut end = offset.min(bytes.len());
    
    // Find start of word
    while start > 0 && is_ident_char(bytes[start - 1] as char) {
        start -= 1;
    }
    
    // Find end of word
    while end < bytes.len() && is_ident_char(bytes[end] as char) {
        end += 1;
    }
    
    source[start..end].to_string()
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Convert a span to an LSP Diagnostic
fn span_to_diagnostic(source: &str, span: Span, message: &str, severity: DiagnosticSeverity) -> Diagnostic {
    let range = offset_to_range(source, span.start, span.end);
    Diagnostic {
        range,
        severity: Some(severity),
        code: None,
        code_description: None,
        source: Some("wisp".to_string()),
        message: message.to_string(),
        related_information: None,
        tags: None,
        data: None,
    }
}

/// Convert byte offset to LSP Position
fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    
    Position { line, character: col }
}

/// Convert byte offsets to LSP Range
fn offset_to_range(source: &str, start: usize, end: usize) -> Range {
    Range {
        start: offset_to_position(source, start),
        end: offset_to_position(source, end),
    }
}

/// Get the type name before a dot (for method/associated function calls)
/// e.g., for "Point.new()" when cursor is on "new", returns Some("Point")
fn get_type_before_dot(source: &str, offset: usize) -> Option<String> {
    // Find the start of the current word
    let mut word_start = offset;
    while word_start > 0 {
        let prev = source.as_bytes().get(word_start - 1)?;
        if prev.is_ascii_alphanumeric() || *prev == b'_' {
            word_start -= 1;
        } else {
            break;
        }
    }
    
    // Check if there's a dot before the word
    if word_start == 0 {
        return None;
    }
    
    let before_word = &source[..word_start];
    let trimmed = before_word.trim_end();
    if !trimmed.ends_with('.') {
        return None;
    }
    
    // Find the type name before the dot
    let before_dot = &trimmed[..trimmed.len() - 1].trim_end();
    let mut type_end = before_dot.len();
    let mut type_start = type_end;
    
    let bytes = before_dot.as_bytes();
    while type_start > 0 {
        let ch = bytes[type_start - 1];
        if ch.is_ascii_alphanumeric() || ch == b'_' {
            type_start -= 1;
        } else {
            break;
        }
    }
    
    if type_start < type_end {
        Some(before_dot[type_start..type_end].to_string())
    } else {
        None
    }
}

/// Convert LSP Position to byte offset
fn position_to_offset(source: &str, position: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    
    for (i, ch) in source.char_indices() {
        if line == position.line && col == position.character {
            return i;
        }
        if ch == '\n' {
            if line == position.line {
                return i;
            }
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    
    source.len()
}


/// Collect named argument info from AST
fn collect_named_args_from_ast(
    ast: &SourceFile,
    functions: &HashMap<String, FunctionInfo>,
    type_info: &mut HashMap<(usize, usize), String>,
    source_len: usize,
) {
    for item in &ast.items {
        match item {
            Item::Function(func) => {
                if let Some(ref body) = func.body {
                    collect_named_args_from_block(body, functions, type_info, source_len);
                }
            }
            Item::Impl(imp) => {
                for method in &imp.methods {
                    if let Some(ref body) = method.body {
                        collect_named_args_from_block(body, functions, type_info, source_len);
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_named_args_from_block(
    block: &wisp_ast::Block,
    functions: &HashMap<String, FunctionInfo>,
    type_info: &mut HashMap<(usize, usize), String>,
    source_len: usize,
) {
    for stmt in &block.stmts {
        match stmt {
            wisp_ast::Stmt::Let(let_stmt) => {
                if let Some(ref init) = let_stmt.init {
                    collect_named_args_from_expr(init, functions, type_info, source_len);
                }
            }
            wisp_ast::Stmt::Expr(expr_stmt) => {
                collect_named_args_from_expr(&expr_stmt.expr, functions, type_info, source_len);
            }
        }
    }
}

fn collect_named_args_from_expr(
    expr: &wisp_ast::Expr,
    functions: &HashMap<String, FunctionInfo>,
    type_info: &mut HashMap<(usize, usize), String>,
    source_len: usize,
) {
    if expr.span.end > source_len {
        return;
    }
    
    match &expr.kind {
        wisp_ast::ExprKind::Call(callee, args) => {
            // Get function name to look up parameter types
            let func_name: Option<String> = match &callee.kind {
                wisp_ast::ExprKind::Ident(name) => Some(name.name.clone()),
                wisp_ast::ExprKind::Field(_, field) => Some(field.name.clone()), // Type.method
                _ => None,
            };
            
            if let Some(ref func_name) = func_name {
                if let Some(func_info) = functions.get(func_name) {
                    // Match named args to parameters
                    for arg in args {
                        if let Some(ref name_ident) = arg.name {
                            // Find the parameter type
                            for (param_name, param_type) in &func_info.params {
                                if param_name == &name_ident.name {
                                    // Insert type info for the named argument label
                                    type_info.insert(
                                        (name_ident.span.start, name_ident.span.end),
                                        format!("{}: {}", param_name, param_type)
                                    );
                                    break;
                                }
                            }
                        }
                        // Recurse into the argument value
                        collect_named_args_from_expr(&arg.value, functions, type_info, source_len);
                    }
                } else {
                    // Function not found, still recurse into args
                    for arg in args {
                        collect_named_args_from_expr(&arg.value, functions, type_info, source_len);
                    }
                }
            } else {
                for arg in args {
                    collect_named_args_from_expr(&arg.value, functions, type_info, source_len);
                }
            }
            collect_named_args_from_expr(callee, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Binary(left, _, right) => {
            collect_named_args_from_expr(left, functions, type_info, source_len);
            collect_named_args_from_expr(right, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Unary(_, inner) => {
            collect_named_args_from_expr(inner, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::If(cond, then_block, else_block) => {
            collect_named_args_from_expr(cond, functions, type_info, source_len);
            collect_named_args_from_block(then_block, functions, type_info, source_len);
            if let Some(else_branch) = else_block {
                match else_branch {
                    wisp_ast::ElseBranch::Block(block) => collect_named_args_from_block(block, functions, type_info, source_len),
                    wisp_ast::ElseBranch::If(if_expr) => collect_named_args_from_expr(if_expr, functions, type_info, source_len),
                }
            }
        }
        wisp_ast::ExprKind::While(cond, body) => {
            collect_named_args_from_expr(cond, functions, type_info, source_len);
            collect_named_args_from_block(body, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Block(block) => {
            collect_named_args_from_block(block, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Field(inner, _) => {
            collect_named_args_from_expr(inner, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Ident(_) => {
            // Get function name for field access like Type.method
        }
        wisp_ast::ExprKind::Index(inner, index) => {
            collect_named_args_from_expr(inner, functions, type_info, source_len);
            collect_named_args_from_expr(index, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Ref(_, inner) => {
            collect_named_args_from_expr(inner, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Deref(inner) => {
            collect_named_args_from_expr(inner, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::Assign(target, value) => {
            collect_named_args_from_expr(target, functions, type_info, source_len);
            collect_named_args_from_expr(value, functions, type_info, source_len);
        }
        wisp_ast::ExprKind::StructLit(_, fields) => {
            for field in fields {
                collect_named_args_from_expr(&field.value, functions, type_info, source_len);
            }
        }
        _ => {}
    }
}

/// Collect variable definitions for go-to-definition (simplified version)
fn collect_variable_defs(
    program: &wisp_types::TypedProgram,
    variable_defs: &mut HashMap<String, (usize, usize)>,
    source_len: usize,
) {
    // Collect function parameters
    for func in &program.functions {
        if func.span.end > source_len { continue; }
        for param in &func.params {
            variable_defs.insert(param.name.clone(), (param.span.start, param.span.end));
        }
        if let Some(ref body) = func.body {
            collect_block_variable_defs(body, variable_defs, source_len);
        }
    }
    
    // Collect impl method parameters
    for imp in &program.impls {
        for method in &imp.methods {
            if method.span.end > source_len { continue; }
            for param in &method.params {
                variable_defs.insert(param.name.clone(), (param.span.start, param.span.end));
            }
            if let Some(ref body) = method.body {
                collect_block_variable_defs(body, variable_defs, source_len);
            }
        }
    }
}

fn collect_block_variable_defs(
    block: &wisp_types::TypedBlock,
    variable_defs: &mut HashMap<String, (usize, usize)>,
    source_len: usize,
) {
    for stmt in &block.stmts {
        match stmt {
            wisp_types::TypedStmt::Let { name, span, init, .. } => {
                if span.end <= source_len {
                    variable_defs.insert(name.clone(), (span.start, span.end));
                }
                if let Some(init) = init {
                    collect_expr_variable_defs(init, variable_defs, source_len);
                }
            }
            wisp_types::TypedStmt::Expr(expr) => {
                collect_expr_variable_defs(expr, variable_defs, source_len);
            }
        }
    }
}

fn collect_expr_variable_defs(
    expr: &wisp_types::TypedExpr,
    variable_defs: &mut HashMap<String, (usize, usize)>,
    source_len: usize,
) {
    if expr.span.end > source_len { return; }
    
    match &expr.kind {
        wisp_types::TypedExprKind::Block(block) => {
            collect_block_variable_defs(block, variable_defs, source_len);
        }
        wisp_types::TypedExprKind::If { then_block, else_block, .. } => {
            collect_block_variable_defs(then_block, variable_defs, source_len);
            if let Some(else_branch) = else_block {
                match else_branch {
                    wisp_types::TypedElse::Block(block) => collect_block_variable_defs(block, variable_defs, source_len),
                    wisp_types::TypedElse::If(if_expr) => collect_expr_variable_defs(if_expr, variable_defs, source_len),
                }
            }
        }
        wisp_types::TypedExprKind::While { body, .. } => {
            collect_block_variable_defs(body, variable_defs, source_len);
        }
        _ => {}
    }
}

/// Extract namespace information from parsed imports
/// This works even when resolution fails, giving us basic namespace structure for completion
fn extract_namespaces_from_imports(source: &wisp_ast::SourceFileWithImports) -> HashMap<String, NamespaceInfo> {
    let mut namespaces = HashMap::new();
    
    // Process all imported modules (transitive and direct)
    for module in &source.imported_modules {
        // Get the namespace name for this import
        let ns_name = if let Some(ref alias) = module.import.alias {
            alias.name.clone()
        } else {
            module.import.path.last_segment()
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        
        if ns_name.is_empty() || module.import.destructure_only {
            continue;
        }
        
        // Get or create namespace info
        let ns_info = namespaces.entry(ns_name.clone()).or_insert_with(NamespaceInfo::default);
        
        // If this is a direct import (not transitive), add its PUBLIC items directly
        if !module.is_transitive {
            for item in &module.items {
                if is_item_public(item) {
                    if let Some((name, kind)) = get_item_name_and_kind(item) {
                        ns_info.items.insert(name, (kind, String::new()));
                    }
                }
            }
            
            // Add children from module_imports (e.g., `pub import std/io as io` in std/mod.ws)
            for sub_import in &module.module_imports {
                let child_name = if let Some(ref alias) = sub_import.alias {
                    alias.name.clone()
                } else {
                    sub_import.path.last_segment()
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                };
                
                if !child_name.is_empty() {
                    // Create a child namespace and populate it from transitive imports
                    let mut child_info = NamespaceInfo::default();
                    
                    // Find the transitive import that matches this child
                    for transitive in &source.imported_modules {
                        if transitive.is_transitive {
                            let transitive_name = transitive.import.path.last_segment()
                                .map(|s| s.to_string())
                                .unwrap_or_default();
                            if transitive_name == child_name {
                                // Add PUBLIC items from this module
                                for item in &transitive.items {
                                    if is_item_public(item) {
                                        if let Some((name, kind)) = get_item_name_and_kind(item) {
                                            child_info.items.insert(name, (kind, String::new()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    ns_info.children.insert(child_name, child_info);
                }
            }
        }
    }
    
    namespaces
}

/// Check if an item is public
fn is_item_public(item: &wisp_ast::Item) -> bool {
    match item {
        wisp_ast::Item::Function(f) => f.is_pub,
        wisp_ast::Item::ExternFunction(f) => f.is_pub,
        wisp_ast::Item::ExternStatic(s) => s.is_pub,
        wisp_ast::Item::Struct(s) => s.is_pub,
        wisp_ast::Item::Enum(e) => e.is_pub,
        wisp_ast::Item::Trait(t) => t.is_pub,
        wisp_ast::Item::Impl(_) => true, // Impl blocks are always visible if the type is visible
        wisp_ast::Item::Import(_) => false, // Imports are not items in the namespace
    }
}

/// Get the name and kind of an item for namespace info
fn get_item_name_and_kind(item: &wisp_ast::Item) -> Option<(String, String)> {
    match item {
        wisp_ast::Item::Function(f) => Some((f.name.name.clone(), "function".to_string())),
        wisp_ast::Item::ExternFunction(f) => Some((f.name.name.clone(), "extern fn".to_string())),
        wisp_ast::Item::Struct(s) => Some((s.name.name.clone(), "struct".to_string())),
        wisp_ast::Item::Enum(e) => Some((e.name.name.clone(), "enum".to_string())),
        wisp_ast::Item::Trait(t) => Some((t.name.name.clone(), "trait".to_string())),
        _ => None,
    }
}

/// Synchronous version for populating std symbols
fn populate_std_symbols_sync(std_symbols: &mut HashMap<String, String>, base_dir: &Path) {
    // Find the std directory
    // First try relative to project root
    let mut std_dir = base_dir.join("std");
    
    if !std_dir.exists() {
        // Try going up from base_dir to find std
        let mut current = base_dir.to_path_buf();
        for _ in 0..5 {  // Limit search depth
            if !current.pop() {
                break;
            }
            let candidate = current.join("std");
            if candidate.exists() && candidate.is_dir() {
                std_dir = candidate;
                break;
            }
        }
        
        if !std_dir.exists() {
            // std directory not found
            return;
        }
    }
    
    populate_std_symbols_from_dir_sync(std_symbols, &std_dir, "std");
}

/// Synchronous recursive scan
fn populate_std_symbols_from_dir_sync(std_symbols: &mut HashMap<String, String>, dir: &Path, module_prefix: &str) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_file() && path.extension().map_or(false, |e| e == "ws") {
                // Parse the file to extract public items
                if let Ok(source) = fs::read_to_string(&path) {
                    if let Ok(parse_result) = wisp_parser::Parser::parse_with_recovery(&source) {
                        let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        
                        // Skip mod.ws as it's just for re-exports
                        if file_stem == "mod" {
                            continue;
                        }
                        
                        let module_path = format!("{}.{}", module_prefix, file_stem);
                        
                        for item in &parse_result.ast.items {
                            match item {
                                Item::Trait(t) if t.is_pub => {
                                    std_symbols.insert(t.name.name.clone(), module_path.clone());
                                }
                                Item::Struct(s) if s.is_pub => {
                                    std_symbols.insert(s.name.name.clone(), module_path.clone());
                                }
                                Item::Function(f) if f.is_pub => {
                                    std_symbols.insert(f.name.name.clone(), module_path.clone());
                                }
                                Item::ExternFunction(f) if f.is_pub => {
                                    std_symbols.insert(f.name.name.clone(), module_path.clone());
                                }
                                _ => {}
                            }
                        }
                    }
                }
            } else if path.is_dir() {
                // Recursively scan subdirectories
                let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                let new_prefix = format!("{}.{}", module_prefix, dir_name);
                populate_std_symbols_from_dir_sync(std_symbols, &path, &new_prefix);
            }
        }
    }
}

/// Find the insertion point for a new import statement
/// Returns the byte offset where the import should be inserted
fn find_import_insertion_point(source: &str) -> usize {
    let mut last_import_end = 0;
    let mut in_import = false;
    let mut chars = source.char_indices().peekable();
    
    while let Some((i, ch)) = chars.next() {
        // Skip whitespace and comments
        if ch.is_whitespace() {
            continue;
        }
        
        // Check for "import" keyword
        if ch == 'i' {
            let rest = &source[i..];
            if rest.starts_with("import ") || rest.starts_with("import\t") || rest.starts_with("import\n") {
                in_import = true;
                // Skip past the entire import statement
                while let Some((j, c)) = chars.next() {
                    if c == '\n' {
                        last_import_end = j + 1;
                        in_import = false;
                        break;
                    }
                }
            } else {
                // Not an import, we've reached the end of imports
                break;
            }
        } else if !in_import {
            // Found a non-whitespace, non-import character
            break;
        }
    }
    
    last_import_end
}

/// Check if two ranges overlap
fn ranges_overlap(a: &Range, b: &Range) -> bool {
    // Ranges overlap if one starts before the other ends
    !(a.end.line < b.start.line || (a.end.line == b.start.line && a.end.character < b.start.character) ||
      b.end.line < a.start.line || (b.end.line == a.start.line && b.end.character < a.start.character))
}

/// Convert a byte offset to a Position
/// Run the LSP server
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| WispLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
