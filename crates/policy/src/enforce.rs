//! The fail-closed envelope every backend runs a planned statement in.
//!
//! Only [`admit`] builds an [`Admitted`] and only [`settle`] a [`Run`], so
//! no backend skips the ordering:
//!
//! 1. no redactor, or a write in read-only mode: nothing runs,
//! 2. a refusal ([`Stop::Rejected`], [`Stop::Unfit`], [`Stop::Refused`])
//!    rolls the transaction back,
//! 3. a database error, at `COMMIT` too, is withheld when it may echo a
//!    value and the plan withholds,
//! 4. a read rolls back, a write commits,
//! 5. the kept rows are redacted by the plan.

use std::mem;

use dbmcp_privacy::Redactor;
use dbmcp_sql::SqlError;
use dbmcp_truncate::Window;
use serde_json::Value;
use sqlx::{Database, Transaction};

use crate::{Planned, Rejection, Unfit};

/// Whether the statement may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Runs in a read-only transaction and rolls back.
    Read,
    /// Commits; refused in read-only mode.
    Write,
}

/// Redacted rows and write counts of a statement run under the column rules.
#[derive(Debug)]
pub struct Enforced {
    /// The kept rows, redacted, as JSON objects.
    pub rows: Vec<Value>,
    /// Rows the statement affected.
    pub rows_affected: u64,
    /// The last id the statement inserted, if the backend reports one.
    pub last_insert_id: Option<u64>,
}

/// Column names and write counts of an executed statement.
#[derive(Debug)]
pub struct Executed {
    /// The result column names; empty without rows.
    pub names: Vec<String>,
    /// Rows the statement affected.
    pub rows_affected: u64,
    /// The last id the statement inserted, if the backend reports one.
    pub last_insert_id: Option<u64>,
}

/// A settled statement: its plan and what it executed.
#[derive(Debug)]
pub struct Run<'c> {
    planned: Planned<'c>,
    executed: Executed,
}

/// Why a statement returned no rows under the column rules.
///
/// `E` holds the backend's own refusals.
#[derive(Debug, thiserror::Error)]
pub enum Stop<E> {
    /// The planner refused the statement; nothing ran.
    #[error(transparent)]
    Rejected(#[from] Rejection),
    /// The result does not fit the plan; nothing was committed.
    #[error(transparent)]
    Unfit(#[from] Unfit),
    /// The backend refused the statement; nothing was committed.
    #[error(transparent)]
    Refused(E),
    /// The statement failed with an error that may echo a ruled value.
    ///
    /// Holds the backend's label of the error code, such as `SQLSTATE 23505`.
    #[error("query failed ({0}): error detail withheld under column policy")]
    Withheld(String),
    /// Any other database error, reported as without column rules.
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
}

/// Ends `tx` after `executed`, the statement and the backend's rechecks.
///
/// A refusal rolls `tx` back. A database error, of the statement or of
/// ending `tx`, becomes [`Stop::Withheld`] when `echoing` labels it and the
/// plan withholds; deferred constraints fail at `COMMIT`. A read rolls
/// back, a write commits. `tx` is `None` for a statement that runs outside
/// a transaction block.
///
/// # Errors
///
/// Returns `executed`'s stop, withheld as above, or the error of ending `tx`.
pub async fn settle<'c, DB: Database, E>(
    tx: Option<Transaction<'_, DB>>,
    access: Access,
    planned: Planned<'c>,
    echoing: fn(&sqlx::Error) -> Option<String>,
    executed: Result<Executed, Stop<E>>,
) -> Result<Run<'c>, Stop<E>> {
    let withheld = |error: sqlx::Error| {
        planned
            .withheld(echoing(&error))
            .map_or(Stop::Sql(error), Stop::Withheld)
    };
    let executed = match executed {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(withheld(error)),
        Err(stop) => {
            if let Some(tx) = tx {
                tx.rollback().await?;
            }
            return Err(stop);
        }
    };
    if let Some(tx) = tx {
        let finished = match access {
            Access::Read => tx.rollback().await,
            Access::Write => tx.commit().await,
        };
        finished.map_err(withheld)?;
    }
    Ok(Run { planned, executed })
}

/// A statement the guards let run: rules can redact, and it may write.
#[derive(Debug)]
pub struct Admitted<'r> {
    redactor: &'r Redactor,
}

/// Lets a statement with `access` run under the column rules.
///
/// `read_only` is the connection's read-only mode.
///
/// # Errors
///
/// Returns [`SqlError::ColumnPolicy`] when there is no redactor, or
/// [`SqlError::ReadOnlyViolation`] for a write in read-only mode.
pub fn admit(redactor: Option<&Redactor>, access: Access, read_only: bool) -> Result<Admitted<'_>, SqlError> {
    let Some(redactor) = redactor else {
        return Err(SqlError::ColumnPolicy(
            "column policy: column rules need PII redaction enabled".into(),
        ));
    };
    if access == Access::Write && read_only {
        return Err(SqlError::ReadOnlyViolation);
    }
    Ok(Admitted { redactor })
}

impl<E> Stop<E> {
    /// Moves a [`Stop::Sql`] out of `result`, for `Connection::with_connection` to map.
    ///
    /// # Errors
    ///
    /// Returns the database error of a [`Stop::Sql`].
    pub fn split<T>(result: Result<T, Self>) -> Result<Result<T, Self>, sqlx::Error> {
        match result {
            Err(Self::Sql(error)) => Err(error),
            other => Ok(other),
        }
    }
}

impl Admitted<'_> {
    /// Redacts the rows `ran` kept in `window` by its plan.
    ///
    /// `ran` is the backend's `Connection::with_connection` result over
    /// [`Stop::split`]. The window holds each row as a JSON array of its
    /// values; the kept rows come back redacted, as JSON objects.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ColumnPolicy`] with the reason the column rules
    /// stopped the statement or the redactor's error, else the [`SqlError`]
    /// of anything else that failed.
    pub async fn finish<E: std::fmt::Display>(
        self,
        ran: Result<Result<Run<'_>, Stop<E>>, SqlError>,
        window: &mut Window,
    ) -> Result<Enforced, SqlError> {
        let Run { planned, executed } = ran?.map_err(|stop| SqlError::ColumnPolicy(stop.to_string()))?;
        let rows = mem::take(&mut window.items).into_iter().map(values).collect();
        let rows = planned
            .redact(self.redactor, executed.names, rows)
            .await
            .map_err(|error| SqlError::ColumnPolicy(error.to_string()))?;
        Ok(Enforced {
            rows,
            rows_affected: executed.rows_affected,
            last_insert_id: executed.last_insert_id,
        })
    }
}

/// Unwraps a window row, which backends always push as an array.
fn values(row: Value) -> Vec<Value> {
    match row {
        Value::Array(values) => values,
        other => vec![other],
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use dbmcp_truncate::Caps;
    use dbmcp_types::privacy;
    use serde_json::json;
    use sqlx::{Connection as _, SqliteConnection};

    use super::*;
    use crate::plan::Families;
    use crate::planned::Mode;

    type TestStop = Stop<Infallible>;

    fn planned(config: &privacy::Config, withholds: bool) -> Planned<'_> {
        Planned::new(Mode::Global, config, Families::new(), withholds)
    }

    fn executed() -> Executed {
        Executed {
            names: vec!["n".into()],
            rows_affected: 1,
            last_insert_id: Some(7),
        }
    }

    /// Labels every database error as one that may echo a value.
    #[expect(clippy::unnecessary_wraps, reason = "the signature settle takes")]
    fn echoing(_: &sqlx::Error) -> Option<String> {
        Some("code X".into())
    }

    /// An in-memory database with table `t` and a deferred foreign key from `c` to `p`.
    async fn memory() -> SqliteConnection {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await.expect("opens");
        sqlx::raw_sql(
            "PRAGMA foreign_keys = ON; CREATE TABLE t (a); CREATE TABLE p (id PRIMARY KEY); \
             CREATE TABLE c (p REFERENCES p DEFERRABLE INITIALLY DEFERRED)",
        )
        .execute(&mut conn)
        .await
        .expect("setup");
        conn
    }

    /// Opens a transaction on `conn` that inserts one row into `t`.
    async fn inserting(conn: &mut SqliteConnection) -> Transaction<'_, sqlx::Sqlite> {
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query("INSERT INTO t VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("insert");
        tx
    }

    async fn rows(conn: &mut SqliteConnection) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM t")
            .fetch_one(conn)
            .await
            .expect("counts")
    }

    fn redactor(config: &privacy::Config) -> Redactor {
        Redactor::from_config(Some(config))
            .expect("builds")
            .expect("configured")
    }

    #[tokio::test]
    async fn writes_commit_and_reads_roll_back() {
        let config = privacy::Config::default();
        let mut conn = memory().await;
        for (access, kept) in [(Access::Read, 0), (Access::Write, 1)] {
            let tx = inserting(&mut conn).await;
            let run = settle(
                Some(tx),
                access,
                planned(&config, true),
                echoing,
                Ok::<_, TestStop>(executed()),
            )
            .await
            .expect("settles");
            assert_eq!((run.executed.rows_affected, run.executed.last_insert_id), (1, Some(7)));
            assert_eq!(rows(&mut conn).await, kept, "{access:?}");
        }
    }

    #[tokio::test]
    async fn refusals_roll_back_a_write() {
        let config = privacy::Config::default();
        let mut conn = memory().await;
        let tx = inserting(&mut conn).await;
        let unfit = Err(TestStop::Unfit(Unfit::Unplanned(0)));
        let stop = settle(Some(tx), Access::Write, planned(&config, true), echoing, unfit)
            .await
            .expect_err("refused");
        assert!(matches!(stop, Stop::Unfit(Unfit::Unplanned(0))), "{stop}");
        assert_eq!(rows(&mut conn).await, 0);
    }

    #[tokio::test]
    async fn database_errors_are_withheld_only_when_the_plan_withholds() {
        let config = privacy::Config::default();
        let mut conn = memory().await;
        for withholds in [true, false] {
            let tx = inserting(&mut conn).await;
            let failed = Err(TestStop::Sql(sqlx::Error::Protocol("secret".into())));
            let stop = settle(Some(tx), Access::Write, planned(&config, withholds), echoing, failed)
                .await
                .expect_err("fails");
            if withholds {
                assert_eq!(
                    stop.to_string(),
                    "query failed (code X): error detail withheld under column policy"
                );
            } else {
                assert!(matches!(stop, Stop::Sql(_)), "{stop}");
            }
            assert_eq!(rows(&mut conn).await, 0);
        }
    }

    #[tokio::test]
    async fn commit_errors_are_withheld() {
        let config = privacy::Config::default();
        let mut conn = memory().await;
        let mut tx = conn.begin().await.expect("begin");
        sqlx::query("INSERT INTO c VALUES (1)")
            .execute(&mut *tx)
            .await
            .expect("deferred");
        let stop = settle(
            Some(tx),
            Access::Write,
            planned(&config, true),
            echoing,
            Ok::<_, TestStop>(executed()),
        )
        .await
        .expect_err("COMMIT fails");
        assert!(matches!(stop, Stop::Withheld(ref label) if label == "code X"), "{stop}");
    }

    #[tokio::test]
    async fn statements_outside_a_transaction_still_withhold() {
        let config = privacy::Config::default();
        let failed = Err(TestStop::Sql(sqlx::Error::Protocol("secret".into())));
        let stop = settle::<sqlx::Sqlite, _>(None, Access::Write, planned(&config, true), echoing, failed)
            .await
            .expect_err("fails");
        assert!(matches!(stop, Stop::Withheld(_)), "{stop}");
    }

    #[test]
    fn admission_needs_a_redactor_and_a_writable_connection_for_writes() {
        let config = privacy::Config::default();
        let redactor = redactor(&config);
        let missing = admit(None, Access::Read, false).expect_err("no redactor");
        assert!(
            matches!(missing, SqlError::ColumnPolicy(ref message) if message.contains("column policy")),
            "{missing}"
        );
        let write = admit(Some(&redactor), Access::Write, true).expect_err("read-only");
        assert!(matches!(write, SqlError::ReadOnlyViolation), "{write}");
        assert!(admit(Some(&redactor), Access::Read, true).is_ok());
        assert!(admit(Some(&redactor), Access::Write, false).is_ok());
    }

    #[test]
    fn split_moves_only_database_errors_out() {
        assert!(TestStop::split(Err::<(), _>(TestStop::Sql(sqlx::Error::RowNotFound))).is_err());
        assert!(matches!(
            TestStop::split(Err::<(), _>(TestStop::Unfit(Unfit::Unplanned(0)))),
            Ok(Err(Stop::Unfit(_)))
        ));
        assert!(matches!(TestStop::split(Ok::<_, TestStop>(())), Ok(Ok(()))));
    }

    #[tokio::test]
    async fn finish_redacts_the_kept_rows_and_reports_stops() {
        let config = privacy::Config::default();
        let redactor = redactor(&config);
        let mut window = Window::new(0, None, Caps::default());
        window.push(json!(["plain"]));
        let run = Run {
            planned: planned(&config, true),
            executed: executed(),
        };
        let admitted = admit(Some(&redactor), Access::Read, false).expect("admitted");
        let enforced = admitted
            .finish(Ok(Ok::<_, TestStop>(run)), &mut window)
            .await
            .expect("redacts");
        assert_eq!(enforced.rows, [json!({ "n": "plain" })]);
        assert_eq!((enforced.rows_affected, enforced.last_insert_id), (1, Some(7)));
        assert!(window.items.is_empty());

        let admitted = admit(Some(&redactor), Access::Read, false).expect("admitted");
        let refused = Err(TestStop::Unfit(Unfit::Unplanned(3)));
        let error = admitted.finish(Ok(refused), &mut window).await.expect_err("stopped");
        assert_eq!(error.to_string(), Unfit::Unplanned(3).to_string());
    }
}
