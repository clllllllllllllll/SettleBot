use futures_util::future::{select, Either};
use std::future::Future;

pub(crate) async fn with_deadline<T>(
    operation: impl Future<Output = T>,
    deadline: impl Future<Output = ()>,
) -> std::result::Result<T, ()> {
    let operation = Box::pin(operation);
    let deadline = Box::pin(deadline);
    match select(operation, deadline).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right(_) => Err(()),
    }
}
