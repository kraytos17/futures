pub mod fut_test;

use log::{debug, error};
use std::{fmt::Debug, mem};

#[derive(Debug)]
pub enum FutError {
    SleepingUnsupported,
    PolledAfterCompletion,
    CompletedWithoutValue,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum FutState {
    Pending,
    Done,
    Waiting,
}

#[derive(Debug)]
pub struct FutResult<T> {
    pub state: FutState,
    pub value: Option<T>,
}

impl<T: Debug> FutResult<T> {
    pub fn pending() -> Self {
        debug!("Creating pending FutResult");
        Self {
            state: FutState::Pending,
            value: None,
        }
    }

    pub fn finished(val: T) -> Self {
        debug!("Creating finished FutResult with value {:?}", val);
        Self {
            state: FutState::Done,
            value: Some(val),
        }
    }
}

pub trait Future {
    type Output;
    type Error;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error>;
    fn cleanup(&mut self);
}

#[derive(Debug, Clone)]
pub struct Done<T> {
    res: Option<T>,
}

impl<T: Debug> Done<T> {
    pub fn new(val: T) -> Self {
        debug!("Creating new Done future with value {:?}", val);
        Self { res: Some(val) }
    }
}

impl<T: Clone + Debug> Future for Done<T> {
    type Output = T;
    type Error = FutError;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        debug!("Polling Done future");

        let value = self.res.take().ok_or(FutError::PolledAfterCompletion)?;
        debug!("Done future poll result: {:?}", value);

        Ok(FutResult::finished(value))
    }

    fn cleanup(&mut self) {
        debug!("Destroying Done future");
    }
}

#[derive(Debug, Clone)]
pub struct Failed<T> {
    err: Option<T>,
}

impl<T: Debug> Failed<T> {
    pub fn _new(err: T) -> Self {
        debug!("Creating new Reject future with err {:?}", err);
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

#[derive(Debug, Clone)]
enum ChainState<F1, F2, Fn>
where
    F1: Future,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    First { future: F1, transform: Fn },
    Second(F2),
    Done,
}

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
    pub fn new(future: F1, transform: Fn) -> Self {
        debug!("Creating new Chain future having future {:?}", future);
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
                        debug!("First future completed with value {:?}", value);
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
                        error!("Second future poll resulted in error {:?}", e);
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
