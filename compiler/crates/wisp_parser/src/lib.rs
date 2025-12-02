use wisp_ast::*;
use wisp_lexer::{Lexer, Span, SpannedToken, Token};

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
        match self.peek() {
            Token::Import => self.parse_import().map(Item::Import),
            Token::Fn => self.parse_fn_def().map(Item::Function),
            Token::Extern => self.parse_extern_item(),
            Token::Struct => self.parse_struct_def().map(Item::Struct),
            Token::Enum => self.parse_enum_def().map(Item::Enum),
            Token::Trait => self.parse_trait_def().map(Item::Trait),
            Token::Impl => self.parse_impl_block().map(Item::Impl),
            _ => Err(ParseError {
                message: format!("expected item (import, fn, extern, struct, enum, trait, impl), found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        }
    }
    
    fn parse_extern_item(&mut self) -> ParseResult<Item> {
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
                
                Ok(Item::ExternFunction(ExternFnDef { name, params, return_type, span }))
            }
            Token::Static => {
                // extern static NAME: TYPE
                self.advance();
                let name = self.expect_ident()?;
                self.expect(Token::Colon)?;
                let ty = self.parse_type()?;
                
                let span = Span::new(start.start, ty.span.end);
                
                Ok(Item::ExternStatic(ExternStaticDef { name, ty, span }))
            }
            _ => Err(ParseError {
                message: format!("expected 'fn' or 'static' after 'extern', found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        }
    }
    
    fn parse_import(&mut self) -> ParseResult<ImportDecl> {
        let start = self.peek_span();
        self.expect(Token::Import)?;
        
        // Expect a string literal for the path
        let path = match self.peek().clone() {
            Token::StringLiteral(s) => {
                self.advance();
                s
            }
            _ => return Err(ParseError {
                message: format!("expected string literal after 'import', found '{}'", self.peek()),
                span: self.peek_span(),
            }),
        };
        
        let end = self.peek_span();
        let span = Span::new(start.start, end.end);
        
        Ok(ImportDecl { path, span })
    }
    
    fn parse_fn_def(&mut self) -> ParseResult<FnDef> {
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
        
        Ok(FnDef { name, type_params, params, return_type, body, span })
    }
    
    /// Parse generic parameters: <T, U: Clone + Debug>
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
            
            let span = Span::new(start.start, self.peek_span().end);
            params.push(GenericParam { name, bounds, span });
            
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

    fn parse_struct_def(&mut self) -> ParseResult<StructDef> {
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
        
        Ok(StructDef { name, type_params, fields, span })
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

    fn parse_enum_def(&mut self) -> ParseResult<EnumDef> {
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
        
        Ok(EnumDef { name, type_params, variants, span })
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

    fn parse_trait_def(&mut self) -> ParseResult<TraitDef> {
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
            methods.push(self.parse_fn_def()?);
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(TraitDef { name, type_params, methods, span })
    }

    fn parse_impl_block(&mut self) -> ParseResult<ImplBlock> {
        let start = self.peek_span();
        self.expect(Token::Impl)?;
        
        let first_type = self.parse_type()?;
        
        // Check for "for Type" (trait impl)
        let (trait_name, target_type) = if self.check(&Token::For) {
            self.advance();
            let target = self.parse_type()?;
            // first_type was the trait name
            let trait_ident = match first_type.kind {
                TypeKind::Named(id, _) => id,
                _ => return Err(ParseError {
                    message: "expected trait name".to_string(),
                    span: first_type.span,
                }),
            };
            (Some(trait_ident), target)
        } else {
            (None, first_type)
        };
        
        self.expect(Token::LBrace)?;
        
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.is_at_end() {
            methods.push(self.parse_fn_def()?);
        }
        
        let end = self.expect(Token::RBrace)?;
        let span = Span::new(start.start, end.span.end);
        
        Ok(ImplBlock { trait_name, target_type, methods, span })
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
            } else {
                break;
            }
        }
        
        Ok(expr)
    }

    fn parse_arg_list(&mut self) -> ParseResult<Vec<Expr>> {
        let mut args = Vec::new();
        
        while !self.check(&Token::RParen) && !self.is_at_end() {
            args.push(self.parse_expr()?);
            
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
                Ok(Expr {
                    kind: ExprKind::StringLiteral(s),
                    span: start,
                })
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
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RParen)?;
                Ok(expr)
            }
            _ => Err(ParseError {
                message: format!("expected expression, found '{}'", self.peek()),
                span: start,
            }),
        }
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
