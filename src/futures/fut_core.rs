use std::fmt::Debug;

/// A simple error type for our custom futures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutError {
    PolledAfterCompletion,
}

/// Result of polling a custom future.
#[derive(Debug)]
pub enum FutResult<T> {
    Pending,
    Waiting,
    Done(T),
}

/// Core trait for our toy futures. Error type is fixed to `FutError`.
pub trait Fut {
    type Output;

    /// Poll the future. Return `Ok(FutResult::*)` on success or `Err(FutError)` on failure.
    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError>;

    /// Convenience combinator to chain futures.
    fn then<F2, FN>(self, f: FN) -> Chain<Self, F2, FN>
    where
        Self: Sized,
        FN: FnOnce(Self::Output) -> F2,
        F2: Fut,
    {
        Chain::new(self, f)
    }
}

/// A future that is immediately ready (returns value once).
#[derive(Debug)]
pub struct Done<T>(Option<T>);

impl<T> Done<T> {
    pub const fn new(val: T) -> Self {
        Self(Some(val))
    }
}

impl<T> Fut for Done<T> {
    type Output = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        match self.0.take() {
            Some(v) => Ok(FutResult::Done(v)),
            None => Err(FutError::PolledAfterCompletion),
        }
    }
}

/// A future that always fails with a `FutError`.
#[derive(Debug)]
pub struct Failed(Option<FutError>);

impl Failed {
    pub const fn new(err: FutError) -> Self {
        Self(Some(err))
    }
}

impl Fut for Failed {
    type Output = std::convert::Infallible;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        Err(self.0.take().expect("Failed polled after completion"))
    }
}

/// Internal state for Chain.
#[derive(Debug)]
enum ChainState<F1, F2, FN> {
    First(F1, Option<FN>),
    Second(F2),
    Done,
}

/// Chain combinator: run F1, when it yields a value build and run F2.
#[derive(Debug)]
pub struct Chain<F1, F2, FN> {
    state: Option<ChainState<F1, F2, FN>>,
}

impl<F1, F2, FN> Chain<F1, F2, FN>
where
    F1: Fut,
    F2: Fut,
    FN: FnOnce(F1::Output) -> F2,
{
    pub const fn new(f1: F1, f: FN) -> Self {
        Self {
            state: Some(ChainState::First(f1, Some(f))),
        }
    }
}

impl<F1, F2, FN> Fut for Chain<F1, F2, FN>
where
    F1: Fut,
    F2: Fut,
    FN: FnOnce(F1::Output) -> F2,
{
    type Output = F2::Output;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        match self.state.take() {
            Some(ChainState::First(mut f1, mut opt_f)) => match f1.poll()? {
                FutResult::Done(v) => {
                    let f2 = opt_f.take().expect("closure already used")(v);
                    self.state = Some(ChainState::Second(f2));
                    Ok(FutResult::Pending)
                }
                FutResult::Pending => {
                    self.state = Some(ChainState::First(f1, opt_f));
                    Ok(FutResult::Pending)
                }
                FutResult::Waiting => {
                    self.state = Some(ChainState::First(f1, opt_f));
                    Ok(FutResult::Waiting)
                }
            },
            Some(ChainState::Second(mut f2)) => match f2.poll() {
                Ok(FutResult::Done(v)) => {
                    self.state = Some(ChainState::Done);
                    Ok(FutResult::Done(v))
                }
                Ok(FutResult::Pending) => {
                    self.state = Some(ChainState::Second(f2));
                    Ok(FutResult::Pending)
                }
                Ok(FutResult::Waiting) => {
                    self.state = Some(ChainState::Second(f2));
                    Ok(FutResult::Waiting)
                }
                Err(e) => {
                    self.state = Some(ChainState::Second(f2));
                    Err(e)
                }
            },
            Some(ChainState::Done) | None => Err(FutError::PolledAfterCompletion),
        }
    }
}

/// Map combinator: transform the successful output value.
#[derive(Debug)]
pub struct Map<F, FN> {
    fut: F,
    f: Option<FN>,
}

impl<F, FN, T, U> Map<F, FN>
where
    F: Fut<Output = T>,
    FN: FnOnce(T) -> U,
{
    pub const fn new(fut: F, f: FN) -> Self {
        Self { fut, f: Some(f) }
    }
}

impl<F, FN, T, U> Fut for Map<F, FN>
where
    F: Fut<Output = T>,
    FN: FnOnce(T) -> U,
{
    type Output = U;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        match self.fut.poll()? {
            FutResult::Done(v) => {
                let f = self.f.take().expect("map closure already used");
                Ok(FutResult::Done(f(v)))
            }
            FutResult::Pending => Ok(FutResult::Pending),
            FutResult::Waiting => Ok(FutResult::Waiting),
        }
    }
}

/// Join combinator: poll two futures and return a tuple when both are Done.
#[derive(Debug)]
pub struct Join<F1, F2> {
    f1: F1,
    f2: F2,
}

impl<F1, F2> Join<F1, F2> {
    pub const fn new(f1: F1, f2: F2) -> Self {
        Self { f1, f2 }
    }
}

impl<F1, F2, T1, T2> Fut for Join<F1, F2>
where
    F1: Fut<Output = T1>,
    F2: Fut<Output = T2>,
{
    type Output = (T1, T2);

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        let r1 = self.f1.poll()?;
        let r2 = self.f2.poll()?;

        match (r1, r2) {
            (FutResult::Done(v1), FutResult::Done(v2)) => Ok(FutResult::Done((v1, v2))),
            (FutResult::Waiting, _) | (_, FutResult::Waiting) => Ok(FutResult::Waiting),
            _ => Ok(FutResult::Pending),
        }
    }
}

#[derive(Debug)]
enum OrElseState<F1, F2> {
    Primary(F1, F2),
    Fallback(F2),
    Done,
}

#[derive(Debug)]
pub struct OrElse<F1, F2> {
    state: Option<OrElseState<F1, F2>>,
}

impl<F1, F2> OrElse<F1, F2> {
    pub const fn new(primary: F1, fallback: F2) -> Self {
        Self {
            state: Some(OrElseState::Primary(primary, fallback)),
        }
    }
}

impl<F1, F2, T> Fut for OrElse<F1, F2>
where
    F1: Fut<Output = T>,
    F2: Fut<Output = T>,
{
    type Output = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        match self.state.take() {
            Some(OrElseState::Primary(mut p, fb)) => match p.poll() {
                Ok(FutResult::Done(v)) => {
                    self.state = Some(OrElseState::Done);
                    Ok(FutResult::Done(v))
                }
                Ok(FutResult::Pending) => {
                    self.state = Some(OrElseState::Primary(p, fb));
                    Ok(FutResult::Pending)
                }
                Ok(FutResult::Waiting) => {
                    self.state = Some(OrElseState::Primary(p, fb));
                    Ok(FutResult::Waiting)
                }
                Err(_) => {
                    self.state = Some(OrElseState::Fallback(fb));
                    Ok(FutResult::Pending)
                }
            },
            Some(OrElseState::Fallback(mut fb)) => match fb.poll() {
                Ok(r @ FutResult::Done(_)) => {
                    self.state = Some(OrElseState::Done);
                    Ok(r)
                }
                Ok(FutResult::Pending) => {
                    self.state = Some(OrElseState::Fallback(fb));
                    Ok(FutResult::Pending)
                }
                Ok(FutResult::Waiting) => {
                    self.state = Some(OrElseState::Fallback(fb));
                    Ok(FutResult::Waiting)
                }
                Err(e) => {
                    self.state = Some(OrElseState::Done);
                    Err(e)
                }
            },
            Some(OrElseState::Done) | None => Err(FutError::PolledAfterCompletion),
        }
    }
}

/// Race combinator: completes when the first of two futures finishes (success or error).
#[derive(Debug)]
pub struct Race<F1, F2> {
    f1: Option<F1>,
    f2: Option<F2>,
}

impl<F1, F2> Race<F1, F2> {
    pub const fn new(f1: F1, f2: F2) -> Self {
        Self {
            f1: Some(f1),
            f2: Some(f2),
        }
    }
}

impl<F1, F2, T> Fut for Race<F1, F2>
where
    F1: Fut<Output = T>,
    F2: Fut<Output = T>,
{
    type Output = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        if let Some(mut a) = self.f1.take() {
            match a.poll() {
                Ok(FutResult::Done(v)) => return Ok(FutResult::Done(v)),
                Ok(FutResult::Pending | FutResult::Waiting) => {
                    self.f1 = Some(a);
                }
                Err(e) => return Err(e),
            }
        }

        if let Some(mut b) = self.f2.take() {
            match b.poll() {
                Ok(FutResult::Done(v)) => return Ok(FutResult::Done(v)),
                Ok(FutResult::Pending | FutResult::Waiting) => {
                    self.f2 = Some(b);
                }
                Err(e) => return Err(e),
            }
        }

        Ok(FutResult::Pending)
    }
}

/// Ergonomic extension trait with common combinators.
pub trait FutExt: Fut + Sized {
    fn and_then<F2, FN>(self, f: FN) -> Chain<Self, F2, FN>
    where
        FN: FnOnce(Self::Output) -> F2,
        F2: Fut,
    {
        self.then(f)
    }

    fn map<U, FN>(self, f: FN) -> Map<Self, FN>
    where
        FN: FnOnce(Self::Output) -> U,
    {
        Map::new(self, f)
    }

    fn join<F2>(self, other: F2) -> Join<Self, F2>
    where
        F2: Fut,
    {
        Join::new(self, other)
    }

    fn or_else<F2>(self, fallback: F2) -> OrElse<Self, F2>
    where
        F2: Fut<Output = Self::Output>,
    {
        OrElse::new(self, fallback)
    }

    fn race<F2>(self, other: F2) -> Race<Self, F2>
    where
        F2: Fut<Output = Self::Output>,
    {
        Race::new(self, other)
    }
}

impl<F: Fut> FutExt for F {}
