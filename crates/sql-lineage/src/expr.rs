//! Collects the physical sources of an expression.
//!
//! The walk matches every [`Expr`] variant explicitly; kinds lineage does not
//! model are rejected. Window specifications record their own
//! [`Role::Window`] references; everything else (including `CASE`
//! conditions, aggregate arguments, and subquery outputs) flows into the
//! caller's source set.

use sqlparser::ast::{
    AccessExpr, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArgumentClause, FunctionArguments, InTarget,
    Query, Subscript, WindowFrameBound, WindowSpec, WindowType,
};

use crate::bind::{Clause, Level, Sources};
use crate::error::LineageError;
use crate::functions;
use crate::model::Role;
use crate::resolve::{Ctx, MAX_EXPR_DEPTH, idents, unsupported};

impl Ctx<'_> {
    /// Adds the sources of `expr` to `acc`.
    ///
    /// # Errors
    /// Returns [`LineageError`] for unresolvable columns, unmodeled kinds, or
    /// nesting deeper than [`MAX_EXPR_DEPTH`].
    pub(crate) fn expr(
        &mut self,
        expr: &Expr,
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        if self.expr_depth >= MAX_EXPR_DEPTH {
            return Err(LineageError::DepthLimit);
        }
        self.expr_depth += 1;
        let result = self.expr_inner(expr, level, clause, acc);
        self.expr_depth -= 1;
        result
    }

    /// Body of [`Ctx::expr`], inside the depth guard.
    #[expect(clippy::too_many_lines, reason = "one exhaustive match over every Expr variant")]
    fn expr_inner(
        &mut self,
        expr: &Expr,
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        match expr {
            Expr::Identifier(id) => {
                acc.extend(level.column(clause, &[], id)?.0);
                Ok(())
            }
            Expr::CompoundIdentifier(parts) => {
                let Some((name, qualifier)) = parts.split_last() else {
                    return Err(unsupported("empty identifier"));
                };
                acc.extend(level.column(clause, qualifier, name)?.0);
                Ok(())
            }
            Expr::IsFalse(e)
            | Expr::IsNotFalse(e)
            | Expr::IsTrue(e)
            | Expr::IsNotTrue(e)
            | Expr::IsNull(e)
            | Expr::IsNotNull(e)
            | Expr::IsUnknown(e)
            | Expr::IsNotUnknown(e)
            | Expr::IsJson { expr: e, .. }
            | Expr::IsNormalized { expr: e, .. }
            | Expr::UnaryOp { expr: e, .. }
            | Expr::Cast { expr: e, .. }
            | Expr::Extract { expr: e, .. }
            | Expr::Ceil { expr: e, .. }
            | Expr::Floor { expr: e, .. }
            | Expr::Collate { expr: e, .. }
            | Expr::Nested(e)
            | Expr::Prefixed { value: e, .. } => self.expr(e, level, clause, acc),
            Expr::IsDistinctFrom(a, b)
            | Expr::IsNotDistinctFrom(a, b)
            | Expr::BinaryOp { left: a, right: b, .. }
            | Expr::AnyOp { left: a, right: b, .. }
            | Expr::AllOp { left: a, right: b, .. }
            | Expr::RLike {
                expr: a, pattern: b, ..
            }
            | Expr::AtTimeZone {
                timestamp: a,
                time_zone: b,
            }
            | Expr::Position { expr: a, r#in: b } => {
                self.expr(a, level, clause, acc)?;
                self.expr(b, level, clause, acc)
            }
            Expr::MemberOf(member) => {
                self.expr(&member.value, level, clause, acc)?;
                self.expr(&member.array, level, clause, acc)
            }
            Expr::Like {
                expr,
                pattern,
                escape_char,
                ..
            }
            | Expr::ILike {
                expr,
                pattern,
                escape_char,
                ..
            }
            | Expr::SimilarTo {
                expr,
                pattern,
                escape_char,
                ..
            } => {
                self.expr(expr, level, clause, acc)?;
                self.expr(pattern, level, clause, acc)?;
                self.opt(escape_char.as_deref(), level, clause, acc)
            }
            Expr::In {
                expr,
                target: InTarget::List(list),
                ..
            } => {
                self.expr(expr, level, clause, acc)?;
                self.all(list, level, clause, acc)
            }
            Expr::In {
                expr,
                target: InTarget::Subquery(subquery),
                ..
            } => {
                self.expr(expr, level, clause, acc)?;
                self.subquery(subquery, level, acc)
            }
            Expr::Between { expr, low, high, .. } => {
                self.expr(expr, level, clause, acc)?;
                self.expr(low, level, clause, acc)?;
                self.expr(high, level, clause, acc)
            }
            Expr::Convert { expr, styles, .. } => {
                self.expr(expr, level, clause, acc)?;
                self.all(styles, level, clause, acc)
            }
            Expr::Substring {
                expr,
                substring_from,
                substring_for,
                ..
            } => {
                self.expr(expr, level, clause, acc)?;
                self.opt(substring_from.as_deref(), level, clause, acc)?;
                self.opt(substring_for.as_deref(), level, clause, acc)
            }
            Expr::Trim {
                expr,
                trim_what,
                trim_characters,
                ..
            } => {
                self.expr(expr, level, clause, acc)?;
                self.opt(trim_what.as_deref(), level, clause, acc)?;
                self.all(trim_characters.as_deref().unwrap_or_default(), level, clause, acc)
            }
            Expr::Overlay {
                expr,
                overlay_what,
                overlay_from,
                overlay_for,
            } => {
                self.expr(expr, level, clause, acc)?;
                self.expr(overlay_what, level, clause, acc)?;
                self.expr(overlay_from, level, clause, acc)?;
                self.opt(overlay_for.as_deref(), level, clause, acc)
            }
            Expr::Value(_) | Expr::TypedString(_) => Ok(()),
            Expr::Function(function) => self.function(function, level, clause, acc),
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => {
                self.opt(operand.as_deref(), level, clause, acc)?;
                for when in conditions {
                    self.expr(&when.condition, level, clause, acc)?;
                    self.expr(&when.result, level, clause, acc)?;
                }
                self.opt(else_result.as_deref(), level, clause, acc)
            }
            Expr::Exists { subquery, .. } | Expr::Subquery(subquery) => self.subquery(subquery, level, acc),
            Expr::GroupingSets(sets) | Expr::Cube(sets) | Expr::Rollup(sets) => {
                for set in sets {
                    self.all(set, level, clause, acc)?;
                }
                Ok(())
            }
            Expr::Tuple(items) => self.all(items, level, clause, acc),
            Expr::Array(array) => self.all(&array.elem, level, clause, acc),
            Expr::Interval(interval) => self.expr(&interval.value, level, clause, acc),
            Expr::MatchAgainst { columns, .. } => {
                for name in columns {
                    let parts = idents(name)?;
                    let Some((name, qualifier)) = parts.split_last() else {
                        return Err(unsupported("empty identifier"));
                    };
                    acc.extend(level.column(clause, qualifier, name)?.0);
                }
                Ok(())
            }
            Expr::CompoundFieldAccess { root, access_chain } => {
                self.field_access(root, access_chain, level, clause, acc)
            }
            Expr::Wildcard(_) | Expr::QualifiedWildcard(..) => Err(unsupported("wildcard expression")),
            Expr::JsonAccess { .. } => Err(unsupported("JSON path access")),
            Expr::In {
                target: InTarget::Unnest(_),
                ..
            } => Err(unsupported("IN UNNEST")),
            Expr::In {
                target: InTarget::Table { .. },
                ..
            } => Err(unsupported("IN table")),
            Expr::Struct { .. } | Expr::Named { .. } | Expr::Dictionary(_) | Expr::Map(_) => {
                Err(unsupported("struct or map literal"))
            }
            Expr::OuterJoin(_) => Err(unsupported("(+) outer join")),
            Expr::Prior(_) => Err(unsupported("PRIOR")),
            Expr::Lambda(_) => Err(unsupported("lambda")),
        }
    }

    /// Adds the sources of a field access or subscript chain.
    ///
    /// Kept out of [`Ctx::expr_inner`] so its locals do not grow every
    /// recursion frame.
    fn field_access(
        &mut self,
        root: &Expr,
        access_chain: &[AccessExpr],
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        // `o.tags[1]` parses as root `o` with chain `.tags`, `[1]`.
        let mut chain = access_chain;
        let mut parts = match root {
            Expr::Identifier(id) => vec![id.clone()],
            Expr::CompoundIdentifier(parts) => parts.clone(),
            _ => Vec::new(),
        };
        if parts.is_empty() {
            self.expr(root, level, clause, acc)?;
        } else {
            while let [AccessExpr::Dot(Expr::Identifier(id)), rest @ ..] = chain {
                parts.push(id.clone());
                chain = rest;
            }
            let Some((name, qualifier)) = parts.split_last() else {
                return Err(unsupported("empty identifier"));
            };
            acc.extend(level.column(clause, qualifier, name)?.0);
        }
        for access in chain {
            match access {
                // A field of a parenthesized value, `(u).f`; the value's sources cover it.
                AccessExpr::Dot(Expr::Identifier(_)) if matches!(root, Expr::Nested(_)) => {}
                AccessExpr::Dot(_) => return Err(unsupported("composite field access")),
                AccessExpr::Subscript(Subscript::Index { index }) => self.expr(index, level, clause, acc)?,
                AccessExpr::Subscript(Subscript::Slice {
                    lower_bound,
                    upper_bound,
                    stride,
                }) => {
                    self.opt(lower_bound.as_ref(), level, clause, acc)?;
                    self.opt(upper_bound.as_ref(), level, clause, acc)?;
                    self.opt(stride.as_ref(), level, clause, acc)?;
                }
            }
        }
        Ok(())
    }

    fn all(
        &mut self,
        exprs: &[Expr],
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        for expr in exprs {
            self.expr(expr, level, clause, acc)?;
        }
        Ok(())
    }

    fn opt(
        &mut self,
        expr: Option<&Expr>,
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        expr.map_or(Ok(()), |e| self.expr(e, level, clause, acc))
    }

    /// Adds every output source of a subquery.
    fn subquery(&mut self, query: &Query, level: &Level<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        let cols = self.query(query, level.correlated())?;
        acc.extend(cols.cols.into_iter().flat_map(|c| c.sources));
        Ok(())
    }

    /// Records a function call and adds its argument sources.
    fn function(
        &mut self,
        function: &Function,
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        let Function {
            name,
            uses_odbc_syntax: _,
            parameters,
            args,
            within_group,
            filter,
            null_treatment: _,
            over,
        } = function;
        if !matches!(parameters, FunctionArguments::None) {
            return Err(unsupported("parametric function"));
        }
        let (name, known) = functions::resolve(self.dialect, name)?;
        self.call(name, known);
        match args {
            FunctionArguments::None => {}
            FunctionArguments::Subquery(query) => self.subquery(query, level, acc)?,
            FunctionArguments::List(list) => {
                for arg in &list.args {
                    match arg {
                        FunctionArg::Named { arg, .. } | FunctionArg::Unnamed(arg) => {
                            self.arg(arg, level, clause, acc)?;
                        }
                        FunctionArg::ExprNamed { name, arg, .. } => {
                            self.expr(name, level, clause, acc)?;
                            self.arg(arg, level, clause, acc)?;
                        }
                    }
                }
                for arg_clause in &list.clauses {
                    match arg_clause {
                        FunctionArgumentClause::IgnoreOrRespectNulls(_)
                        | FunctionArgumentClause::Separator(_)
                        | FunctionArgumentClause::JsonNullClause(_)
                        | FunctionArgumentClause::JsonReturningClause(_) => {}
                        FunctionArgumentClause::Where(expr) | FunctionArgumentClause::Limit(expr) => {
                            self.expr(expr, level, clause, acc)?;
                        }
                        FunctionArgumentClause::OrderBy(items) => {
                            for item in items {
                                self.expr(&item.expr, level, clause, acc)?;
                            }
                        }
                        FunctionArgumentClause::OnOverflow(_) | FunctionArgumentClause::Having(_) => {
                            return Err(unsupported("aggregate clause (ON OVERFLOW/HAVING)"));
                        }
                    }
                }
            }
        }
        for item in within_group {
            self.expr(&item.expr, level, clause, acc)?;
        }
        self.opt(filter.as_deref(), level, clause, acc)?;
        match over {
            None | Some(WindowType::NamedWindow(_)) => Ok(()),
            Some(WindowType::WindowSpec(spec)) => self.window(spec, level, clause),
        }
    }

    /// Adds the sources of one function argument.
    fn arg(
        &mut self,
        arg: &FunctionArgExpr,
        level: &Level<'_>,
        clause: Clause,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        match arg {
            FunctionArgExpr::Expr(expr) => self.expr(expr, level, clause, acc),
            FunctionArgExpr::Wildcard => Ok(()),
            FunctionArgExpr::QualifiedWildcard(name) => {
                let sources = level
                    .whole_row(&idents(name)?)?
                    .ok_or_else(|| LineageError::ColumnNotFound {
                        column: format!("{name}.*"),
                    })?;
                acc.extend(sources);
                Ok(())
            }
            FunctionArgExpr::WildcardWithOptions(_) => Err(unsupported("wildcard modifier in function argument")),
        }
    }

    /// Records a window specification's columns as [`Role::Window`] references.
    pub(crate) fn window(&mut self, spec: &WindowSpec, level: &Level<'_>, clause: Clause) -> Result<(), LineageError> {
        let mut sources = Sources::new();
        self.all(&spec.partition_by, level, clause, &mut sources)?;
        for item in &spec.order_by {
            self.expr(&item.expr, level, clause, &mut sources)?;
        }
        if let Some(frame) = &spec.window_frame {
            for bound in std::iter::once(&frame.start_bound).chain(frame.end_bound.as_ref()) {
                match bound {
                    WindowFrameBound::CurrentRow => {}
                    WindowFrameBound::Preceding(e) | WindowFrameBound::Following(e) => {
                        self.opt(e.as_deref(), level, clause, &mut sources)?;
                    }
                }
            }
        }
        self.refer(sources, Role::Window);
        Ok(())
    }
}
