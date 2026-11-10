use super::{PersistenceError, PersistenceResult};

// D1 Free: 50 calls/invocation; batch() counts as one. Verified 2026-11-10.
// https://developers.cloudflare.com/d1/platform/limits/#batch-limits
pub(crate) const D1_FREE_QUERY_LIMIT: usize = 50;
pub(crate) const REPORT_QUERY_HEADROOM: usize = 10;
pub(crate) const REPORT_ADMISSION_QUERIES: usize = 2;
const REPORT_INITIAL_QUERIES: usize = 1;
pub(crate) const MAX_CONTRIBUTION_QUERIES: usize =
    D1_FREE_QUERY_LIMIT - REPORT_QUERY_HEADROOM - REPORT_ADMISSION_QUERIES - REPORT_INITIAL_QUERIES;
pub(crate) const REPORT_CONTRIBUTION_BATCH_SIZE: usize = 100;

#[allow(async_fn_in_trait)]
pub(crate) trait BatchSource {
    type Cursor: Default;
    type Row;
    async fn next(&self, cursor: &Self::Cursor) -> PersistenceResult<Vec<Self::Row>>;
}

pub(crate) async fn scan<S: BatchSource, A>(
    source: &S,
    mut aggregate: A,
    apply: impl Fn(&mut A, &mut S::Cursor, Vec<S::Row>) -> PersistenceResult<()>,
) -> PersistenceResult<A> {
    let mut cursor = S::Cursor::default();
    for _ in 0..MAX_CONTRIBUTION_QUERIES {
        let rows = source.next(&cursor).await?;
        let complete = rows.len() < REPORT_CONTRIBUTION_BATCH_SIZE;
        apply(&mut aggregate, &mut cursor, rows)?;
        if complete {
            return Ok(aggregate);
        }
    }
    Err(PersistenceError::ReportCapacity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct CountingSource {
        rows: usize,
        calls: Cell<usize>,
    }
    impl BatchSource for CountingSource {
        type Cursor = usize;
        type Row = ();
        async fn next(&self, cursor: &usize) -> PersistenceResult<Vec<()>> {
            self.calls.set(self.calls.get() + 1);
            assert!(
                self.calls.get() + REPORT_INITIAL_QUERIES + REPORT_ADMISSION_QUERIES
                    <= D1_FREE_QUERY_LIMIT - REPORT_QUERY_HEADROOM
            );
            Ok(vec![
                ();
                self.rows
                    .saturating_sub(*cursor)
                    .min(REPORT_CONTRIBUTION_BATCH_SIZE)
            ])
        }
    }

    #[test]
    fn iteration_stops_before_the_ceiling_and_requires_a_completion_query() {
        let maximum = MAX_CONTRIBUTION_QUERIES * REPORT_CONTRIBUTION_BATCH_SIZE - 1;
        for rows in [0, 100, maximum - 1, maximum, maximum + 1, maximum + 2] {
            let source = CountingSource {
                rows,
                calls: Cell::new(0),
            };
            let result = run(scan(&source, 0, |total, cursor, rows| {
                *total += rows.len();
                *cursor += rows.len();
                Ok(())
            }));
            if rows <= maximum {
                assert_eq!(result, Ok(rows));
            } else {
                assert_eq!(result, Err(PersistenceError::ReportCapacity));
            }
            assert_eq!(
                source.calls.get(),
                (rows / REPORT_CONTRIBUTION_BATCH_SIZE + 1).min(MAX_CONTRIBUTION_QUERIES)
            );
        }
    }

    fn run<F: std::future::Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("counting source must be ready"),
        }
    }
}
