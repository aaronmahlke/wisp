//! Wisp Language Server Protocol implementation

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use wisp_ast::{Item, SourceFile, StructField};
use wisp_lexer::Span;
use wisp_parser::parse_with_imports;
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
    /// Variable definitions: name -> (definition span start, definition span end)
    variable_defs: HashMap<String, (usize, usize)>,
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
        let mut variable_defs = HashMap::new();

        // Get file path from URI
        let file_path = uri.to_file_path().unwrap_or_else(|_| PathBuf::from("."));
        let file_str = file_path.to_string_lossy().to_string();

        // Run parser with import resolution
        let ast = match parse_with_imports(text, &file_path) {
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
                    docs.insert(uri.clone(), DocumentState {
                        source: text.to_string(),
                        type_info,
                        span_definitions,
                        def_spans,
                        functions,
                        structs,
                        variable_defs,
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

        // Collect function and struct info from AST
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

        // Run name resolution
        let resolved = match Resolver::resolve(&ast) {
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
                        variable_defs,
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

        // Run type checker
        let typed = match wisp_types::TypeChecker::check(&resolved) {
            Ok(typed) => typed,
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
                        variable_defs,
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
                variable_defs,
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
                    trigger_characters: Some(vec![".".to_string(), ":".to_string(), "{".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
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
                
                // Default: suggest all functions, structs, and keywords
                for (name, info) in &doc.functions {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(info.signature.clone()),
                        ..Default::default()
                    });
                }
                
                for (name, info) in &doc.structs {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::STRUCT),
                        detail: Some(info.definition.lines().next().unwrap_or("").to_string()),
                        // Insert struct literal template
                        insert_text: Some(format!("{} {{ }}", name)),
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
}

/// Find if we're inside a struct literal and return the struct name
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

/// Run the LSP server
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| WispLanguageServer::new(client));
    Server::new(stdin, stdout, socket).serve(service).await;
}
