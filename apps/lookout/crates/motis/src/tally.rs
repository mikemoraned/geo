use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tally {
    pub requests: usize,
    pub took: Duration,
}

impl Tally {
    pub async fn time<T>(&mut self, request: impl Future<Output = T>) -> T {
        let started = Instant::now();
        let response = request.await;
        self.requests += 1;
        self.took += started.elapsed();
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn each_request_adds_one_and_its_time() {
        let mut tally = Tally::default();
        for _ in 0..2 {
            tally
                .time(tokio::time::sleep(Duration::from_millis(10)))
                .await;
        }

        assert_eq!(
            tally,
            Tally {
                requests: 2,
                took: Duration::from_millis(20),
            }
        );
    }
}
