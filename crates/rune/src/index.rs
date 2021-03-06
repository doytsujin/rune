use crate::ast;
use crate::collections::HashMap;
use crate::error::{CompileError, CompileResult};
use crate::index_scopes::IndexScopes;
use crate::items::Items;
use crate::query::{Build, BuildEntry, Function, Indexed, IndexedEntry, InstanceFunction, Query};
use crate::sources::Sources;
use crate::traits::Resolve as _;
use crate::warning::Warnings;
use crate::{SourceId, UnitBuilder};
use runestick::{Call, CompileMeta, Context, Hash, Item, Source, Span, Type};
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) struct Macro {
    pub(crate) items: Items,
    pub(crate) ast: ast::ExprCallMacro,
    pub(crate) source: Arc<Source>,
    pub(crate) source_id: usize,
    pub(crate) scopes: IndexScopes,
    pub(crate) impl_items: Vec<Item>,
    pub(crate) kind: MacroKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MacroKind {
    Expr,
}

/// Import to process.
pub(crate) struct Import {
    pub(crate) item: Item,
    pub(crate) ast: ast::DeclUse,
    pub(crate) source: Arc<Source>,
    pub(crate) source_id: usize,
}

impl Import {
    /// Process the import, populating the unit.
    pub(crate) fn process(
        self,
        context: &Context,
        unit: &mut UnitBuilder,
    ) -> Result<(), CompileError> {
        let Self {
            item,
            ast: decl_use,
            source,
            source_id,
        } = self;

        let span = decl_use.span();

        let mut name = Item::empty();
        let first = decl_use.first.resolve(&*source)?;
        name.push(first);

        let mut it = decl_use.rest.iter();
        let last = it.next_back();

        for (_, c) in it {
            match c {
                ast::DeclUseComponent::Wildcard(t) => {
                    return Err(CompileError::UnsupportedWildcard { span: t.span() });
                }
                ast::DeclUseComponent::Ident(ident) => {
                    name.push(ident.resolve(&*source)?);
                }
            }
        }

        if let Some((_, c)) = last {
            match c {
                ast::DeclUseComponent::Wildcard(..) => {
                    let mut new_names = Vec::new();

                    if !context.contains_prefix(&name) && !unit.contains_prefix(&name) {
                        return Err(CompileError::MissingModule { span, item: name });
                    }

                    let iter = context
                        .iter_components(&name)
                        .chain(unit.iter_components(&name));

                    for c in iter {
                        let mut name = name.clone();
                        name.push(c);
                        new_names.push(name);
                    }

                    for name in new_names {
                        unit.new_import(item.clone(), &name, span, source_id)?;
                    }
                }
                ast::DeclUseComponent::Ident(ident) => {
                    name.push(ident.resolve(&*source)?);
                    unit.new_import(item.clone(), &name, span, source_id)?;
                }
            }
        }

        Ok(())
    }
}

pub(crate) struct Indexer<'a> {
    pub(crate) loaded: &'a mut HashMap<Item, (SourceId, Span)>,
    pub(crate) query: &'a mut Query,
    /// Imports to process.
    pub(crate) imports: &'a mut VecDeque<Import>,
    /// Macros to queue up for building.
    pub(crate) macros: &'a mut VecDeque<Macro>,
    /// Source builders.
    pub(crate) sources: &'a mut Sources,
    /// Native context.
    pub(crate) source_id: SourceId,
    pub(crate) source: Arc<Source>,
    pub(crate) warnings: &'a mut Warnings,
    pub(crate) items: Items,
    pub(crate) scopes: IndexScopes,
    /// Set if we are inside of an impl block.
    pub(crate) impl_items: Vec<Item>,
}

impl<'a> Indexer<'a> {
    /// Construct the calling convention based on the parameters.
    fn call(generator: bool, is_async: bool) -> Call {
        if is_async {
            if generator {
                Call::Stream
            } else {
                Call::Async
            }
        } else if generator {
            Call::Generator
        } else {
            Call::Immediate
        }
    }

    /// Handle a filesystem module.
    pub(crate) fn handle_file_mod(&mut self, decl_mod: &ast::DeclMod) -> CompileResult<()> {
        let span = decl_mod.span();
        let name = decl_mod.name.resolve(&*self.source)?;
        let _guard = self.items.push_name(name);

        let path = match self.source.path() {
            Some(path) => path,
            None => {
                return Err(CompileError::UnsupportedFileMod { span });
            }
        };

        let base = match path.parent() {
            Some(parent) => parent.join(name),
            None => {
                return Err(CompileError::UnsupportedFileMod { span });
            }
        };

        let candidates = [
            base.join("mod").with_extension("rn"),
            base.with_extension("rn"),
        ];

        let mut found = None;

        for path in &candidates[..] {
            if path.is_file() {
                found = Some(path);
                break;
            }
        }

        let path = match found {
            Some(path) => path,
            None => {
                return Err(CompileError::ModNotFound {
                    path: base.to_owned(),
                    span,
                });
            }
        };

        let item = self.items.item();

        if let Some(existing) = self.loaded.insert(item.clone(), (self.source_id, span)) {
            return Err(CompileError::ModAlreadyLoaded {
                item: item.clone(),
                span,
                existing,
            });
        }

        let source = match Source::from_path(path) {
            Ok(source) => source,
            Err(error) => {
                return Err(CompileError::ModFileError {
                    span,
                    path: path.to_owned(),
                    error,
                });
            }
        };

        self.sources.insert(item, source);
        Ok(())
    }
}

pub(crate) trait Index<T> {
    /// Walk the current type with the given item.
    fn index(&mut self, item: &T) -> CompileResult<()>;
}

impl Index<ast::DeclFile> for Indexer<'_> {
    fn index(&mut self, decl_file: &ast::DeclFile) -> CompileResult<()> {
        for (decl, semi_colon) in &decl_file.decls {
            if let Some(semi_colon) = semi_colon {
                if !decl.needs_semi_colon() {
                    self.warnings
                        .uneccessary_semi_colon(self.source_id, semi_colon.span());
                }
            }

            self.index(decl)?;
        }

        Ok(())
    }
}

impl Index<ast::DeclFn> for Indexer<'_> {
    fn index(&mut self, decl_fn: &ast::DeclFn) -> CompileResult<()> {
        let span = decl_fn.span();
        let is_toplevel = self.items.is_empty();
        let _guard = self.items.push_name(decl_fn.name.resolve(&*self.source)?);

        let item = self.items.item();

        let guard = self.scopes.push_function(decl_fn.async_.is_some());

        for (arg, _) in &decl_fn.args.items {
            match arg {
                ast::FnArg::Self_(s) => {
                    let span = s.span();
                    self.scopes.declare("self", span)?;
                }
                ast::FnArg::Ident(ident) => {
                    let span = ident.span();
                    let ident = ident.resolve(&*self.source)?;
                    self.scopes.declare(ident, span)?;
                }
                _ => (),
            }
        }

        self.index(&decl_fn.body)?;

        let f = guard.into_function(span)?;
        let call = Self::call(f.generator, f.is_async);

        let fun = Function {
            ast: decl_fn.clone(),
            call,
        };

        if decl_fn.is_instance() {
            let impl_item = self
                .impl_items
                .last()
                .ok_or_else(|| CompileError::InstanceFunctionOutsideImpl { span })?;

            let f = InstanceFunction {
                ast: fun.ast,
                impl_item: impl_item.clone(),
                instance_span: span,
                call: fun.call,
            };

            // NB: all instance functions must be pre-emptively built,
            // because statically we don't know if they will be used or
            // not.
            self.query.queue.push_back(BuildEntry {
                item: item.clone(),
                build: Build::InstanceFunction(f),
                source: self.source.clone(),
                source_id: self.source_id,
            });

            let meta = CompileMeta::Function {
                value_type: Type::Hash(Hash::type_hash(&item)),
                item: item.clone(),
            };

            self.query.unit.borrow_mut().insert_meta(meta)?;
        } else if is_toplevel {
            // NB: immediately compile all toplevel functions.
            self.query.queue.push_back(BuildEntry {
                item: item.clone(),
                build: Build::Function(fun),
                source: self.source.clone(),
                source_id: self.source_id,
            });

            self.query
                .unit
                .borrow_mut()
                .insert_meta(CompileMeta::Function {
                    value_type: Type::Hash(Hash::type_hash(&item)),
                    item,
                })?;
        } else {
            // NB: non toplevel functions can be indexed for later construction.
            self.query.index(
                item,
                IndexedEntry {
                    indexed: Indexed::Function(fun),
                    source: self.source.clone(),
                    source_id: self.source_id,
                },
                span,
            )?;
        }

        Ok(())
    }
}

impl Index<ast::ExprBlock> for Indexer<'_> {
    fn index(&mut self, expr_block: &ast::ExprBlock) -> Result<(), CompileError> {
        let span = expr_block.span();

        if let Some(..) = &expr_block.async_ {
            let _guard = self.items.push_async_block();
            let guard = self.scopes.push_closure(true);

            for (expr, _) in &expr_block.exprs {
                self.index(expr)?;
            }

            if let Some(expr) = &expr_block.trailing_expr {
                self.index(&**expr)?;
            }

            let c = guard.into_closure(span)?;

            let captures = Arc::new(c.captures);
            let call = Self::call(c.generator, c.is_async);

            self.query.index_async_block(
                self.items.item(),
                expr_block.clone(),
                captures,
                call,
                self.source.clone(),
                self.source_id,
            )?;
        } else {
            let _guard = self.items.push_block();
            let _guard = self.scopes.push_scope();

            for (expr, _) in &expr_block.exprs {
                self.index(expr)?;
            }

            if let Some(expr) = &expr_block.trailing_expr {
                self.index(&**expr)?;
            }
        }

        Ok(())
    }
}

impl Index<ast::ExprLet> for Indexer<'_> {
    fn index(&mut self, expr_let: &ast::ExprLet) -> Result<(), CompileError> {
        self.index(&expr_let.pat)?;
        self.index(&*expr_let.expr)?;
        Ok(())
    }
}

impl Index<ast::Ident> for Indexer<'_> {
    fn index(&mut self, ident: &ast::Ident) -> Result<(), CompileError> {
        let span = ident.span();
        let ident = ident.resolve(&*self.source)?;
        self.scopes.declare(ident, span)?;
        Ok(())
    }
}

impl Index<ast::Pat> for Indexer<'_> {
    fn index(&mut self, pat: &ast::Pat) -> Result<(), CompileError> {
        match pat {
            ast::Pat::PatPath(pat_path) => {
                if let Some(ident) = pat_path.path.try_as_ident() {
                    self.index(ident)?;
                }
            }
            ast::Pat::PatObject(pat_object) => {
                self.index(pat_object)?;
            }
            ast::Pat::PatVec(pat_vec) => {
                self.index(pat_vec)?;
            }
            ast::Pat::PatTuple(pat_tuple) => {
                self.index(pat_tuple)?;
            }
            ast::Pat::PatByte(..) => (),
            ast::Pat::PatIgnore(..) => (),
            ast::Pat::PatNumber(..) => (),
            ast::Pat::PatString(..) => (),
            ast::Pat::PatUnit(..) => (),
            ast::Pat::PatChar(..) => (),
        }

        Ok(())
    }
}

impl Index<ast::PatTuple> for Indexer<'_> {
    fn index(&mut self, pat_tuple: &ast::PatTuple) -> Result<(), CompileError> {
        for (pat, _) in &pat_tuple.items {
            self.index(&**pat)?;
        }

        Ok(())
    }
}

impl Index<ast::PatObject> for Indexer<'_> {
    fn index(&mut self, pat_object: &ast::PatObject) -> Result<(), CompileError> {
        for (field, _) in &pat_object.fields {
            if let Some((_, pat)) = &field.binding {
                self.index(pat)?;
            } else {
                match &field.key {
                    ast::LitObjectKey::Ident(ident) => {
                        self.index(ident)?;
                    }
                    ast::LitObjectKey::LitStr(..) => (),
                }
            }
        }

        Ok(())
    }
}

impl Index<ast::PatVec> for Indexer<'_> {
    fn index(&mut self, pat_vec: &ast::PatVec) -> Result<(), CompileError> {
        for (pat, _) in &pat_vec.items {
            self.index(&**pat)?;
        }

        Ok(())
    }
}

impl Index<ast::Expr> for Indexer<'_> {
    fn index(&mut self, expr: &ast::Expr) -> Result<(), CompileError> {
        match expr {
            ast::Expr::Self_(..) => {
                self.scopes.mark_use("self");
            }
            ast::Expr::Path(path) => {
                self.index(path)?;
            }
            ast::Expr::ExprLet(expr_let) => {
                self.index(expr_let)?;
            }
            ast::Expr::ExprBlock(block) => {
                self.index(block)?;
            }
            ast::Expr::ExprGroup(expr) => {
                self.index(&*expr.expr)?;
            }
            ast::Expr::ExprIf(expr_if) => {
                self.index(expr_if)?;
            }
            ast::Expr::ExprBinary(expr_binary) => {
                self.index(expr_binary)?;
            }
            ast::Expr::ExprMatch(expr_if) => {
                self.index(expr_if)?;
            }
            ast::Expr::Decl(decl) => {
                self.index(decl)?;
            }
            ast::Expr::ExprClosure(expr_closure) => {
                self.index(expr_closure)?;
            }
            ast::Expr::ExprWhile(expr_while) => {
                self.index(expr_while)?;
            }
            ast::Expr::ExprLoop(expr_loop) => {
                self.index(expr_loop)?;
            }
            ast::Expr::ExprFor(expr_for) => {
                self.index(expr_for)?;
            }
            ast::Expr::ExprIndexSet(expr_index_set) => {
                self.index(expr_index_set)?;
            }
            ast::Expr::ExprFieldAccess(expr_field_access) => {
                self.index(expr_field_access)?;
            }
            ast::Expr::ExprUnary(expr_unary) => {
                self.index(expr_unary)?;
            }
            ast::Expr::ExprIndexGet(expr_index_get) => {
                self.index(expr_index_get)?;
            }
            ast::Expr::ExprBreak(expr_break) => {
                self.index(expr_break)?;
            }
            ast::Expr::ExprYield(expr_yield) => {
                self.index(expr_yield)?;
            }
            ast::Expr::ExprReturn(expr_return) => {
                self.index(expr_return)?;
            }
            ast::Expr::ExprAwait(expr_await) => {
                self.index(expr_await)?;
            }
            ast::Expr::ExprTry(expr_try) => {
                self.index(expr_try)?;
            }
            ast::Expr::ExprSelect(expr_select) => {
                self.index(expr_select)?;
            }
            // ignored because they have no effect on indexing.
            ast::Expr::ExprCall(expr_call) => {
                self.index(expr_call)?;
            }
            ast::Expr::LitTemplate(lit_template) => {
                self.index(lit_template)?;
            }
            // NB: literals have nothing to index, they don't export language
            // items.
            ast::Expr::LitUnit(..) => (),
            ast::Expr::LitBool(..) => (),
            ast::Expr::LitByte(..) => (),
            ast::Expr::LitChar(..) => (),
            ast::Expr::LitNumber(..) => (),
            ast::Expr::LitObject(..) => (),
            ast::Expr::LitStr(..) => (),
            ast::Expr::LitByteStr(..) => (),
            ast::Expr::LitTuple(..) => (),
            ast::Expr::LitVec(..) => (),
            // NB: macros have nothing to index, they don't export language
            // items.
            ast::Expr::ExprCallMacro(expr_call_macro) => {
                let _guard = self.items.push_macro();

                self.macros.push_back(Macro {
                    items: self.items.snapshot(),
                    ast: expr_call_macro.clone(),
                    source: self.source.clone(),
                    source_id: self.source_id,
                    scopes: self.scopes.snapshot(),
                    impl_items: self.impl_items.clone(),
                    kind: MacroKind::Expr,
                });
            }
        }

        Ok(())
    }
}

impl Index<ast::ExprIf> for Indexer<'_> {
    fn index(&mut self, expr_if: &ast::ExprIf) -> Result<(), CompileError> {
        self.index(&expr_if.condition)?;
        self.index(&*expr_if.block)?;

        for expr_else_if in &expr_if.expr_else_ifs {
            self.index(&expr_else_if.condition)?;
            self.index(&*expr_else_if.block)?;
        }

        if let Some(expr_else) = &expr_if.expr_else {
            self.index(&*expr_else.block)?;
        }

        Ok(())
    }
}

impl Index<ast::ExprBinary> for Indexer<'_> {
    fn index(&mut self, expr_binary: &ast::ExprBinary) -> Result<(), CompileError> {
        self.index(&*expr_binary.lhs)?;
        self.index(&*expr_binary.rhs)?;
        Ok(())
    }
}

impl Index<ast::ExprMatch> for Indexer<'_> {
    fn index(&mut self, expr_match: &ast::ExprMatch) -> Result<(), CompileError> {
        self.index(&*expr_match.expr)?;

        for (branch, _) in &expr_match.branches {
            if let Some((_, condition)) = &branch.condition {
                self.index(&**condition)?;
            }

            let _guard = self.scopes.push_scope();
            self.index(&branch.pat)?;
            self.index(&*branch.body)?;
        }

        Ok(())
    }
}

impl Index<ast::Condition> for Indexer<'_> {
    fn index(&mut self, condition: &ast::Condition) -> Result<(), CompileError> {
        match condition {
            ast::Condition::Expr(expr) => {
                self.index(&**expr)?;
            }
            ast::Condition::ExprLet(expr_let) => {
                self.index(&**expr_let)?;
            }
        }

        Ok(())
    }
}

impl Index<ast::Decl> for Indexer<'_> {
    fn index(&mut self, decl: &ast::Decl) -> Result<(), CompileError> {
        match decl {
            ast::Decl::DeclUse(import) => {
                self.imports.push_back(Import {
                    item: self.items.item(),
                    ast: import.clone(),
                    source: self.source.clone(),
                    source_id: self.source_id,
                });
            }
            ast::Decl::DeclEnum(decl_enum) => {
                let _guard = self.items.push_name(decl_enum.name.resolve(&*self.source)?);

                let span = decl_enum.span();
                let enum_item = self.items.item();

                self.query.index_enum(
                    enum_item.clone(),
                    self.source.clone(),
                    self.source_id,
                    span,
                )?;

                for (variant, body, _) in &decl_enum.variants {
                    let _guard = self.items.push_name(variant.resolve(&*self.source)?);

                    let span = variant.span();

                    self.query.index_variant(
                        self.items.item(),
                        enum_item.clone(),
                        body.clone(),
                        self.source.clone(),
                        self.source_id,
                        span,
                    )?;
                }
            }
            ast::Decl::DeclStruct(decl_struct) => {
                let _guard = self
                    .items
                    .push_name(decl_struct.ident.resolve(&*self.source)?);

                self.query.index_struct(
                    self.items.item(),
                    decl_struct.clone(),
                    self.source.clone(),
                    self.source_id,
                )?;
            }
            ast::Decl::DeclFn(decl_fn) => {
                self.index(decl_fn)?;
            }
            ast::Decl::DeclImpl(decl_impl) => {
                let mut guards = Vec::new();

                for ident in decl_impl.path.components() {
                    guards.push(self.items.push_name(ident.resolve(&*self.source)?));
                }

                self.impl_items.push(self.items.item());

                for decl_fn in &decl_impl.functions {
                    self.index(decl_fn)?;
                }

                self.impl_items.pop();
            }
            ast::Decl::DeclMod(decl_mod) => {
                if let Some(body) = &decl_mod.body {
                    let name = decl_mod.name.resolve(&*self.source)?;
                    let _guard = self.items.push_name(name);
                    self.index(&*body.file)?;
                } else {
                    self.handle_file_mod(decl_mod)?;
                }
            }
        }

        Ok(())
    }
}

impl Index<ast::Path> for Indexer<'_> {
    fn index(&mut self, path: &ast::Path) -> Result<(), CompileError> {
        if let Some(ident) = path.try_as_ident() {
            let ident = ident.resolve(&*self.source)?;
            self.scopes.mark_use(ident);
        }

        Ok(())
    }
}

impl Index<ast::ExprWhile> for Indexer<'_> {
    fn index(&mut self, expr_while: &ast::ExprWhile) -> Result<(), CompileError> {
        let _guard = self.scopes.push_scope();
        self.index(&expr_while.condition)?;
        self.index(&*expr_while.body)?;
        Ok(())
    }
}

impl Index<ast::ExprLoop> for Indexer<'_> {
    fn index(&mut self, expr_loop: &ast::ExprLoop) -> Result<(), CompileError> {
        let _guard = self.scopes.push_scope();
        self.index(&*expr_loop.body)?;
        Ok(())
    }
}

impl Index<ast::ExprFor> for Indexer<'_> {
    fn index(&mut self, expr_for: &ast::ExprFor) -> Result<(), CompileError> {
        // NB: creating the iterator is evaluated in the parent scope.
        self.index(&*expr_for.iter)?;

        let _guard = self.scopes.push_scope();
        self.index(&expr_for.var)?;
        self.index(&*expr_for.body)?;
        Ok(())
    }
}

impl Index<ast::ExprClosure> for Indexer<'_> {
    fn index(&mut self, expr_closure: &ast::ExprClosure) -> Result<(), CompileError> {
        let _guard = self.items.push_closure();
        let guard = self.scopes.push_closure(expr_closure.async_.is_some());
        let span = expr_closure.span();

        for (arg, _) in expr_closure.args.as_slice() {
            match arg {
                ast::FnArg::Self_(s) => {
                    return Err(CompileError::UnsupportedSelf { span: s.span() });
                }
                ast::FnArg::Ident(ident) => {
                    let ident = ident.resolve(&*self.source)?;
                    self.scopes.declare(ident, span)?;
                }
                ast::FnArg::Ignore(..) => (),
            }
        }

        self.index(&*expr_closure.body)?;

        let c = guard.into_closure(span)?;

        let captures = Arc::new(c.captures);
        let call = Self::call(c.generator, c.is_async);

        self.query.index_closure(
            self.items.item(),
            expr_closure.clone(),
            captures,
            call,
            self.source.clone(),
            self.source_id,
        )?;

        Ok(())
    }
}

impl Index<ast::ExprIndexSet> for Indexer<'_> {
    fn index(&mut self, expr_index_set: &ast::ExprIndexSet) -> Result<(), CompileError> {
        self.index(&*expr_index_set.value)?;
        self.index(&*expr_index_set.index)?;
        self.index(&*expr_index_set.target)?;
        Ok(())
    }
}

impl Index<ast::ExprFieldAccess> for Indexer<'_> {
    fn index(&mut self, expr_field_access: &ast::ExprFieldAccess) -> Result<(), CompileError> {
        self.index(&*expr_field_access.expr)?;
        Ok(())
    }
}

impl Index<ast::ExprUnary> for Indexer<'_> {
    fn index(&mut self, expr_unary: &ast::ExprUnary) -> Result<(), CompileError> {
        self.index(&*expr_unary.expr)?;
        Ok(())
    }
}

impl Index<ast::ExprIndexGet> for Indexer<'_> {
    fn index(&mut self, expr_index_get: &ast::ExprIndexGet) -> Result<(), CompileError> {
        self.index(&*expr_index_get.index)?;
        self.index(&*expr_index_get.target)?;
        Ok(())
    }
}

impl Index<ast::ExprBreak> for Indexer<'_> {
    fn index(&mut self, expr_break: &ast::ExprBreak) -> Result<(), CompileError> {
        if let Some(expr) = &expr_break.expr {
            match expr {
                ast::ExprBreakValue::Expr(expr) => {
                    self.index(&**expr)?;
                }
                ast::ExprBreakValue::Label(..) => (),
            }
        }

        Ok(())
    }
}

impl Index<ast::ExprYield> for Indexer<'_> {
    fn index(&mut self, expr_yield: &ast::ExprYield) -> Result<(), CompileError> {
        let span = expr_yield.span();
        self.scopes.mark_yield(span)?;

        if let Some(expr) = &expr_yield.expr {
            self.index(&**expr)?;
        }

        Ok(())
    }
}

impl Index<ast::ExprReturn> for Indexer<'_> {
    fn index(&mut self, expr_return: &ast::ExprReturn) -> Result<(), CompileError> {
        if let Some(expr) = expr_return.expr.as_deref() {
            self.index(expr)?;
        }

        Ok(())
    }
}

impl Index<ast::ExprAwait> for Indexer<'_> {
    fn index(&mut self, expr_await: &ast::ExprAwait) -> Result<(), CompileError> {
        let span = expr_await.span();
        self.scopes.mark_await(span)?;
        self.index(&*expr_await.expr)?;
        Ok(())
    }
}

impl Index<ast::ExprTry> for Indexer<'_> {
    fn index(&mut self, expr_try: &ast::ExprTry) -> Result<(), CompileError> {
        self.index(&*expr_try.expr)?;
        Ok(())
    }
}

impl Index<ast::ExprSelect> for Indexer<'_> {
    fn index(&mut self, expr_select: &ast::ExprSelect) -> Result<(), CompileError> {
        self.scopes.mark_await(expr_select.span())?;

        for (branch, _) in &expr_select.branches {
            // NB: expression to evaluate future is evaled in parent scope.
            self.index(&*branch.expr)?;

            let _guard = self.scopes.push_scope();
            self.index(&branch.pat)?;
            self.index(&*branch.body)?;
        }

        if let Some((branch, _)) = &expr_select.default_branch {
            let _guard = self.scopes.push_scope();
            self.index(&*branch.body)?;
        }

        Ok(())
    }
}

impl Index<ast::ExprCall> for Indexer<'_> {
    fn index(&mut self, expr_call: &ast::ExprCall) -> Result<(), CompileError> {
        for (expr, _) in expr_call.args.items.iter() {
            self.index(expr)?;
        }

        self.index(&*expr_call.expr)?;
        Ok(())
    }
}

impl Index<ast::LitTemplate> for Indexer<'_> {
    fn index(&mut self, lit_template: &ast::LitTemplate) -> Result<(), CompileError> {
        let template = lit_template.resolve(&*self.source)?;

        for c in &template.components {
            match c {
                ast::TemplateComponent::Expr(expr) => {
                    self.index(&**expr)?;
                }
                ast::TemplateComponent::String(..) => (),
            }
        }

        Ok(())
    }
}
