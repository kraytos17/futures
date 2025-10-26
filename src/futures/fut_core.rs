#![allow(unused)]

use std::fmt::Debug;

/// A simple error type for custom futures.
///
/// In this toy model, errors are not generic. All failures are represented as
/// [`FutError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FutError {
    /// Indicates that a future was polled after it had already completed
    /// (either with success or error).
    PolledAfterCompletion,
}

/// Result of polling a custom future.
///
/// This is similar to `Poll` in the standard `Future` trait, but with
/// an additional `Waiting` state.
#[derive(Debug)]
pub enum FutResult<T> {
    /// The future is not ready yet, and should be polled again later.
    Pending,
    /// The future cannot make progress right now (e.g. waiting on an external
    /// event). Semantically different from `Pending` to allow richer state.
    Waiting,
    /// The future has completed successfully with a value.
    Done(T),
}

/// Core trait for our toy futures. Unlike the standard library, the error type
/// is fixed to [`FutError`].
///
/// You call [`Fut::poll`] until the future resolves or returns an error.
/// Combinators such as [`Fut::then`], [`FutExt::map`], or [`FutExt::join`] can
/// be used to build complex asynchronous flows.
pub trait Fut {
    /// The output type produced when the future completes.
    type Output;

    /// Poll the future.
    ///
    /// Returns:
    /// - `Ok(FutResult::Done(val))` if the future has completed with `val`.
    /// - `Ok(FutResult::Pending)` if the future is not ready yet.
    /// - `Ok(FutResult::Waiting)` if the future is blocked on something external.
    /// - `Err(FutError)` if polled after completion or another fatal error.
    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError>;

    /// Convenience combinator to chain futures sequentially.
    ///
    /// Equivalent to `and_then` in functional programming: runs this future,
    /// then invokes `f` with its output to produce another future.
    ///
    /// # Type Parameters
    /// - `F2`: The type of the future returned by the closure `FN`
    /// - `FN`: Closure type that transforms `Self::Output` into a new future `F2`
    ///
    /// # Interactions
    /// - The closure `f` is only called once the first future completes successfully
    /// - If the first future returns an error, the chain short-circuits and returns the error
    /// - The output type of the chain is `F2::Output`
    fn then<F2, FN>(self, f: FN) -> Chain<Self, F2, FN>
    where
        Self: Sized,
        FN: FnOnce(Self::Output) -> F2,
        F2: Fut,
    {
        Chain::new(self, f)
    }
}

/// A future that is immediately ready (returns a value once).
///
/// After the first poll, it returns `Done(val)`. Any further polls yield
/// [`FutError::PolledAfterCompletion`].
#[derive(Debug)]
pub struct Done<T>(Option<T>);

impl<T> Done<T> {
    /// Create a `Done` future holding `val`.
    pub const fn new(val: T) -> Self {
        Self(Some(val))
    }
}

impl<T> Fut for Done<T> {
    type Output = T;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        self.0.take().map_or_else(
            || Err(FutError::PolledAfterCompletion),
            |v| Ok(FutResult::Done(v)),
        )
    }
}

/// A future that always fails with a [`FutError`].
///
/// After yielding the error once, subsequent polls panic.
#[derive(Debug)]
pub struct Failed(Option<FutError>);

impl Failed {
    /// Create a `Failed` future holding `err`.
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

/// Internal state machine for [`Chain`].
#[derive(Debug)]
enum ChainState<F1, F2, FN> {
    First(F1, Option<FN>),
    Second(F2),
    Done,
}

/// Chain combinator: run `F1`, and when it yields a value,
/// build and run `F2` using a closure.
///
/// This is similar to `and_then` in the `futures` crate.
///
/// # Type Parameters
/// - `F1`: The first future to execute
/// - `F2`: The future produced by the closure
/// - `FN`: Closure type that maps `F1::Output` to `F2`
///
/// # State Transitions
/// 1. Starts in `First` state, polling `F1`
/// 2. When `F1` completes, moves to `Second` state with `F2`
/// 3. When `F2` completes, moves to `Done` state
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
    /// Create a new `Chain` combinator.
    ///
    /// # Parameters
    /// - `f1`: The first future to execute
    /// - `f`: Closure that will be called with `f1`'s output to create the second future
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

/// Map combinator: transform the successful output value of a future.
///
/// Similar to `map` in functional programming.
///
/// # Type Parameters
/// - `F`: The underlying future type
/// - `FN`: Closure type that maps `F::Output` to a new type `U`
/// - `T`: Original output type from future `F`  
/// - `U`: Transformed output type after applying the closure
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
    /// Create a new `Map` combinator.
    ///
    /// # Parameters
    /// - `fut`: The future whose output will be transformed
    /// - `f`: Closure that maps the future's output from `T` to `U`
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

/// Join combinator: poll two futures and return a tuple when both are `Done`.
///
/// Returns `Waiting` if either is `Waiting`. Returns `Pending` if one or both
/// are still in progress.
///
/// # Type Parameters
/// - `F1`: First future type with output `T1`
/// - `F2`: Second future type with output `T2`
/// - `T1`: Output type of the first future
/// - `T2`: Output type of the second future
///
/// # Behavior
/// - Both futures are polled on every call to `poll`
/// - Returns `Done` only when both futures have completed
/// - Returns `Waiting` if either future returns `Waiting` (conservative blocking)
/// - Returns `Pending` if both are still making progress
#[derive(Debug)]
pub struct Join<F1, F2> {
    f1: F1,
    f2: F2,
}

impl<F1, F2> Join<F1, F2> {
    /// Create a new `Join` combinator.
    ///
    /// # Parameters
    /// - `f1`: First future to execute concurrently
    /// - `f2`: Second future to execute concurrently
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

/// State machine for [`OrElse`].
#[derive(Debug)]
enum OrElseState<F1, F2> {
    Primary(F1, F2),
    Fallback(F2),
    Done,
}

/// `OrElse` combinator: run a primary future, fall back to another future if it fails.
///
/// # Type Parameters
/// - `F1`: Primary future type
/// - `F2`: Fallback future type  
/// - `T`: Common output type that both futures must produce
///
/// # Behavior
/// - First attempts to complete the primary future
/// - If primary fails with an error, switches to the fallback future
/// - If primary succeeds, returns its value and ignores fallback
/// - If primary returns `Pending` or `Waiting`, continues polling it
#[derive(Debug)]
pub struct OrElse<F1, F2> {
    state: Option<OrElseState<F1, F2>>,
}

impl<F1, F2> OrElse<F1, F2> {
    /// Create a new `OrElse` combinator with a primary and fallback future.
    ///
    /// # Parameters
    /// - `primary`: The main future to attempt first
    /// - `fallback`: The backup future to use if primary fails
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

/// Race combinator: completes when the first of two futures finishes
/// (either successfully or with an error).
///
/// Returns `Pending` if neither has finished yet.
///
/// # Type Parameters
/// - `F1`: First future type with output `T`
/// - `F2`: Second future type with output `T`
/// - `T`: Common output type that both futures produce
///
/// # Behavior
/// - Polls both futures on each call
/// - Returns immediately when either future completes (success or error)
/// - If both complete on the same poll, the first future takes precedence
/// - Short-circuits on error from either future
#[derive(Debug)]
pub struct Race<F1, F2> {
    f1: Option<F1>,
    f2: Option<F2>,
}

impl<F1, F2> Race<F1, F2> {
    /// Create a new `Race` combinator.
    ///
    /// # Parameters
    /// - `f1`: First future to race
    /// - `f2`: Second future to race
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
///
/// Provides method syntax for [`then`], [`map`], [`join`], [`or_else`],
/// and [`race`].
pub trait FutExt: Fut + Sized {
    /// Sequentially chain two futures.
    ///
    /// See [`Fut::then`] for detailed documentation.
    fn and_then<F2, FN>(self, f: FN) -> Chain<Self, F2, FN>
    where
        FN: FnOnce(Self::Output) -> F2,
        F2: Fut,
    {
        self.then(f)
    }

    /// Transform the output of this future with a closure.
    ///
    /// # Type Parameters
    /// - `U`: The new output type after transformation
    /// - `FN`: Closure type that maps `Self::Output` to `U`
    ///
    /// # Example
    /// ```
    /// # use your_crate::*;
    /// let future = Done::new(5).map(|x| x * 2);
    /// ```
    fn map<U, FN>(self, f: FN) -> Map<Self, FN>
    where
        FN: FnOnce(Self::Output) -> U,
    {
        Map::new(self, f)
    }

    /// Run two futures concurrently and wait until both complete.
    ///
    /// # Type Parameters
    /// - `F2`: The type of the second future to join with
    ///
    /// # Returns
    /// A future that resolves to a tuple `(Self::Output, F2::Output)` when both complete.
    fn join<F2>(self, other: F2) -> Join<Self, F2>
    where
        F2: Fut,
    {
        Join::new(self, other)
    }

    /// Use a fallback future if the primary future fails.
    ///
    /// # Type Parameters
    /// - `F2`: Fallback future type that must produce the same output type
    ///
    /// # Note
    /// Both futures must have the same `Output` type for this combinator to work.
    fn or_else<F2>(self, fallback: F2) -> OrElse<Self, F2>
    where
        F2: Fut<Output = Self::Output>,
    {
        OrElse::new(self, fallback)
    }

    /// Race two futures and resolve with whichever finishes first.
    ///
    /// # Type Parameters
    /// - `F2`: The future to race against, must have the same output type
    ///
    /// # Returns
    /// The value from whichever future completes first.
    fn race<F2>(self, other: F2) -> Race<Self, F2>
    where
        F2: Fut<Output = Self::Output>,
    {
        Race::new(self, other)
    }
}

impl<F: Fut> FutExt for F {}
