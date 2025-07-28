//! A minimalistic and custom future-like implementation.
//!
//! This module provides types and traits to simulate asynchronous computation behavior,
//! including chaining of futures, completion states, and result wrapping.

pub mod fut_test;

use log::{debug, error};
use std::{fmt::Debug, mem};

/// Represents errors that may occur during future execution.
#[derive(Debug)]
pub enum FutError {
    /// The future attempted to sleep or block in an unsupported way.
    SleepingUnsupported,

    /// The future was polled after it had already completed.
    PolledAfterCompletion,

    /// The future completed without returning a value.
    CompletedWithoutValue,
}

/// Represents the state of a future.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FutState {
    /// The future is still pending execution.
    Pending,

    /// The future has completed.
    Done,

    /// The future is temporarily waiting (e.g., on I/O).
    Waiting,
}

/// Encapsulates the result of polling a future.
#[derive(Debug)]
pub struct FutResult<T> {
    /// The current state of the future.
    pub state: FutState,

    /// The output value, if any.
    pub value: Option<T>,
}

impl<T: Debug> FutResult<T> {
    /// Creates a new `FutResult` in the pending state with no value.
    pub fn pending() -> Self {
        debug!("Creating pending FutResult");
        Self {
            state: FutState::Pending,
            value: None,
        }
    }

    /// Creates a new `FutResult` in the done state with the provided value.
    pub fn finished(val: T) -> Self {
        debug!("Creating finished FutResult with value {val:?}");
        Self {
            state: FutState::Done,
            value: Some(val),
        }
    }
}

/// Trait representing a custom future with poll and cleanup logic.
pub trait Future {
    /// The output type returned when the future completes successfully.
    type Output;

    /// The error type returned when polling fails.
    type Error;

    /// Polls the future to attempt to resolve it.
    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error>;

    /// Performs any necessary cleanup of resources.
    fn cleanup(&mut self);
}

/// A future that is immediately completed with a value.
#[derive(Debug, Clone)]
pub struct Done<T> {
    res: Option<T>,
}

impl<T: Debug> Done<T> {
    /// Creates a new `Done` future containing the given value.
    pub fn new(val: T) -> Self {
        debug!("Creating new Done future with value {val:?}");
        Self { res: Some(val) }
    }
}

impl<T: Clone + Debug> Future for Done<T> {
    type Output = T;
    type Error = FutError;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        debug!("Polling Done future");

        let value = self.res.take().ok_or(FutError::PolledAfterCompletion)?;
        debug!("Done future poll result: {value:?}");

        Ok(FutResult::finished(value))
    }

    fn cleanup(&mut self) {
        debug!("Destroying Done future");
    }
}

/// A future that represents a failed computation.
#[derive(Debug, Clone)]
pub struct Failed<T> {
    err: Option<T>,
}

impl<T: Debug> Failed<T> {
    /// Creates a new `Failed` future that will return the given error when polled.
    pub fn _new(err: T) -> Self {
        debug!("Creating new Reject future with err {err:?}");
        Self { err: Some(err) }
    }
}

impl<T: Clone> Future for Failed<T> {
    type Output = ();
    type Error = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        debug!("Polling Reject future");

        let result = Err(self.err.take().expect("Reject polled"));
        error!("Reject future poll resulted in error");

        result
    }

    fn cleanup(&mut self) {
        println!("Destroying Reject future");
    }
}

/// Internal state used by the `Chain` future to track progress.
#[derive(Debug, Clone)]
enum ChainState<F1, F2, Fn>
where
    F1: Future,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    /// The first future is still being polled.
    First { future: F1, transform: Fn },

    /// The second future is active.
    Second(F2),

    /// Both futures have completed.
    Done,
}

/// A future that chains two futures: the second is created from the first's output.
#[derive(Debug, Clone)]
pub struct Chain<F1, F2, Fn>
where
    F1: Future,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    state: ChainState<F1, F2, Fn>,
}

impl<F1, F2, Fn> Chain<F1, F2, Fn>
where
    F1: Future + Debug,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    /// Creates a new chained future from a base future and a transformation function.
    pub fn new(future: F1, transform: Fn) -> Self {
        debug!("Creating new Chain future having future {future:?}");
        Self {
            state: ChainState::First { future, transform },
        }
    }
}

impl<F1, F2, Fn> Future for Chain<F1, F2, Fn>
where
    F1: Future,
    F2: Future<Error = F1::Error>,
    F1::Error: std::fmt::Debug + From<FutError>,
    F2::Output: Debug,
    F1::Output: Debug,
    Fn: FnOnce(F1::Output) -> F2 + Clone,
{
    type Output = F2::Output;
    type Error = F1::Error;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        debug!("Polling Chain future");
        let result = match mem::replace(&mut self.state, ChainState::Done) {
            ChainState::First {
                mut future,
                transform: then_fn,
            } => {
                debug!("Then future in First state");
                match future.poll()? {
                    FutResult {
                        state: FutState::Done,
                        value: Some(value),
                    } => {
                        debug!("First future completed with value {value:?}");
                        self.state = ChainState::Second(then_fn(value));
                        Ok(FutResult::pending())
                    }
                    FutResult {
                        state: FutState::Pending,
                        ..
                    } => {
                        debug!("First future still pending");
                        self.state = ChainState::First {
                            future,
                            transform: then_fn,
                        };
                        Ok(FutResult::pending())
                    }
                    FutResult {
                        state: FutState::Waiting,
                        ..
                    } => {
                        debug!("First future waiting");
                        self.state = ChainState::First {
                            future,
                            transform: then_fn,
                        };
                        Ok(FutResult {
                            state: FutState::Waiting,
                            value: None,
                        })
                    }
                    FutResult {
                        state: FutState::Done,
                        value: None,
                    } => {
                        error!("ERROR: First future completed without value!");
                        Err(FutError::CompletedWithoutValue.into())
                    }
                }
            }
            ChainState::Second(mut future) => {
                debug!("Then future in Second state");
                match future.poll() {
                    Ok(res) => {
                        debug!("Second future poll result state: {:?}", res.state);
                        if res.state != FutState::Done {
                            self.state = ChainState::Second(future);
                        }
                        Ok(res)
                    }
                    Err(e) => {
                        error!("Second future poll resulted in error {e:?}");
                        self.state = ChainState::Second(future);
                        Err(e)
                    }
                }
            }
            ChainState::Done => {
                error!("ERROR: Then future polled after completion!");
                Err(FutError::PolledAfterCompletion.into())
            }
        };

        debug!(
            "Then future poll complete with result: {:?}",
            result.as_ref().map(|r| &r.state)
        );

        result
    }

    fn cleanup(&mut self) {
        debug!("Destroying Then future");
        match self.state {
            ChainState::First { ref mut future, .. } => {
                debug!("Destroying First state future");
                future.cleanup();
            }
            ChainState::Second(ref mut future) => {
                debug!("Destroying Second state future");
                future.cleanup();
            }
            ChainState::Done => {
                debug!("Destroying Done state");
            }
        }
    }
}
