//! Shared device-code polling loop (RFC 8628 semantics).
//!
//! The loop owns the interval, the `slow_down` back-off, the expiry deadline,
//! and cancellation. Providers supply only a single poll step describing what
//! their endpoint said.

use std::time::Duration;

use crate::interaction::CancelSignal;

/// Outcome of one poll of the device-authorization endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCodePoll<T> {
    /// Authorization completed; carries the provider's result.
    Complete(T),
    /// The user has not finished authorizing yet.
    Pending,
    /// The server asked the client to poll less often.
    SlowDown {
        /// A server-supplied replacement interval, when one was given.
        interval_seconds: Option<u64>,
    },
    /// Authorization cannot complete; carries the reason.
    Failed(DeviceCodeFailure),
}

/// Why a device-code authorization ended without a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceCodeFailure {
    /// The user declined the authorization.
    Denied,
    /// The code expired before the user finished.
    Expired,
    /// The attempt was cancelled from within jinn.
    Cancelled,
    /// The provider replied with something unusable.
    Provider(String),
}

/// Lower bound on the polling interval, so a misreported interval cannot turn
/// into a busy loop.
const MINIMUM_INTERVAL: Duration = Duration::from_secs(1);

/// RFC 8628 §3.5: each `slow_down` increases the interval by five seconds.
const SLOW_DOWN_INCREMENT: Duration = Duration::from_secs(5);

/// How the loop sleeps between polls. Tests substitute an instant sleeper so
/// they exercise the back-off arithmetic without real delays.
#[async_trait::async_trait]
pub trait PollSleeper: Send + Sync + std::fmt::Debug {
    /// Sleeps for `duration`, or returns early if `cancel` fires.
    async fn sleep(&self, duration: Duration, cancel: &CancelSignal);
}

/// Sleeps on the tokio timer.
#[derive(Debug, Default)]
pub struct TokioSleeper;

#[async_trait::async_trait]
impl PollSleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration, cancel: &CancelSignal) {
        tokio::select! {
            () = tokio::time::sleep(duration) => {}
            () = cancel.cancelled() => {}
        }
    }
}

/// Sleeps instantly, recording how long it was asked to wait.
#[derive(Debug, Default)]
pub struct InstantSleeper {
    waits: parking_lot::Mutex<Vec<Duration>>,
}

impl InstantSleeper {
    /// Creates a sleeper that never actually waits.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The durations this sleeper was asked to wait, in order.
    #[must_use]
    pub fn waits(&self) -> Vec<Duration> {
        self.waits.lock().clone()
    }
}

#[async_trait::async_trait]
impl PollSleeper for InstantSleeper {
    async fn sleep(&self, duration: Duration, _cancel: &CancelSignal) {
        self.waits.lock().push(duration);
    }
}

/// How long a device-code authorization may run and how often to poll.
#[derive(Debug, Clone, Copy)]
pub struct DeviceCodeSchedule {
    /// Initial interval between polls.
    pub interval: Duration,
    /// Total time the user has to authorize.
    pub expires_in: Duration,
}

/// Polls `poll` until the authorization completes, fails, expires, or is
/// cancelled.
///
/// # Errors
///
/// Returns the reason the authorization did not complete.
pub async fn poll_device_code<T, F, Fut>(
    schedule: DeviceCodeSchedule,
    cancel: &CancelSignal,
    sleeper: &dyn PollSleeper,
    mut poll: F,
) -> Result<T, DeviceCodeFailure>
where
    F: FnMut() -> Fut + Send,
    Fut: std::future::Future<Output = DeviceCodePoll<T>> + Send,
    T: Send,
{
    let mut interval = schedule.interval.max(MINIMUM_INTERVAL);
    let mut remaining = schedule.expires_in;
    let started = tokio::time::Instant::now();

    loop {
        if cancel.is_cancelled() {
            return Err(DeviceCodeFailure::Cancelled);
        }
        remaining = remaining.min(schedule.expires_in.saturating_sub(started.elapsed()));
        if remaining.is_zero() {
            return Err(DeviceCodeFailure::Expired);
        }

        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(DeviceCodeFailure::Cancelled),
            result = tokio::time::timeout(remaining, poll()) => result.map_err(|_elapsed| DeviceCodeFailure::Expired)?,
        };
        match outcome {
            DeviceCodePoll::Complete(value) => return Ok(value),
            DeviceCodePoll::Failed(failure) => return Err(failure),
            DeviceCodePoll::Pending => {}
            DeviceCodePoll::SlowDown { interval_seconds } => {
                interval = match interval_seconds {
                    Some(seconds) => Duration::from_secs(seconds).max(MINIMUM_INTERVAL),
                    None => interval.saturating_add(SLOW_DOWN_INCREMENT),
                };
            }
        }

        let wait = interval.min(remaining);
        sleeper.sleep(wait, cancel).await;
        remaining = remaining.saturating_sub(wait);

        if cancel.is_cancelled() {
            return Err(DeviceCodeFailure::Cancelled);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, reason = "test code")]
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn schedule() -> DeviceCodeSchedule {
        DeviceCodeSchedule {
            interval: Duration::from_secs(5),
            expires_in: Duration::from_mins(15),
        }
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_completed_authorization_returns_its_value() {
        // Given an endpoint that authorizes immediately.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();

        // When polling it.
        let result = poll_device_code(schedule(), &cancel, &sleeper, || async {
            DeviceCodePoll::Complete("authorization-code")
        })
        .await;

        // Then the provider's value is returned.
        assert_eq!(result, Ok("authorization-code"));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn pending_responses_are_retried_until_completion() {
        // Given an endpoint that is pending twice before authorizing.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();
        let attempts = AtomicUsize::new(0);

        // When polling it.
        let result = poll_device_code(schedule(), &cancel, &sleeper, || async {
            match attempts.fetch_add(1, Ordering::SeqCst) {
                0 | 1 => DeviceCodePoll::Pending,
                _ => DeviceCodePoll::Complete("authorization-code"),
            }
        })
        .await;

        // Then the loop keeps polling until the authorization completes.
        assert_eq!(result, Ok("authorization-code"));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_slow_down_response_lengthens_the_interval() {
        // Given an endpoint that asks the client to slow down once.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();
        let attempts = AtomicUsize::new(0);
        let _ = poll_device_code(schedule(), &cancel, &sleeper, || async {
            match attempts.fetch_add(1, Ordering::SeqCst) {
                0 => DeviceCodePoll::SlowDown {
                    interval_seconds: None,
                },
                _ => DeviceCodePoll::Complete(()),
            }
        })
        .await;

        // When inspecting how long the loop waited after the slow_down.
        // Then the wait grew by the RFC 8628 increment.
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(10)]);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_server_supplied_interval_replaces_the_client_back_off() {
        // Given an endpoint that reports its own required interval.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();
        let attempts = AtomicUsize::new(0);
        let _ = poll_device_code(schedule(), &cancel, &sleeper, || async {
            match attempts.fetch_add(1, Ordering::SeqCst) {
                0 => DeviceCodePoll::SlowDown {
                    interval_seconds: Some(30),
                },
                _ => DeviceCodePoll::Complete(()),
            }
        })
        .await;

        // When inspecting the wait after the slow_down.
        // Then the server's interval is used.
        assert_eq!(sleeper.waits(), vec![Duration::from_secs(30)]);
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn a_denied_authorization_reports_denial() {
        // Given an endpoint that reports the user declined.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();

        // When polling it.
        let result: Result<(), DeviceCodeFailure> =
            poll_device_code(schedule(), &cancel, &sleeper, || async {
                DeviceCodePoll::Failed(DeviceCodeFailure::Denied)
            })
            .await;

        // Then denial is reported.
        assert_eq!(result, Err(DeviceCodeFailure::Denied));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn polling_past_the_deadline_reports_expiry() {
        // Given an authorization window shorter than one poll interval.
        let cancel = CancelSignal::new();
        let sleeper = InstantSleeper::new();
        let schedule = DeviceCodeSchedule {
            interval: Duration::from_secs(5),
            expires_in: Duration::from_secs(5),
        };

        // When the endpoint stays pending.
        let result: Result<(), DeviceCodeFailure> =
            poll_device_code(schedule, &cancel, &sleeper, || async {
                DeviceCodePoll::Pending
            })
            .await;

        // Then the loop stops with an expiry.
        assert_eq!(result, Err(DeviceCodeFailure::Expired));
    }

    #[rstest::rstest]
    #[tokio::test]
    async fn cancelling_stops_the_polling_loop() {
        // Given an attempt cancelled before the first poll.
        let cancel = CancelSignal::new();
        cancel.cancel();
        let sleeper = InstantSleeper::new();

        // When polling.
        let result: Result<(), DeviceCodeFailure> =
            poll_device_code(schedule(), &cancel, &sleeper, || async {
                DeviceCodePoll::Complete(())
            })
            .await;

        // Then the loop reports cancellation without contacting the provider.
        assert_eq!(result, Err(DeviceCodeFailure::Cancelled));
    }
}
