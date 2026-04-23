use crate::ast::*;
use crate::error::{Result, RockError};
use crate::token::{InterpPiece, Span, Spanned, Token};

pub struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
    no_struct_lit: bool,
    pending: Vec<Item>,
}

impl Parser {
    pub fn new(toks: Vec<Spanned>) -> Self {
        Self { toks, pos: 0, no_struct_lit: false, pending: Vec::new() }
    }

    pub fn parse_program(&mut self) -> Result<Program> {
        let mut items = Vec::new();
        while !self.at_end() {
            if let Some(p) = self.pending.pop() {
                items.push(p);
                continue;
            }
            let item = self.parse_item()?;
            items.push(item);
            while let Some(p) = self.pending.pop() { items.push(p); }
        }
        while let Some(p) = self.pending.pop() { items.push(p); }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item> {
        if self.check(&Token::At) {
            let save = self.pos;
            self.advance();
            let ident = if let Token::Ident(n) = &self.peek().token { Some(n.clone()) } else { None };
            if let Some(n) = ident {
                if n == "prove" {
                    self.advance();
                    if self.check(&Token::LBrace) {
                        return self.parse_prove_block();
                    }
                }
                if n == "extern" {
                    self.advance();
                    if self.matches(&Token::LParen) {
                        while !self.check(&Token::RParen) && !self.at_end() { self.advance(); }
                        self.expect(&Token::RParen, "expected ')' after @extern(...)")?;
                    }
                    if self.check(&Token::LBrace) {
                        return self.parse_extern_block();
                    }
                }
            }
            self.pos = save;
        }
        let attrs = self.parse_attributes()?;
        // Optional `pub` visibility modifier (M5 roadmap item #11).
        // Default: all top-level items are public (backward compat).
        // When a module marks *any* item `pub`, only `pub` items are exposed
        // to importers of that module. See interpreter Item::Import handler.
        let is_pub = self.matches(&Token::Pub);
        if self.check(&Token::Fn) {
            let mut f = self.parse_function()?;
            f.attrs = attrs;
            f.is_pub = is_pub;
            Ok(Item::Function(f))
        } else if self.check(&Token::Type) {
            let mut td = self.parse_type_decl()?;
            td.is_pub = is_pub;
            Ok(Item::TypeDecl(td))
        } else if self.check(&Token::Enum) {
            let mut ed = self.parse_enum_decl()?;
            ed.is_pub = is_pub;
            Ok(Item::EnumDecl(ed))
        } else if self.check(&Token::Impl) {
            if is_pub { return Err(self.err("'pub' cannot be applied to 'impl' blocks")); }
            let item = self.parse_impl_or_trait_impl()?;
            Ok(item)
        } else if self.check(&Token::Trait) {
            if is_pub { return Err(self.err("'pub' cannot be applied to 'trait' blocks")); }
            Ok(Item::Trait(self.parse_trait_decl()?))
        } else if self.check(&Token::Const) {
            let span = self.peek().span;
            self.advance();
            let name = self.expect_ident("expected const name")?;
            if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
            self.expect(&Token::Assign, "expected '=' after const name")?;
            let value = self.parse_expr()?;
            Ok(Item::Const { name, value, is_pub, span })
        } else if self.check(&Token::StateMachine) {
            if is_pub { return Err(self.err("'pub' cannot be applied to 'state_machine'")); }
            Ok(Item::StateMachine(self.parse_state_machine()?))
        } else if self.check(&Token::Import) {
            let span = self.peek().span;
            self.advance();
            let path = match &self.peek().token {
                Token::Str(s) => s.clone(),
                _ => return Err(self.err("expected string path after 'import'")),
            };
            self.advance();
            // optional `as <ident>`
            let alias = if let Token::Ident(name) = &self.peek().token {
                if name == "as" {
                    self.advance();
                    let alias_name = match &self.peek().token {
                        Token::Ident(n) => n.clone(),
                        _ => return Err(self.err("expected identifier after 'as'")),
                    };
                    self.advance();
                    Some(alias_name)
                } else { None }
            } else { None };
            Ok(Item::Import { path, alias, is_pub, span })
        } else {
            if !attrs.is_empty() {
                return Err(self.err("attributes only allowed on fn/type/impl/const"));
            }
            if is_pub {
                return Err(self.err("'pub' must be followed by fn/type/enum/const/import"));
            }
            Ok(Item::Stmt(self.parse_stmt()?))
        }
    }

    fn parse_attributes(&mut self) -> Result<Vec<Attribute>> {
        let mut attrs = Vec::new();
        while self.check(&Token::At) {
            self.advance();
            let name = self.expect_ident("expected attribute name")?;
            let mut args = Vec::new();
            if self.matches(&Token::LParen) {
                if !self.check(&Token::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
                self.expect(&Token::RParen, "expected ')'")?;
            }
            attrs.push(Attribute { name, args });
        }
        Ok(attrs)
    }

    fn parse_type_decl(&mut self) -> Result<TypeDecl> {
        let span = self.peek().span;
        self.expect(&Token::Type, "expected 'type'")?;
        let name = self.expect_ident("expected type name")?;
        let mut type_params = Vec::new();
        if self.matches(&Token::Lt) {
            loop {
                type_params.push(self.expect_ident("expected type parameter")?);
                if !self.matches(&Token::Comma) { break; }
            }
            self.expect(&Token::Gt, "expected '>'")?;
        }
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let fname = self.expect_ident("expected field name")?;
            let ty = if self.matches(&Token::Colon) { Some(self.parse_type()?) } else { None };
            fields.push(Field { name: fname, ty });
            if !self.matches(&Token::Comma) { break; }
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(TypeDecl { name, type_params, fields, span, is_pub: false })
    }

    fn parse_enum_decl(&mut self) -> Result<EnumDecl> {
        let span = self.peek().span;
        self.expect(&Token::Enum, "expected 'enum'")?;
        let name = self.expect_ident("expected enum name")?;
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut variants = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let vname = self.expect_ident("expected variant name")?;
            let kind = if self.matches(&Token::LParen) {
                let mut count = 0;
                if !self.check(&Token::RParen) {
                    loop {
                        let _ = self.parse_type()?;
                        count += 1;
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
                self.expect(&Token::RParen, "expected ')'")?;
                VariantKind::Tuple(count)
            } else if self.matches(&Token::LBrace) {
                let mut field_names = Vec::new();
                while !self.check(&Token::RBrace) && !self.at_end() {
                    let fname = self.expect_ident("expected field name")?;
                    if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
                    field_names.push(fname);
                    if !self.matches(&Token::Comma) { break; }
                }
                self.expect(&Token::RBrace, "expected '}'")?;
                VariantKind::Named(field_names)
            } else {
                VariantKind::Nullary
            };
            variants.push(Variant { name: vname, kind });
            if !self.matches(&Token::Comma) { break; }
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(EnumDecl { name, variants, span, is_pub: false })
    }

    fn parse_state_machine(&mut self) -> Result<StateMachineDecl> {
        let span = self.peek().span;
        self.expect(&Token::StateMachine, "expected 'state_machine'")?;
        let name = self.expect_ident("expected state_machine name")?;
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut states: Vec<String> = Vec::new();
        let mut transitions: Vec<(String, String)> = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let first = self.expect_ident("expected state name")?;
            if !states.contains(&first) { states.push(first.clone()); }
            let mut current = first;
            while self.matches(&Token::Arrow) {
                let next = self.expect_ident("expected target state")?;
                if !states.contains(&next) { states.push(next.clone()); }
                transitions.push((current.clone(), next.clone()));
                current = next;
            }
            self.matches(&Token::Comma);
            while self.matches(&Token::Semicolon) {}
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(StateMachineDecl { name, states, transitions, span })
    }

    fn parse_extern_block(&mut self) -> Result<Item> {
        let span = self.peek().span;
        self.expect(&Token::LBrace, "expected '{' after @extern")?;
        let mut fns: Vec<Function> = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            self.expect(&Token::Fn, "expected 'fn' inside @extern block")?;
            let name = self.expect_ident("expected extern fn name")?;
            self.expect(&Token::LParen, "expected '('")?;
            let mut params = Vec::new();
            if !self.check(&Token::RParen) {
                loop {
                    let pname = self.expect_ident("expected parameter name")?;
                    if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
                    params.push(Param { name: pname, ty: None, literal: None });
                    if !self.matches(&Token::Comma) { break; }
                }
            }
            self.expect(&Token::RParen, "expected ')'")?;
            if self.matches(&Token::Arrow) { let _ = self.parse_type()?; }
            else if !self.check(&Token::Semicolon) && !self.check(&Token::Fn) && !self.check(&Token::RBrace) {
                let _ = self.parse_type()?;
            }
            while self.matches(&Token::Semicolon) {}
            let body = Block { stmts: Vec::new(), span };
            let attrs = vec![Attribute { name: "extern_stub".to_string(), args: Vec::new() }];
            fns.push(Function { name, params, body, span, attrs, has_self: false, is_pub: false });
        }
        self.expect(&Token::RBrace, "expected '}' closing @extern")?;
        if fns.is_empty() {
            return Ok(Item::Stmt(Stmt::Expr(Expr::Nil(span))));
        }
        let first = fns.remove(0);
        for f in fns.into_iter().rev() {
            self.pending.push(Item::Function(f));
        }
        Ok(Item::Function(first))
    }

    fn parse_prove_block(&mut self) -> Result<Item> {
        let span = self.peek().span;
        self.expect(&Token::LBrace, "expected '{' after @prove")?;        let mut assertions = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let a_span = self.peek().span;
            let name = self.expect_ident("expected 'assert_unreachable' or 'assert_never'")?;
            self.expect(&Token::LParen, "expected '(' after assertion name")?;
            match name.as_str() {
                "assert_unreachable" => {
                    let from = self.parse_expr()?;
                    self.expect(&Token::Arrow, "expected '->' in assert_unreachable")?;
                    let to = self.parse_expr()?;
                    self.expect(&Token::RParen, "expected ')'")?;
                    assertions.push(ProveAssertion::Unreachable { from, to, span: a_span });
                }
                "assert_never" => {
                    let expr = self.parse_expr()?;
                    self.expect(&Token::RParen, "expected ')'")?;
                    assertions.push(ProveAssertion::Never { expr, span: a_span });
                }
                _ => return Err(self.err(&format!(
                    "unknown @prove assertion '{}'; expected 'assert_unreachable' or 'assert_never'", name
                ))),
            }
            while self.matches(&Token::Semicolon) {}
            self.matches(&Token::Comma);
        }
        self.expect(&Token::RBrace, "expected '}' closing @prove")?;
        Ok(Item::Prove(ProveBlock { assertions, span }))
    }

    fn parse_impl_or_trait_impl(&mut self) -> Result<Item> {
        let span = self.peek().span;
        self.expect(&Token::Impl, "expected 'impl'")?;
        let mut type_params = Vec::new();
        if self.matches(&Token::Lt) {
            loop {
                type_params.push(self.expect_ident("expected type parameter")?);
                if !self.matches(&Token::Comma) { break; }
            }
            self.expect(&Token::Gt, "expected '>'")?;
        }
        let first = self.expect_ident("expected type or trait name")?;
        if self.matches(&Token::Lt) {
            while !self.matches(&Token::Gt) { self.advance(); }
        }
        if self.matches(&Token::For) {
            let target = self.expect_ident("expected target type after 'for'")?;
            if self.matches(&Token::Lt) {
                while !self.matches(&Token::Gt) { self.advance(); }
            }
            self.expect(&Token::LBrace, "expected '{'")?;
            let mut methods = Vec::new();
            while !self.check(&Token::RBrace) && !self.at_end() {
                let attrs = self.parse_attributes()?;
                let mut f = self.parse_function()?;
                f.attrs = attrs;
                methods.push(f);
            }
            self.expect(&Token::RBrace, "expected '}'")?;
            return Ok(Item::TraitImpl(TraitImpl { trait_name: first, target, methods, span }));
        }
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let attrs = self.parse_attributes()?;
            let mut f = self.parse_function()?;
            f.attrs = attrs;
            methods.push(f);
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(Item::Impl(ImplBlock { target: first, type_params, methods, span }))
    }

    fn parse_trait_decl(&mut self) -> Result<TraitDecl> {
        let span = self.peek().span;
        self.expect(&Token::Trait, "expected 'trait'")?;
        let name = self.expect_ident("expected trait name")?;
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let m_span = self.peek().span;
            self.expect(&Token::Fn, "expected 'fn' in trait body")?;
            let mname = self.expect_ident("expected method name")?;
            self.expect(&Token::LParen, "expected '('")?;
            let mut has_self = false;
            let mut param_count = 0usize;
            if !self.check(&Token::RParen) {
                if self.check(&Token::SelfKw) { self.advance(); has_self = true; self.matches(&Token::Comma); }
                if !self.check(&Token::RParen) {
                    loop {
                        let _ = self.expect_ident("expected parameter name")?;
                        if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
                        param_count += 1;
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
            }
            self.expect(&Token::RParen, "expected ')'")?;
            if self.matches(&Token::Arrow) { let _ = self.parse_type()?; }
            else if !self.check(&Token::LBrace) && !self.check(&Token::Semicolon) && !self.check(&Token::Fn) && !self.check(&Token::RBrace) {
                let _ = self.parse_type()?;
            }
            let default = if self.check(&Token::LBrace) {
                let body = self.parse_block()?;
                Some(Function {
                    name: mname.clone(),
                    params: Vec::new(),
                    body,
                    span: m_span,
                    attrs: Vec::new(),
                    has_self,
                    is_pub: false,
                })
            } else {
                while self.matches(&Token::Semicolon) {}
                None
            };
            methods.push(TraitMethod { name: mname, has_self, param_count, default, span: m_span });
            while self.matches(&Token::Semicolon) {}
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(TraitDecl { name, methods, span })
    }

    fn parse_impl_block(&mut self) -> Result<ImplBlock> {
        let span = self.peek().span;
        self.expect(&Token::Impl, "expected 'impl'")?;
        let mut type_params = Vec::new();
        if self.matches(&Token::Lt) {
            loop {
                type_params.push(self.expect_ident("expected type parameter")?);
                if !self.matches(&Token::Comma) { break; }
            }
            self.expect(&Token::Gt, "expected '>'")?;
        }
        let target = self.expect_ident("expected impl target type")?;
        if self.matches(&Token::Lt) {
            while !self.matches(&Token::Gt) { self.advance(); }
        }
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut methods = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            let attrs = self.parse_attributes()?;
            let mut f = self.parse_function()?;
            f.attrs = attrs;
            methods.push(f);
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(ImplBlock { target, type_params, methods, span })
    }

    fn parse_function(&mut self) -> Result<Function> {
        let span = self.peek().span;
        self.expect(&Token::Fn, "expected 'fn'")?;
        let name = self.expect_ident("expected function name")?;
        if self.matches(&Token::Lt) {
            while !self.matches(&Token::Gt) { self.advance(); }
        }
        self.expect(&Token::LParen, "expected '('")?;
        let mut params = Vec::new();
        let mut has_self = false;
        if !self.check(&Token::RParen) {
            if self.check(&Token::SelfKw) {
                self.advance();
                has_self = true;
                self.matches(&Token::Comma);
            }
            if !self.check(&Token::RParen) {
                loop {
                    let param_span = self.peek().span;
                    let is_literal = matches!(
                        self.peek().token,
                        Token::Int(_) | Token::Float(_) | Token::Str(_)
                        | Token::True | Token::False | Token::Nil | Token::Minus
                    );
                    if is_literal {
                        let lit = self.parse_unary()?;
                        params.push(Param {
                            name: format!("__lit{}", params.len()),
                            ty: None,
                            literal: Some(lit),
                        });
                        let _ = param_span;
                    } else {
                        let pname = self.expect_ident("expected parameter name")?;
                        let ty = if self.matches(&Token::Colon) {
                            Some(self.parse_type()?)
                        } else {
                            None
                        };
                        params.push(Param { name: pname, ty, literal: None });
                    }
                    if !self.matches(&Token::Comma) { break; }
                }
            }
        }
        self.expect(&Token::RParen, "expected ')'")?;
        if self.matches(&Token::Arrow) {
            let _ret = self.parse_type()?;
        } else if !self.check(&Token::LBrace) {
            let _ret = self.parse_type()?;
        }
        let body = self.parse_block()?;
        Ok(Function { name, params, body, span, attrs: Vec::new(), has_self, is_pub: false })
    }

    fn parse_type(&mut self) -> Result<String> {
        if self.matches(&Token::Amp) {
            let inner = self.parse_type()?;
            return Ok(format!("&{}", inner));
        }
        if self.matches(&Token::LBracket) {
            let inner = self.parse_type()?;
            self.expect(&Token::RBracket, "expected ']'")?;
            return Ok(format!("[{}]", inner));
        }
        let name = self.expect_ident("expected type name")?;
        if self.matches(&Token::Lt) {
            let mut args = Vec::new();
            loop {
                args.push(self.parse_type()?);
                if !self.matches(&Token::Comma) { break; }
            }
            self.expect(&Token::Gt, "expected '>'")?;
            return Ok(format!("{}<{}>", name, args.join(",")));
        }
        Ok(name)
    }

    fn parse_block(&mut self) -> Result<Block> {
        let span = self.peek().span;
        self.expect(&Token::LBrace, "expected '{'")?;
        let mut stmts = Vec::new();
        while !self.check(&Token::RBrace) && !self.at_end() {
            stmts.push(self.parse_stmt()?);
            while self.matches(&Token::Semicolon) {}
        }
        self.expect(&Token::RBrace, "expected '}'")?;
        Ok(Block { stmts, span })
    }

    fn parse_stmt(&mut self) -> Result<Stmt> {
        let tok = self.peek();
        let span = tok.span;

        match &tok.token {
            Token::Let => {
                self.advance();
                let mutable = self.matches(&Token::Mut);
                let is_destructure = !mutable && match &self.peek().token {
                    Token::LBracket | Token::LBrace | Token::LParen => true,
                    Token::Ident(n) => {
                        n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
                            && matches!(self.peek_kind(1), Some(Token::LBrace) | Some(Token::LParen))
                    }
                    _ => false,
                };
                if is_destructure {
                    let pattern = self.parse_pattern()?;
                    self.expect(&Token::Assign, "expected '='")?;
                    let value = self.parse_expr()?;
                    return Ok(Stmt::LetPattern { pattern, value, span });
                }
                let name = self.expect_ident("expected name")?;
                if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
                self.expect(&Token::Assign, "expected '='")?;
                let value = self.parse_expr()?;
                return Ok(Stmt::Let { name, mutable, value, span });
            }
            Token::Mut => {
                self.advance();
                let name = self.expect_ident("expected name")?;
                if self.matches(&Token::Colon) { let _ = self.parse_type()?; }
                if self.matches(&Token::Walrus) || self.matches(&Token::Assign) {
                    let value = self.parse_expr()?;
                    return Ok(Stmt::Let { name, mutable: true, value, span });
                }
                return Err(self.err("expected ':=' or '=' after 'mut <name>'"));
            }
            Token::Return => {
                self.advance();
                let value = if self.stmt_terminator() { None } else { Some(self.parse_expr()?) };
                return Ok(Stmt::Return(value, span));
            }
            Token::Break => { self.advance(); return Ok(Stmt::Break(span)); }
            Token::Continue => { self.advance(); return Ok(Stmt::Continue(span)); }
            Token::While => {
                self.advance();
                let cond = self.parse_expr_no_struct()?;
                let body = self.parse_block()?;
                return Ok(Stmt::While { cond, body, span });
            }
            Token::Loop => {
                self.advance();
                let body = self.parse_block()?;
                return Ok(Stmt::Loop { body, span });
            }
            Token::For => {
                self.advance();
                let var = self.expect_ident("expected loop variable")?;
                self.expect(&Token::In, "expected 'in'")?;
                let iter = self.parse_expr_no_struct()?;
                let body = self.parse_block()?;
                return Ok(Stmt::For { var, iter, body, span });
            }
            Token::Defer => {
                self.advance();
                let body = self.parse_block()?;
                return Ok(Stmt::Defer { body, span });
            }
            Token::Try => {
                self.advance();
                let try_body = self.parse_block()?;
                self.expect(&Token::Catch, "expected 'catch' after try block")?;
                self.expect(&Token::LParen, "expected '(' after 'catch'")?;
                let err_name = self.expect_ident("expected error variable name")?;
                self.expect(&Token::RParen, "expected ')' after error variable")?;
                let catch_body = self.parse_block()?;
                return Ok(Stmt::TryCatch { try_body, err_name, catch_body, span });
            }
            Token::With => {
                self.advance();
                let ctx = self.parse_expr_no_struct()?;
                let body = self.parse_block()?;
                return Ok(Stmt::With { ctx, body, span });
            }
            Token::Ident(_) if self.peek_kind(1) == Some(&Token::Walrus) => {
                let name = self.expect_ident("expected name")?;
                self.advance();
                let value = self.parse_expr()?;
                return Ok(Stmt::Let { name, mutable: false, value, span });
            }
            _ => {}
        }

        let expr = self.parse_expr()?;
        if self.matches(&Token::ReactiveArrow) {
            let name = match &expr {
                Expr::Ident(n, _) => n.clone(),
                _ => return Err(self.err("left side of '~>' must be an identifier")),
            };
            let reactive_expr = self.parse_expr()?;
            return Ok(Stmt::Reactive { name, expr: reactive_expr, span });
        }
        let op = match &self.peek().token {
            Token::Assign => Some(AssignOp::Set),
            Token::PlusAssign => Some(AssignOp::Add),
            Token::MinusAssign => Some(AssignOp::Sub),
            Token::StarAssign => Some(AssignOp::Mul),
            Token::SlashAssign => Some(AssignOp::Div),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let value = self.parse_expr()?;
            return Ok(Stmt::Assign { target: expr, op, value, span });
        }
        Ok(Stmt::Expr(expr))
    }

    fn stmt_terminator(&self) -> bool {
        matches!(self.peek().token, Token::RBrace | Token::Semicolon | Token::Eof)
    }

    pub fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_pipe()
    }

    fn parse_expr_no_struct(&mut self) -> Result<Expr> {
        let prev = self.no_struct_lit;
        self.no_struct_lit = true;
        let e = self.parse_expr();
        self.no_struct_lit = prev;
        e
    }

    fn parse_pipe(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_default()?;
        while self.matches(&Token::PipeArrow) {
            let rhs = self.parse_default()?;
            let span = lhs.span();
            lhs = Expr::Pipe { lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_default(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_range()?;
        while self.matches(&Token::DoubleQuestion) {
            let rhs = self.parse_range()?;
            let span = lhs.span();
            lhs = Expr::DefaultOr { lhs: Box::new(lhs), default: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_range(&mut self) -> Result<Expr> {
        let lhs = self.parse_or()?;
        if self.matches(&Token::DotDot) {
            let rhs = self.parse_or()?;
            let span = lhs.span();
            return Ok(Expr::Range { start: Box::new(lhs), end: Box::new(rhs), span });
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while self.matches(&Token::Or) {
            let rhs = self.parse_and()?;
            let span = lhs.span();
            lhs = Expr::Binary { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_equality()?;
        while self.matches(&Token::And) {
            let rhs = self.parse_equality()?;
            let span = lhs.span();
            lhs = Expr::Binary { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_comparison()?;
        loop {
            let op = match &self.peek().token {
                Token::Eq => BinOp::Eq,
                Token::Neq => BinOp::Neq,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_comparison()?;
            let span = lhs.span();
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_additive()?;
        loop {
            let op = match &self.peek().token {
                Token::Lt => BinOp::Lt,
                Token::Gt => BinOp::Gt,
                Token::Le => BinOp::Le,
                Token::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_additive()?;
            let span = lhs.span();
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_mult()?;
        loop {
            let op = match &self.peek().token {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mult()?;
            let span = lhs.span();
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_mult(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match &self.peek().token {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Percent => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            let span = lhs.span();
            lhs = Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        let span = self.peek().span;
        if self.matches(&Token::Minus) {
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary { op: UnaryOp::Neg, rhs: Box::new(rhs), span });
        }
        if self.matches(&Token::Not) {
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary { op: UnaryOp::Not, rhs: Box::new(rhs), span });
        }
        if self.matches(&Token::Amp) {
            return self.parse_postfix();
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            let span = expr.span();
            match &self.peek().token {
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RParen, "expected ')'")?;
                    expr = Expr::Call { callee: Box::new(expr), args, span };
                }
                Token::LBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.expect(&Token::RBracket, "expected ']'")?;
                    expr = Expr::Index { base: Box::new(expr), idx: Box::new(idx), span };
                }
                Token::Dot => {
                    self.advance();
                    let name = self.expect_ident("expected field name")?;
                    if self.check(&Token::LParen) {
                        self.advance();
                        let mut args = Vec::new();
                        if !self.check(&Token::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if !self.matches(&Token::Comma) { break; }
                            }
                        }
                        self.expect(&Token::RParen, "expected ')'")?;
                        expr = Expr::MethodCall { receiver: Box::new(expr), method: name, args, span };
                    } else {
                        expr = Expr::Field { base: Box::new(expr), name, span };
                    }
                }
                Token::QuestionDot => {
                    self.advance();
                    let name = self.expect_ident("expected field name")?;
                    expr = Expr::OptField { base: Box::new(expr), name, span };
                }
                Token::Bang => {
                    self.advance();
                    expr = Expr::Panic(Box::new(expr), span);
                }
                Token::Question => {
                    self.advance();
                    expr = Expr::Try(Box::new(expr), span);
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        let tok = self.advance_owned();
        let span = tok.span;
        match tok.token {
            Token::Int(n) => Ok(Expr::Int(n, span)),
            Token::Float(f) => Ok(Expr::Float(f, span)),
            Token::Str(s) => Ok(Expr::Str(s, span)),
            Token::InterpStr(pieces) => {
                let mut parts = Vec::new();
                for p in pieces {
                    match p {
                        InterpPiece::Lit(s) => parts.push(InterpPart::Lit(s)),
                        InterpPiece::Expr(src) => {
                            let toks = crate::lexer::Lexer::new(&src).tokenize()?;
                            let mut sub = Parser::new(toks);
                            let e = sub.parse_expr()?;
                            parts.push(InterpPart::Expr(e));
                        }
                    }
                }
                Ok(Expr::Interp(parts, span))
            }
            Token::True => Ok(Expr::Bool(true, span)),
            Token::False => Ok(Expr::Bool(false, span)),
            Token::Nil => Ok(Expr::Nil(span)),
            Token::SelfKw => Ok(Expr::SelfExpr(span)),
            Token::Spawn => {
                let inner = self.parse_unary()?;
                Ok(Expr::Spawn(Box::new(inner), span))
            }
            Token::Await => {
                let inner = self.parse_unary()?;
                Ok(Expr::Await(Box::new(inner), span))
            }
            Token::Raw => {
                let block = self.parse_block()?;
                Ok(Expr::Raw(block))
            }
            Token::At => {
                let name = self.expect_ident("expected attribute name after '@'")?;
                if name == "run" || name == "comptime" {
                    let inner = self.parse_unary()?;
                    return Ok(Expr::Comptime(Box::new(inner), span));
                }
                if name == "grad" || name == "trace" || name == "reflect" {
                    self.expect(&Token::LParen, "expected '(' after attribute")?;
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.parse_expr()?);
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RParen, "expected ')'")?;
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::Ident(format!("__{}", name), span)),
                        args,
                        span,
                    });
                }
                Err(RockError::parse(
                    format!("unexpected attribute '@{}' in expression position", name),
                    span.line, span.col,
                ))
            }
            Token::Ident(name) => {
                if !self.no_struct_lit && self.looks_like_struct_lit() {
                    self.advance();
                    let mut fields = Vec::new();
                    if !self.check(&Token::RBrace) {
                        loop {
                            let fname = self.expect_ident("expected field name")?;
                            let value = if self.matches(&Token::Colon) {
                                self.parse_expr()?
                            } else {
                                Expr::Ident(fname.clone(), span)
                            };
                            fields.push((fname, value));
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RBrace, "expected '}'")?;
                    Ok(Expr::StructLit { name, fields, span })
                } else {
                    Ok(Expr::Ident(name, span))
                }
            }
            Token::Print => Ok(Expr::Ident("print".to_string(), span)),
            Token::Fn => {
                self.expect(&Token::LParen, "expected '(' in lambda")?;
                let mut params = Vec::new();
                if !self.check(&Token::RParen) {
                    loop {
                        let pname = self.expect_ident("expected parameter name")?;
                        let ty = if self.matches(&Token::Colon) {
                            Some(self.parse_type()?)
                        } else { None };
                        params.push(Param { name: pname, ty, literal: None });
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
                self.expect(&Token::RParen, "expected ')'")?;
                if !self.check(&Token::LBrace) { let _ = self.parse_type()?; }
                let body = self.parse_block()?;
                Ok(Expr::Lambda { params, body, span })
            }
            Token::Match => {
                let scrutinee = self.parse_expr_no_struct()?;
                self.expect(&Token::LBrace, "expected '{' after match expr")?;
                let mut arms = Vec::new();
                while !self.check(&Token::RBrace) && !self.at_end() {
                    let pattern = self.parse_pattern()?;
                    let guard = if self.matches(&Token::If) {
                        Some(self.parse_expr()?)
                    } else { None };
                    self.expect(&Token::FatArrow, "expected '=>' in match arm")?;
                    let body = self.parse_expr()?;
                    arms.push(MatchArm { pattern, guard, body });
                    self.matches(&Token::Comma);
                }
                self.expect(&Token::RBrace, "expected '}' closing match")?;
                Ok(Expr::Match { scrutinee: Box::new(scrutinee), arms, span })
            }
            Token::LParen => {
                let e = self.parse_expr()?;
                if self.matches(&Token::Comma) {
                    let mut items = vec![e];
                    if !self.check(&Token::RParen) {
                        loop {
                            items.push(self.parse_expr()?);
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RParen, "expected ')' closing tuple")?;
                    return Ok(Expr::Array(items, span));
                }
                self.expect(&Token::RParen, "expected ')'")?;
                Ok(e)
            }
            Token::LBracket => {
                if self.check(&Token::RBracket) {
                    self.advance();
                    return Ok(Expr::Array(Vec::new(), span));
                }
                let first = self.parse_expr()?;
                if self.matches(&Token::Semicolon) {
                    let count = self.parse_expr()?;
                    self.expect(&Token::RBracket, "expected ']'")?;
                    return Ok(Expr::Call {
                        callee: Box::new(Expr::Ident("__array_fill".to_string(), span)),
                        args: vec![first, count],
                        span,
                    });
                }
                let mut elems = vec![first];
                while self.matches(&Token::Comma) {
                    if self.check(&Token::RBracket) { break; }
                    elems.push(self.parse_expr()?);
                }
                self.expect(&Token::RBracket, "expected ']'")?;
                Ok(Expr::Array(elems, span))
            }
            Token::LBrace => {
                if self.looks_like_map() {
                    let mut pairs = Vec::new();
                    if !self.check(&Token::RBrace) {
                        loop {
                            let k = self.parse_expr()?;
                            self.expect(&Token::Colon, "expected ':' in map")?;
                            let v = self.parse_expr()?;
                            pairs.push((k, v));
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RBrace, "expected '}'")?;
                    return Ok(Expr::Map(pairs, span));
                }
                self.pos -= 1;
                let block = self.parse_block()?;
                Ok(Expr::Block(block))
            }
            Token::If => {
                let cond = self.parse_expr_no_struct()?;
                let then = self.parse_block()?;
                let else_branch = if self.matches(&Token::Else) {
                    if self.check(&Token::If) {
                        Some(Box::new(self.parse_primary()?))
                    } else {
                        let b = self.parse_block()?;
                        Some(Box::new(Expr::Block(b)))
                    }
                } else { None };
                Ok(Expr::If { cond: Box::new(cond), then, else_branch, span })
            }
            Token::Try => {
                // Try-expression form: `try { ... } catch (e) { ... }` used in
                // expression position. The block's final expression becomes the
                // value; on any error in the try-body, the catch-body runs with
                // the error message string bound to `err_name` and its final
                // expression becomes the value.
                let try_body = self.parse_block()?;
                self.expect(&Token::Catch, "expected 'catch' after try block")?;
                self.expect(&Token::LParen, "expected '(' after 'catch'")?;
                let err_name = self.expect_ident("expected error variable name")?;
                self.expect(&Token::RParen, "expected ')' after error variable")?;
                let catch_body = self.parse_block()?;
                Ok(Expr::TryCatch { try_body, err_name, catch_body, span })
            }
            other => Err(RockError::parse(
                format!("unexpected token {:?}", other),
                span.line,
                span.col,
            )),
        }
    }

    fn looks_like_map(&self) -> bool {
        if let Some(Token::RBrace) = self.peek_kind(0) {
            return true;
        }
        let second = self.peek_kind(1);
        matches!(self.peek_kind(0), Some(Token::Str(_)) | Some(Token::Ident(_)) | Some(Token::Int(_)))
            && matches!(second, Some(Token::Colon))
    }

    fn looks_like_struct_lit(&self) -> bool {
        if !matches!(self.peek_kind(0), Some(Token::LBrace)) {
            return false;
        }
        match self.peek_kind(1) {
            Some(Token::RBrace) => true,
            Some(Token::Ident(_)) => matches!(
                self.peek_kind(2),
                Some(Token::Colon) | Some(Token::Comma) | Some(Token::RBrace)
            ),
            _ => false,
        }
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        let first = self.parse_pattern_atom()?;
        if self.check(&Token::Pipe) {
            let mut alts = vec![first];
            while self.matches(&Token::Pipe) {
                alts.push(self.parse_pattern_atom()?);
            }
            Ok(Pattern::Or(alts))
        } else {
            Ok(first)
        }
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern> {
        let tok = self.peek();
        match &tok.token {
            Token::LBracket => {
                self.advance();
                let mut items = Vec::new();
                let mut rest: Option<Option<String>> = None;
                if !self.check(&Token::RBracket) {
                    loop {
                        if self.check(&Token::DotDot) {
                            self.advance();
                            let name = if let Token::Ident(n) = &self.peek().token {
                                let n = n.clone();
                                self.advance();
                                if n == "_" { None } else { Some(n) }
                            } else { None };
                            rest = Some(name);
                            break;
                        }
                        items.push(self.parse_pattern()?);
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
                self.expect(&Token::RBracket, "expected ']' closing array pattern")?;
                Ok(Pattern::Array { items, rest })
            }
            Token::LParen => {
                self.advance();
                let mut items = Vec::new();
                if !self.check(&Token::RParen) {
                    loop {
                        items.push(self.parse_pattern()?);
                        if !self.matches(&Token::Comma) { break; }
                    }
                }
                self.expect(&Token::RParen, "expected ')' closing tuple pattern")?;
                Ok(Pattern::Tuple(items))
            }
            Token::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            Token::Ident(name) => {
                let n = name.clone();
                let is_type = n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
                if is_type && self.peek_kind(1) == Some(&Token::LBrace) {
                    self.advance();
                    self.advance();
                    let (fields, rest) = self.parse_struct_pattern_body()?;
                    Ok(Pattern::Struct { type_name: Some(n), fields, rest })
                } else if is_type && self.peek_kind(1) == Some(&Token::LParen) {
                    self.advance();
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&Token::RParen) {
                        loop {
                            args.push(self.parse_pattern()?);
                            if !self.matches(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RParen, "expected ')' closing variant pattern")?;
                    Ok(Pattern::VariantCall { name: n, args })
                } else {
                    self.advance();
                    Ok(Pattern::Binding(n))
                }
            }
            Token::LBrace => {
                self.advance();
                let (fields, rest) = self.parse_struct_pattern_body()?;
                Ok(Pattern::Struct { type_name: None, fields, rest })
            }
            _ => {
                let e = self.parse_primary()?;
                if self.matches(&Token::DotDot) {
                    let end = self.parse_primary()?;
                    Ok(Pattern::Range { start: e, end })
                } else {
                    Ok(Pattern::Literal(e))
                }
            }
        }
    }

    fn parse_struct_pattern_body(&mut self) -> Result<(Vec<(String, Pattern)>, bool)> {
        let mut fields = Vec::new();
        let mut rest = false;
        if !self.check(&Token::RBrace) {
            loop {
                if self.check(&Token::DotDot) {
                    self.advance();
                    rest = true;
                    break;
                }
                let fname = self.expect_ident("expected field name in struct pattern")?;
                let sub = if self.matches(&Token::Colon) {
                    self.parse_pattern()?
                } else {
                    Pattern::Binding(fname.clone())
                };
                fields.push((fname, sub));
                if !self.matches(&Token::Comma) { break; }
            }
        }
        self.expect(&Token::RBrace, "expected '}' closing struct pattern")?;
        Ok((fields, rest))
    }

    fn peek(&self) -> &Spanned {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek_kind(&self, offset: usize) -> Option<&Token> {
        self.toks.get(self.pos + offset).map(|s| &s.token)
    }

    fn advance(&mut self) -> &Spanned {
        let t = &self.toks[self.pos];
        if !matches!(t.token, Token::Eof) { self.pos += 1; }
        &self.toks[self.pos - 1]
    }

    fn advance_owned(&mut self) -> Spanned {
        let t = self.toks[self.pos].clone();
        if !matches!(t.token, Token::Eof) { self.pos += 1; }
        t
    }

    fn at_end(&self) -> bool {
        matches!(self.peek().token, Token::Eof)
    }

    fn check(&self, t: &Token) -> bool {
        std::mem::discriminant(&self.peek().token) == std::mem::discriminant(t)
    }

    fn matches(&mut self, t: &Token) -> bool {
        if self.check(t) { self.advance(); true } else { false }
    }

    fn expect(&mut self, t: &Token, msg: &str) -> Result<()> {
        if self.check(t) { self.advance(); Ok(()) }
        else { Err(self.err(msg)) }
    }

    fn expect_ident(&mut self, msg: &str) -> Result<String> {
        let sp = self.peek().span;
        if let Token::Ident(name) = &self.peek().token {
            let n = name.clone();
            self.advance();
            Ok(n)
        } else {
            Err(RockError::parse(msg, sp.line, sp.col))
        }
    }

    fn err(&self, msg: &str) -> RockError {
        let sp = self.peek().span;
        RockError::parse(format!("{} (got {:?})", msg, self.peek().token), sp.line, sp.col)
    }
}

#[allow(dead_code)]
fn _unused(_: Span) {}
