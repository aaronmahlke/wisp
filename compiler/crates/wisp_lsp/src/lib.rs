//! Wisp Language Server Protocol implementation

use std::collections::HashMap;
use std::sync::RwLock;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use wisp_lexer::Span;
use wisp_parser::Parser;
use wisp_hir::Resolver;
use wisp_borrowck::BorrowChecker;

/// Document state stored by the LSP
#[derive(Debug, Default)]
struct DocumentState {
    /// Source text
    source: String,
    /// Type information: (start, end) -> type string
    type_info: HashMap<(usize, usize), String>,
    /// Definition locations: (start, end) -> (file, def_start, def_end)
    definitions: HashMap<(usize, usize), (String, usize, usize)>,
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
        let definitions = HashMap::new();

        // Run parser (lexer is integrated)
        let ast = match Parser::parse(text) {
            Ok(ast) => ast,
            Err(err) => {
                // Parser returns a single error message for now
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position { line: 0, character: 0 },
                        end: Position { line: 0, character: 1 },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("wisp".to_string()),
                    message: err.to_string(),
                    ..Default::default()
                });
                if let Ok(mut docs) = self.documents.write() {
                    docs.insert(uri.clone(), DocumentState {
                        source: text.to_string(),
                        type_info,
                        definitions,
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

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
                        definitions,
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
                        definitions,
                    });
                }
                self.client.publish_diagnostics(uri.clone(), diagnostics, None).await;
                return;
            }
        };

        // Collect type information from typed AST
        collect_type_info(&typed, &mut type_info);

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
                definitions,
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
        // We use full sync, so there's only one change with the full text
        if let Some(change) = params.content_changes.into_iter().next() {
            self.analyze_document(&uri, &change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Ok(mut docs) = self.documents.write() {
            docs.remove(&params.text_document.uri);
        }
        // Clear diagnostics
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Ok(docs) = self.documents.read() {
            if let Some(doc) = docs.get(uri) {
                // Convert position to byte offset
                let offset = position_to_offset(&doc.source, position);
                
                // Find type info at this position
                for ((start, end), type_str) in &doc.type_info {
                    if offset >= *start && offset <= *end {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("```wisp\n{}\n```", type_str),
                            }),
                            range: Some(offset_to_range(&doc.source, *start, *end)),
                        }));
                    }
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
                
                // Find definition at this position
                for ((start, end), (file, def_start, def_end)) in &doc.definitions {
                    if offset >= *start && offset <= *end {
                        let target_uri = if file.is_empty() {
                            uri.clone()
                        } else {
                            Url::from_file_path(file).unwrap_or_else(|_| uri.clone())
                        };
                        
                        // For same-file definitions, use the stored source
                        let target_source = if file.is_empty() {
                            &doc.source
                        } else {
                            // For other files, we'd need to read them
                            // For now, just use the current doc
                            &doc.source
                        };
                        
                        return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                            uri: target_uri,
                            range: offset_to_range(target_source, *def_start, *def_end),
                        })));
                    }
                }
            }
        }

        Ok(None)
    }
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
                // Position is past end of line
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

/// Collect type information from the typed program
fn collect_type_info(program: &wisp_types::TypedProgram, type_info: &mut HashMap<(usize, usize), String>) {
    // Collect types from function bodies
    for func in &program.functions {
        if let Some(ref body) = func.body {
            collect_block_types(body, &program.ctx, type_info);
        }
    }
    
    // Collect types from impl method bodies
    for imp in &program.impls {
        for method in &imp.methods {
            if let Some(ref body) = method.body {
                collect_block_types(body, &program.ctx, type_info);
            }
        }
    }
}

/// Collect types from a block
fn collect_block_types(
    block: &wisp_types::TypedBlock, 
    ctx: &wisp_types::TypeContext, 
    type_info: &mut HashMap<(usize, usize), String>
) {
    for stmt in &block.stmts {
        match stmt {
            wisp_types::TypedStmt::Let { name, ty, init, span, .. } => {
                type_info.insert((span.start, span.start + name.len() + 4), format!("{}: {}", name, ty.display(ctx)));
                if let Some(init) = init {
                    collect_expr_types(init, ctx, type_info);
                }
            }
            wisp_types::TypedStmt::Expr(expr) => {
                collect_expr_types(expr, ctx, type_info);
            }
        }
    }
}

/// Collect types from an expression
fn collect_expr_types(
    expr: &wisp_types::TypedExpr, 
    ctx: &wisp_types::TypeContext, 
    type_info: &mut HashMap<(usize, usize), String>
) {
    // Add type for this expression
    type_info.insert((expr.span.start, expr.span.end), expr.ty.display(ctx));
    
    // Recurse into sub-expressions
    match &expr.kind {
        wisp_types::TypedExprKind::Binary { left, right, .. } => {
            collect_expr_types(left, ctx, type_info);
            collect_expr_types(right, ctx, type_info);
        }
        wisp_types::TypedExprKind::Unary { expr: inner, .. } => {
            collect_expr_types(inner, ctx, type_info);
        }
        wisp_types::TypedExprKind::Call { callee, args, .. } => {
            collect_expr_types(callee, ctx, type_info);
            for arg in args {
                collect_expr_types(arg, ctx, type_info);
            }
        }
        wisp_types::TypedExprKind::MethodCall { receiver, args, .. } => {
            collect_expr_types(receiver, ctx, type_info);
            for arg in args {
                collect_expr_types(arg, ctx, type_info);
            }
        }
        wisp_types::TypedExprKind::Field { expr: inner, .. } => {
            collect_expr_types(inner, ctx, type_info);
        }
        wisp_types::TypedExprKind::If { cond, then_block, else_block, .. } => {
            collect_expr_types(cond, ctx, type_info);
            collect_block_types(then_block, ctx, type_info);
            if let Some(else_branch) = else_block {
                match else_branch {
                    wisp_types::TypedElse::Block(block) => collect_block_types(block, ctx, type_info),
                    wisp_types::TypedElse::If(if_expr) => collect_expr_types(if_expr, ctx, type_info),
                }
            }
        }
        wisp_types::TypedExprKind::While { cond, body, .. } => {
            collect_expr_types(cond, ctx, type_info);
            collect_block_types(body, ctx, type_info);
        }
        wisp_types::TypedExprKind::Block(block) => {
            collect_block_types(block, ctx, type_info);
        }
        wisp_types::TypedExprKind::Assign { target, value, .. } => {
            collect_expr_types(target, ctx, type_info);
            collect_expr_types(value, ctx, type_info);
        }
        wisp_types::TypedExprKind::Ref { expr: inner, .. } => {
            collect_expr_types(inner, ctx, type_info);
        }
        wisp_types::TypedExprKind::Deref(inner) => {
            collect_expr_types(inner, ctx, type_info);
        }
        wisp_types::TypedExprKind::StructLit { fields, .. } => {
            for (_, field_expr) in fields {
                collect_expr_types(field_expr, ctx, type_info);
            }
        }
        wisp_types::TypedExprKind::GenericCall { args, .. } => {
            for arg in args {
                collect_expr_types(arg, ctx, type_info);
            }
        }
        wisp_types::TypedExprKind::TraitMethodCall { receiver, args, .. } => {
            collect_expr_types(receiver, ctx, type_info);
            for arg in args {
                collect_expr_types(arg, ctx, type_info);
            }
        }
        _ => {}
    }
}

/// Run the LSP server
pub async fn run_server() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(WispLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

