//! A simple custom future executor framework supporting sequential and chained future execution.

use crate::futures::fut_core::{Chain, Done, FutError, FutResult, FutState, Future};
use log::debug;
use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

/// Trait that defines a basic interface for scheduling and running futures.
pub trait FutureRunner {
    /// Schedule a new future for execution.
    fn schedule<F>(&mut self, future: F)
    where
        F: Future<Output = usize, Error = FutError> + 'static;

    /// Returns `true` if no futures are scheduled for execution.
    fn is_empty(&self) -> bool;

    /// Runs the scheduled futures to completion or until an error occurs.
    fn run(&mut self) -> Result<(), FutError>;
}

/// A simple future runner that does not support sleeping/waiting futures.
pub struct SimpleRunner {
    futs: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
}

impl SimpleRunner {
    /// Constructs a new `SimpleRunner`.
    pub fn new() -> Self {
        Self {
            futs: VecDeque::new(),
        }
    }
}

impl FutureRunner for SimpleRunner {
    fn schedule<F>(&mut self, fut: F)
    where
        F: Future<Output = usize, Error = FutError> + 'static,
    {
        self.futs.push_back(Box::new(fut));
    }

    fn is_empty(&self) -> bool {
        self.futs.is_empty()
    }

    fn run(&mut self) -> Result<(), FutError> {
        while !self.is_empty() {
            let mut i = 0;
            while i < self.futs.len() {
                match self.futs[i].poll()? {
                    FutResult {
                        state: FutState::Pending,
                        ..
                    } => i += 1,
                    FutResult {
                        state: FutState::Waiting,
                        ..
                    } => return Err(FutError::SleepingUnsupported),
                    FutResult {
                        state: FutState::Done,
                        ..
                    } => {
                        if let Some(mut f) = self.futs.remove(i) {
                            f.cleanup();
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// A more advanced runner supporting active, pending, and sleeping states for futures.
#[derive(Default)]
pub struct PollRunner {
    active: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
    pending: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
    sleeping: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
}

impl PollRunner {
    /// Constructs a new `PollRunner`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles transitioning sleeping futures to the pending queue.
    fn handle_sleeping_futures(&mut self) {
        if self.sleeping.is_empty() {
            return;
        }

        let remaining = VecDeque::new();
        while let Some(future) = self.sleeping.pop_front() {
            self.pending.push_back(future);
        }

        self.sleeping = remaining;
    }
}

impl FutureRunner for PollRunner {
    fn schedule<F>(&mut self, fut: F)
    where
        F: Future<Output = usize, Error = FutError> + 'static,
    {
        self.pending.push_back(Box::new(fut));
    }

    fn is_empty(&self) -> bool {
        self.active.is_empty() && self.sleeping.is_empty() && self.pending.is_empty()
    }

    fn run(&mut self) -> Result<(), FutError> {
        while !self.is_empty() {
            if !self.pending.is_empty() {
                self.active.append(&mut self.pending);
            }

            while let Some(mut future) = self.active.pop_front() {
                match future.poll()? {
                    FutResult {
                        state: FutState::Pending,
                        ..
                    } => self.pending.push_back(future),
                    FutResult {
                        state: FutState::Waiting,
                        value,
                    } => {
                        if value.is_some() {
                            self.sleeping.push_back(future);
                        }
                    }
                    FutResult {
                        state: FutState::Done,
                        ..
                    } => future.cleanup(),
                }
            }

            self.handle_sleeping_futures();
        }
        Ok(())
    }
}

/// A simple test demonstrating usage of `SimpleRunner` with completed and chained futures.
pub fn test_simple_runner() -> Result<(), FutError> {
    let mut runner = SimpleRunner::new();
    runner.schedule(Done::new(42));

    let future_chain = Chain::new(Done::new(10), |x| Done::new(x + 5));
    runner.schedule(future_chain);
    runner.run()?;

    debug!("Simple runner completed successfully");

    Ok(())
}

/// A test showing `PollRunner` executing a mixture of futures, including chained ones.
pub fn test_poll_runner() -> Result<(), FutError> {
    let mut runner = PollRunner::new();

    runner.schedule(Done::new(1));
    runner.schedule(Done::new(2));

    let complex_chain = Chain::new(Done::new(3), |x| {
        Chain::new(Done::new(x + 1), |y| Done::new(y * 2))
    });

    runner.schedule(complex_chain);
    runner.run()?;

    debug!("Poll runner completed successfully");

    Ok(())
}

/// Tracks execution order and results during future execution.
#[derive(Debug, Default)]
struct TestTracker {
    execution_order: Vec<String>,
    results: Vec<usize>,
}

impl TestTracker {
    /// Records the execution step (e.g., creation, polling, cleanup).
    pub fn track_exec_order(&mut self, step: &str) {
        self.execution_order.push(step.to_string());
    }

    /// Records the result produced by a completed future.
    pub fn track_result(&mut self, res: usize) {
        self.results.push(res);
    }
}

/// A wrapper around `Done<T>` that logs its lifecycle into a shared `TestTracker`.
#[derive(Debug)]
struct TrackDone<T> {
    inner: Done<T>,
    tracker: Rc<RefCell<TestTracker>>,
    id: String,
}

impl<T: Debug> TrackDone<T> {
    /// Creates a new `TrackDone`, registering creation with the tracker.
    pub fn new(val: T, tracker: Rc<RefCell<TestTracker>>, id: &str) -> Self {
        tracker
            .borrow_mut()
            .track_exec_order(&format!("Creating {id}"));

        Self {
            inner: Done::new(val),
            tracker,
            id: id.to_string(),
        }
    }
}

impl TrackDone<usize> {
    fn track_result(&self, res: usize) {
        self.tracker.borrow_mut().track_result(res);
    }
}

impl Future for TrackDone<usize> {
    type Output = usize;
    type Error = FutError;

    /// Polls the wrapped future, logging the poll action and final result.
    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        self.tracker
            .borrow_mut()
            .track_exec_order(&format!("Polling {}", self.id));
        match self.inner.poll()? {
            FutResult {
                state: FutState::Done,
                value: Some(val),
            } => {
                self.track_result(val);
                Ok(FutResult::finished(val))
            }
            other => Ok(other),
        }
    }

    /// Cleans up the wrapped future and logs destruction.
    fn cleanup(&mut self) {
        self.tracker
            .borrow_mut()
            .track_exec_order(&format!("Destroying {}", self.id));
        self.inner.cleanup();
    }
}

/// Test that verifies sequential execution and tracking of simple futures.
pub fn test_sequential_execution() -> Result<(), FutError> {
    let tracker = Rc::new(RefCell::new(TestTracker::default()));
    let mut runner = PollRunner::new();

    let fut1 = TrackDone::new(5, Rc::clone(&tracker), "Future1");
    let fut2 = TrackDone::new(10, Rc::clone(&tracker), "Future2");

    runner.schedule(fut1);
    runner.schedule(fut2);
    runner.run()?;

    let tracker = tracker.borrow();
    debug!("Execution order: {:?}", tracker.execution_order);
    debug!("Results: {:?}", tracker.results);

    assert_eq!(tracker.results, vec![5, 10]);

    assert!(
        tracker
            .execution_order
            .contains(&"Creating Future1".to_string())
    );

    assert!(
        tracker
            .execution_order
            .contains(&"Creating Future2".to_string())
    );

    assert!(
        tracker
            .execution_order
            .contains(&"Polling Future1".to_string())
    );

    assert!(
        tracker
            .execution_order
            .contains(&"Polling Future2".to_string())
    );

    Ok(())
}

/// Test demonstrating chained futures and their tracked execution.
pub fn test_chained_futures() -> Result<(), FutError> {
    let tracker = Rc::new(RefCell::new(TestTracker::default()));
    let mut runner = PollRunner::new();

    let initial = TrackDone::new(5, Rc::clone(&tracker), "Initial");
    let tracker_clone = Rc::clone(&tracker);
    let chain = Chain::new(initial, move |x| {
        TrackDone::new(x * 2, Rc::clone(&tracker_clone), "Chained")
    });

    runner.schedule(chain);
    runner.run()?;

    let tracker = tracker.borrow();
    debug!("Chain execution order: {:?}", tracker.execution_order);
    debug!("Chain results: {:?}", tracker.results);

    assert_eq!(tracker.results.last(), Some(&10));

    Ok(())
}

/// Test that explicitly uses the `Waiting` state in a FutResult.
pub fn test_waiting_state() {
    let res: FutResult<()> = FutResult {
        state: FutState::Waiting,
        value: None,
    };
    debug!("Constructed FutResult in Waiting state: {:?}", res);

    assert_eq!(res.state, FutState::Waiting);
    assert!(res.value.is_none());
}

/// Test that constructs and polls a `Failed` future.
pub fn test_failed_future() {
    use crate::futures::fut_core::Failed;

    let mut f = Failed::_new("test-error".to_string());
    let result = f.poll();

    debug!("Polling Failed future returned: {:?}", result);
    assert!(result.is_err());
}
