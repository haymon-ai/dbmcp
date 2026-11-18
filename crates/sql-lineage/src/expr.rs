//! Collects the physical sources of an expression.
//!
//! The walk matches every [`Expr`] variant explicitly; kinds lineage does not
//! model are rejected. Window specifications record their own
//! [`Role::Window`] references; everything else (including `CASE`
//! conditions, aggregate arguments, and subquery outputs) flows into the
//! caller's source set.

use std::collections::BTreeSet;

use sqlparser::ast::{
    AccessExpr, Expr, Function, FunctionArg, FunctionArgExpr, FunctionArgumentClause, FunctionArguments, Ident,
    ObjectName, Query, Subscript, WindowFrameBound, WindowSpec, WindowType,
};

use crate::error::LineageError;
use crate::model::{FunctionUse, Origin, Role, SourceRef};
use crate::resolve::{Ctx, MAX_EXPR_DEPTH, Walk, idents, unsupported};

type Sources = BTreeSet<SourceRef>;

impl Ctx<'_> {
    /// Adds the sources of `expr` to `acc`.
    ///
    /// # Errors
    /// Returns [`LineageError`] for unresolvable columns, unmodeled kinds, or
    /// nesting deeper than [`MAX_EXPR_DEPTH`].
    pub(crate) fn expr(&mut self, expr: &Expr, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        if self.expr_depth >= MAX_EXPR_DEPTH {
            return Err(LineageError::DepthLimit);
        }
        self.expr_depth += 1;
        let result = self.expr_inner(expr, walk, acc);
        self.expr_depth -= 1;
        result
    }

    /// Body of [`Ctx::expr`], inside the depth guard.
    #[expect(clippy::too_many_lines, reason = "one exhaustive match over every Expr variant")]
    fn expr_inner(&mut self, expr: &Expr, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        match expr {
            Expr::Identifier(id) => self.column(&[], id, walk, acc),
            Expr::CompoundIdentifier(parts) => match parts.split_last() {
                Some((name, qualifier)) => self.column(qualifier, name, walk, acc),
                None => Err(unsupported("empty identifier")),
            },
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
            | Expr::Prefixed { value: e, .. } => self.expr(e, walk, acc),
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
                self.expr(a, walk, acc)?;
                self.expr(b, walk, acc)
            }
            Expr::MemberOf(member) => {
                self.expr(&member.value, walk, acc)?;
                self.expr(&member.array, walk, acc)
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
                self.expr(expr, walk, acc)?;
                self.expr(pattern, walk, acc)?;
                self.opt(escape_char.as_deref(), walk, acc)
            }
            Expr::InList { expr, list, .. } => {
                self.expr(expr, walk, acc)?;
                self.all(list, walk, acc)
            }
            Expr::InSubquery { expr, subquery, .. } => {
                self.expr(expr, walk, acc)?;
                self.subquery(subquery, walk, acc)
            }
            Expr::Between { expr, low, high, .. } => {
                self.expr(expr, walk, acc)?;
                self.expr(low, walk, acc)?;
                self.expr(high, walk, acc)
            }
            Expr::Convert { expr, styles, .. } => {
                self.expr(expr, walk, acc)?;
                self.all(styles, walk, acc)
            }
            Expr::Substring {
                expr,
                substring_from,
                substring_for,
                ..
            } => {
                self.expr(expr, walk, acc)?;
                self.opt(substring_from.as_deref(), walk, acc)?;
                self.opt(substring_for.as_deref(), walk, acc)
            }
            Expr::Trim {
                expr,
                trim_what,
                trim_characters,
                ..
            } => {
                self.expr(expr, walk, acc)?;
                self.opt(trim_what.as_deref(), walk, acc)?;
                self.all(trim_characters.as_deref().unwrap_or_default(), walk, acc)
            }
            Expr::Overlay {
                expr,
                overlay_what,
                overlay_from,
                overlay_for,
            } => {
                self.expr(expr, walk, acc)?;
                self.expr(overlay_what, walk, acc)?;
                self.expr(overlay_from, walk, acc)?;
                self.opt(overlay_for.as_deref(), walk, acc)
            }
            Expr::Value(_) | Expr::TypedString(_) => Ok(()),
            Expr::Function(function) => self.function(function, walk, acc),
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => {
                self.opt(operand.as_deref(), walk, acc)?;
                for when in conditions {
                    self.expr(&when.condition, walk, acc)?;
                    self.expr(&when.result, walk, acc)?;
                }
                self.opt(else_result.as_deref(), walk, acc)
            }
            Expr::Exists { subquery, .. } | Expr::Subquery(subquery) => self.subquery(subquery, walk, acc),
            Expr::GroupingSets(sets) | Expr::Cube(sets) | Expr::Rollup(sets) => {
                for set in sets {
                    self.all(set, walk, acc)?;
                }
                Ok(())
            }
            Expr::Tuple(items) => self.all(items, walk, acc),
            Expr::Array(array) => self.all(&array.elem, walk, acc),
            Expr::Interval(interval) => self.expr(&interval.value, walk, acc),
            Expr::MatchAgainst { columns, .. } => {
                for name in columns {
                    let parts = idents(name)?;
                    match parts.split_last() {
                        Some((name, qualifier)) => self.column(qualifier, name, walk, acc)?,
                        None => return Err(unsupported("empty identifier")),
                    }
                }
                Ok(())
            }
            Expr::CompoundFieldAccess { root, access_chain } => {
                self.expr(root, walk, acc)?;
                for access in access_chain {
                    match access {
                        AccessExpr::Dot(_) => return Err(unsupported("composite field access")),
                        AccessExpr::Subscript(Subscript::Index { index }) => self.expr(index, walk, acc)?,
                        AccessExpr::Subscript(Subscript::Slice {
                            lower_bound,
                            upper_bound,
                            stride,
                        }) => {
                            self.opt(lower_bound.as_ref(), walk, acc)?;
                            self.opt(upper_bound.as_ref(), walk, acc)?;
                            self.opt(stride.as_ref(), walk, acc)?;
                        }
                    }
                }
                Ok(())
            }
            Expr::Wildcard(_) | Expr::QualifiedWildcard(..) => Err(unsupported("wildcard expression")),
            Expr::JsonAccess { .. } => Err(unsupported("JSON path access")),
            Expr::InUnnest { .. } => Err(unsupported("IN UNNEST")),
            Expr::Struct { .. } | Expr::Named { .. } | Expr::Dictionary(_) | Expr::Map(_) => {
                Err(unsupported("struct or map literal"))
            }
            Expr::OuterJoin(_) => Err(unsupported("(+) outer join")),
            Expr::Prior(_) => Err(unsupported("PRIOR")),
            Expr::Lambda(_) => Err(unsupported("lambda")),
        }
    }

    fn all(&mut self, exprs: &[Expr], walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        for expr in exprs {
            self.expr(expr, walk, acc)?;
        }
        Ok(())
    }

    fn opt(&mut self, expr: Option<&Expr>, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        expr.map_or(Ok(()), |e| self.expr(e, walk, acc))
    }

    /// Binds a column reference, falling back to output aliases where allowed.
    fn column(
        &mut self,
        qualifier: &[Ident],
        name: &Ident,
        walk: Walk<'_>,
        acc: &mut Sources,
    ) -> Result<(), LineageError> {
        let found = walk.scope.column(self.dialect, qualifier, name);
        let alias = match walk.aliases {
            Some(aliases) if qualifier.is_empty() => {
                let mut hits = aliases
                    .iter()
                    .filter(|c| self.dialect.matches(name, &c.name))
                    .peekable();
                hits.peek()
                    .is_some()
                    .then(|| hits.flat_map(|c| c.sources.iter().cloned()).collect::<Sources>())
            }
            _ => None,
        };
        let result = match (found, alias) {
            (Ok(mut sources), Some(alias)) => {
                sources.extend(alias);
                Ok(sources)
            }
            (Ok(sources), None) => Ok(sources),
            (Err(err), Some(alias)) if err.is_soft() => Ok(alias),
            (Err(err), _) => Err(err),
        };
        acc.extend(self.soften(result)?);
        Ok(())
    }

    /// Adds every output source of a subquery.
    fn subquery(&mut self, query: &Query, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        let cols = self.query(query, Some(walk.scope), walk.ctes)?;
        acc.extend(cols.into_iter().flat_map(|c| c.sources));
        Ok(())
    }

    /// Records a function call and adds its argument sources.
    fn function(&mut self, function: &Function, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
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
        self.functions.insert(FunctionUse {
            name: self.function_name(name)?,
            known: false,
            origin: Origin::Query,
        });
        match args {
            FunctionArguments::None => {}
            FunctionArguments::Subquery(query) => self.subquery(query, walk, acc)?,
            FunctionArguments::List(list) => {
                for arg in &list.args {
                    match arg {
                        FunctionArg::Named { arg, .. } | FunctionArg::Unnamed(arg) => self.arg(arg, walk, acc)?,
                        FunctionArg::ExprNamed { name, arg, .. } => {
                            self.expr(name, walk, acc)?;
                            self.arg(arg, walk, acc)?;
                        }
                    }
                }
                for clause in &list.clauses {
                    match clause {
                        FunctionArgumentClause::IgnoreOrRespectNulls(_)
                        | FunctionArgumentClause::Separator(_)
                        | FunctionArgumentClause::JsonNullClause(_)
                        | FunctionArgumentClause::JsonReturningClause(_) => {}
                        FunctionArgumentClause::Where(expr) | FunctionArgumentClause::Limit(expr) => {
                            self.expr(expr, walk, acc)?;
                        }
                        FunctionArgumentClause::OrderBy(items) => {
                            for item in items {
                                self.expr(&item.expr, walk, acc)?;
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
            self.expr(&item.expr, walk, acc)?;
        }
        self.opt(filter.as_deref(), walk, acc)?;
        match over {
            None | Some(WindowType::NamedWindow(_)) => Ok(()),
            Some(WindowType::WindowSpec(spec)) => self.window(spec, walk),
        }
    }

    /// Adds the sources of one function argument.
    fn arg(&mut self, arg: &FunctionArgExpr, walk: Walk<'_>, acc: &mut Sources) -> Result<(), LineageError> {
        match arg {
            FunctionArgExpr::Expr(expr) => self.expr(expr, walk, acc),
            FunctionArgExpr::Wildcard => Ok(()),
            FunctionArgExpr::QualifiedWildcard(_) => Err(unsupported("whole-row reference")),
            FunctionArgExpr::WildcardWithOptions(_) => Err(unsupported("wildcard modifier in function argument")),
        }
    }

    /// Records a window specification's columns as [`Role::Window`] references.
    pub(crate) fn window(&mut self, spec: &WindowSpec, walk: Walk<'_>) -> Result<(), LineageError> {
        let mut sources = Sources::new();
        self.all(&spec.partition_by, walk, &mut sources)?;
        for item in &spec.order_by {
            self.expr(&item.expr, walk, &mut sources)?;
        }
        if let Some(frame) = &spec.window_frame {
            for bound in std::iter::once(&frame.start_bound).chain(frame.end_bound.as_ref()) {
                match bound {
                    WindowFrameBound::CurrentRow => {}
                    WindowFrameBound::Preceding(e) | WindowFrameBound::Following(e) => {
                        self.opt(e.as_deref(), walk, &mut sources)?;
                    }
                }
            }
        }
        self.refer(sources, Role::Window);
        Ok(())
    }

    /// Returns a function's dotted name, case-folded per dialect.
    fn function_name(&self, name: &ObjectName) -> Result<String, LineageError> {
        Ok(idents(name)?
            .iter()
            .map(|p| self.dialect.function_name(p))
            .collect::<Vec<_>>()
            .join("."))
    }
}
