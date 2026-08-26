//! Statement spans, in two shapes.
//!
//! With the `tracing` feature off every item here compiles away, so the call
//! sites in [crate::driver] carry no `cfg` of their own.

#[cfg(feature = "tracing")]
pub(crate) use enabled::*;

#[cfg(not(feature = "tracing"))]
pub(crate) use disabled::*;

/// What [statement] gets handed: the result of [crate::Query::build].
type Built = Result<(String, Vec<crate::Value>), crate::ParamCountError>;

#[cfg(feature = "tracing")]
mod enabled {
    use tracing::{Instrument as _, Span, field::Empty};

    use super::Built;
    use crate::dialect::Dialect;

    /// A span covering one statement, from build to the last row.
    ///
    /// Field names follow the OpenTelemetry database conventions. The SQL is
    /// recorded but the parameters are not: [crate::Trusted] and the
    /// fragment/param split keep user data out of the string, which is what
    /// makes the string safe to hand to a collector.
    pub(crate) fn statement<D: Dialect>(operation: &'static str, built: &Built) -> Span {
        tracing::debug_span!(
            "esql.statement",
            db.system.name = D::NAME,
            db.operation.name = operation,
            db.query.text = built.as_ref().map_or("", |(sql, _)| sql.as_str()),
            db.response.affected_rows = Empty,
            db.response.returned_rows = Empty,
        )
    }

    pub(crate) fn instrument<F: Future>(span: Span, future: F) -> impl Future<Output = F::Output> {
        future.instrument(span)
    }

    pub(crate) fn affected_rows(rows: u64) {
        Span::current().record("db.response.affected_rows", rows);
    }

    pub(crate) fn returned_rows(rows: usize) {
        Span::current().record("db.response.returned_rows", rows as u64);
    }
}

#[cfg(not(feature = "tracing"))]
mod disabled {
    use super::Built;
    use crate::dialect::Dialect;

    pub(crate) struct Span;

    #[expect(
        clippy::extra_unused_type_parameters,
        reason = "the dialect names the database in the enabled shape"
    )]
    pub(crate) fn statement<D: Dialect>(_: &'static str, _: &Built) -> Span {
        Span
    }

    pub(crate) fn instrument<F: Future>(_: Span, future: F) -> impl Future<Output = F::Output> {
        future
    }

    pub(crate) fn affected_rows(_: u64) {}

    pub(crate) fn returned_rows(_: usize) {}
}
