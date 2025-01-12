use log::{debug, error};
use std::{fmt::Debug, mem};

#[derive(Debug)]
pub enum FutError {
    SleepingUnsupported,
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
        println!("Creating pending FutResult");
        Self {
            state: FutState::Pending,
            value: None,
        }
    }

    pub fn finished(val: T) -> Self {
        println!("Creating finished FutResult with value {:?}", val);
        Self {
            state: FutState::Done,
            value: Some(val),
        }
    }

    pub fn waiting(val: T) -> Self {
        println!("Creating waiting FutResult with value {:?}", val);
        Self {
            state: FutState::Waiting,
            value: Some(val),
        }
    }
}

pub trait Future {
    type Output;
    type Error;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error>;
    fn destroy(&mut self);
}

#[derive(Debug, Clone)]
pub struct Done<T> {
    res: Option<T>,
}

impl<T: Debug> Done<T> {
    pub fn new(val: T) -> Self {
        println!("Creating new Done future with value {:?}", val);
        Self { res: Some(val) }
    }
}

impl<T: Clone + Debug> Future for Done<T> {
    type Output = T;
    type Error = FutError;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        println!("Polling Done future");
        let result = Ok(FutResult::finished(
            self.res.take().expect("Polling completed"),
        ));

        println!(
            "Done future poll result: {:?}",
            result.as_ref().map(|r| &r.state)
        );

        result
    }

    fn destroy(&mut self) {
        println!("Destroying Done future");
    }
}

#[derive(Debug, Clone)]
pub struct Reject<T> {
    err: Option<T>,
}

impl<T: Debug> Reject<T> {
    pub fn new(err: T) -> Self {
        println!("Creating new Reject future with err {:?}", err);
        Self { err: Some(err) }
    }
}

impl<T: Clone> Future for Reject<T> {
    type Output = ();
    type Error = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        println!("Polling Reject future");
        let result = Err(self.err.take().expect("Reject polled"));
        println!("Reject future poll resulted in error");

        result
    }

    fn destroy(&mut self) {
        println!("Destroying Reject future");
    }
}

#[derive(Debug, Clone)]
enum ThenState<F1, F2, Fn>
where
    F1: Future,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    First { future: F1, then_fn: Fn },
    Second(F2),
    Done,
}

#[derive(Debug, Clone)]
pub struct Then<F1, F2, Fn>
where
    F1: Future,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    state: ThenState<F1, F2, Fn>,
}

impl<F1, F2, Fn> Then<F1, F2, Fn>
where
    F1: Future + Debug,
    F2: Future,
    Fn: FnOnce(F1::Output) -> F2,
{
    pub fn new(future: F1, then_fn: Fn) -> Self {
        debug!("Creating new Then future having future {:?}", future);
        Self {
            state: ThenState::First { future, then_fn },
        }
    }
}

impl<F1, F2, Fn> Future for Then<F1, F2, Fn>
where
    F1: Future,
    F2: Future<Error = F1::Error>,
    F1::Error: std::fmt::Debug,
    F2::Output: Debug,
    F1::Output: Debug,
    Fn: FnOnce(F1::Output) -> F2 + Clone,
{
    type Output = F2::Output;
    type Error = F1::Error;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        debug!("Polling Then future");
        let result = match mem::replace(&mut self.state, ThenState::Done) {
            ThenState::First {
                mut future,
                then_fn,
            } => {
                debug!("Then future in First state");
                match future.poll()? {
                    FutResult {
                        state: FutState::Done,
                        value: Some(value),
                    } => {
                        debug!("First future completed with value {:?}", value);
                        self.state = ThenState::Second(then_fn(value));
                        Ok(FutResult::pending())
                    }
                    FutResult {
                        state: FutState::Pending,
                        ..
                    } => {
                        debug!("First future still pending");
                        self.state = ThenState::First { future, then_fn };
                        Ok(FutResult::pending())
                    }
                    FutResult {
                        state: FutState::Waiting,
                        ..
                    } => {
                        debug!("First future waiting");
                        self.state = ThenState::First { future, then_fn };
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
                        panic!("First future polled without any value")
                    }
                }
            }
            ThenState::Second(mut future) => {
                debug!("Then future in Second state");
                match future.poll() {
                    Ok(res) => {
                        debug!("Second future poll result state: {:?}", res.state);
                        if res.state != FutState::Done {
                            self.state = ThenState::Second(future);
                        }
                        Ok(res)
                    }
                    Err(e) => {
                        error!("Second future poll resulted in error {:?}", e);
                        self.state = ThenState::Second(future);
                        Err(e)
                    }
                }
            }
            ThenState::Done => {
                error!("ERROR: Then future polled after completion!");
                panic!("ThenFut polled after completion")
            }
        };

        debug!(
            "Then future poll complete with result: {:?}",
            result.as_ref().map(|r| &r.state)
        );

        result
    }

    fn destroy(&mut self) {
        debug!("Destroying Then future");
        match self.state {
            ThenState::First { ref mut future, .. } => {
                debug!("Destroying First state future");
                future.destroy();
            }
            ThenState::Second(ref mut future) => {
                debug!("Destroying Second state future");
                future.destroy();
            }
            ThenState::Done => {
                debug!("Destroying Done state");
            }
        }
    }
}
