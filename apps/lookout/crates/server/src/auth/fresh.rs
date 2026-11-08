use std::future::Future;
use std::time::Duration;

use backon::{ExponentialBuilder, Retryable};

pub const FIRST_WAIT: Duration = Duration::from_secs(1);
pub const MOST_WAITING: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Claimed,
    Taken,
}

#[derive(Debug, thiserror::Error)]
pub enum ClaimError<E> {
    #[error("every fresh ID drawn was taken, after waiting up to {0:?} in all")]
    NoneFree(Duration),
    #[error(transparent)]
    Store(E),
}

pub async fn claim_fresh<Id, E, Fut>(
    draw: impl Fn() -> Id,
    try_claim: impl Fn(Id) -> Fut,
) -> Result<Id, ClaimError<E>>
where
    Id: Clone,
    Fut: Future<Output = Result<Claim, E>>,
{
    let attempt = || async {
        let id = draw();
        match try_claim(id.clone()).await {
            Ok(Claim::Claimed) => Ok(id),
            Ok(Claim::Taken) => Err(ClaimError::NoneFree(MOST_WAITING)),
            Err(err) => Err(ClaimError::Store(err)),
        }
    };
    attempt
        .retry(backoff())
        .when(|err| matches!(err, ClaimError::NoneFree(_)))
        .await
}

fn backoff() -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(FIRST_WAIT)
        .without_max_delay()
        .without_max_times()
        .with_total_delay(Some(MOST_WAITING))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use tokio::time::Instant;

    use super::*;

    #[derive(Debug, PartialEq, Eq)]
    struct StoreDown;

    fn counter() -> AtomicU32 {
        AtomicU32::new(0)
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_free_id_is_claimed() {
        let drawn = counter();
        let taken_until = 3;

        let claimed = claim_fresh(
            || drawn.fetch_add(1, Ordering::SeqCst) + 1,
            |id| async move {
                Ok::<_, StoreDown>(if id < taken_until {
                    Claim::Taken
                } else {
                    Claim::Claimed
                })
            },
        )
        .await;

        assert!(matches!(claimed, Ok(3)), "{claimed:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn drawing_gives_up_once_the_waiting_would_pass_its_limit() {
        let started = Instant::now();

        let claimed = claim_fresh(|| (), |()| async { Ok::<_, StoreDown>(Claim::Taken) }).await;

        assert!(
            matches!(claimed, Err(ClaimError::NoneFree(_))),
            "{claimed:?}"
        );
        assert!(started.elapsed() <= MOST_WAITING, "{:?}", started.elapsed());
        assert!(started.elapsed() >= FIRST_WAIT, "{:?}", started.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_store_failure_ends_the_drawing_at_once() {
        let tries = counter();

        let claimed = claim_fresh(
            || (),
            |()| {
                tries.fetch_add(1, Ordering::SeqCst);
                async { Err::<Claim, _>(StoreDown) }
            },
        )
        .await;

        assert!(
            matches!(claimed, Err(ClaimError::Store(StoreDown))),
            "{claimed:?}"
        );
        assert_eq!(tries.load(Ordering::SeqCst), 1);
    }
}
