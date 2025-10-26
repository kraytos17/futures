//! A simple custom future executor framework supporting sequential and chained future execution.

use crate::futures::fut_core::{Chain, Done, Failed, Fut, FutError, FutResult};
use log::debug;
use std::{cell::RefCell, collections::VecDeque, fmt::Debug, rc::Rc};

/// Trait that defines a basic interface for scheduling and running futures.
pub trait FutRunner {
    /// Schedule a new future for execution.
    fn schedule<F>(&mut self, future: F)
    where
        F: Fut<Output = usize> + 'static;

    /// Returns `true` if no futures are scheduled for execution.
    fn is_empty(&self) -> bool;

    /// Runs the scheduled futures to completion or until an error occurs.
    fn run(&mut self) -> Result<(), FutError>;
}

/// A simple future runner that does not support sleeping/waiting futures.
pub struct SimpleRunner {
    futs: VecDeque<Box<dyn Fut<Output = usize>>>,
}

impl SimpleRunner {
    pub fn new() -> Self {
        Self {
            futs: VecDeque::new(),
        }
    }
}

impl FutRunner for SimpleRunner {
    fn schedule<F>(&mut self, fut: F)
    where
        F: Fut<Output = usize> + 'static,
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
                    FutResult::Pending => i += 1,
                    FutResult::Waiting => {
                        // SimpleRunner doesn't support waiting/sleeping futures.
                        return Err(FutError::PolledAfterCompletion);
                    }
                    FutResult::Done(_) => {
                        // remove completed future
                        self.futs.remove(i);
                    }
                }
            }
        }
        Ok(())
    }
}

/// A more advanced runner supporting active, pending, and sleeping queues.
#[derive(Default)]
pub struct PollRunner {
    active: VecDeque<Box<dyn Fut<Output = usize>>>,
    pending: VecDeque<Box<dyn Fut<Output = usize>>>,
    sleeping: VecDeque<Box<dyn Fut<Output = usize>>>,
}

impl PollRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move sleeping futures back to pending (simulates wake-up).
    fn handle_sleeping_futures(&mut self) {
        while let Some(future) = self.sleeping.pop_front() {
            self.pending.push_back(future);
        }
    }
}

impl FutRunner for PollRunner {
    fn schedule<F>(&mut self, fut: F)
    where
        F: Fut<Output = usize> + 'static,
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
                    FutResult::Pending => self.pending.push_back(future),
                    FutResult::Waiting => {
                        self.sleeping.push_back(future);
                    }
                    FutResult::Done(_) => {
                        // drop the completed future
                    }
                }
            }

            self.handle_sleeping_futures();
        }

        Ok(())
    }
}

/// Tracks execution order and results during future execution.
#[derive(Debug, Default)]
struct TestTracker {
    execution_order: Vec<String>,
    results: Vec<usize>,
}

impl TestTracker {
    /// Records the execution step (e.g., creation, polling).
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

impl Fut for TrackDone<usize> {
    type Output = usize;

    /// Polls the wrapped future, logging the poll action and final result.
    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        self.tracker
            .borrow_mut()
            .track_exec_order(&format!("Polling {}", self.id));

        match self.inner.poll()? {
            FutResult::Done(val) => {
                self.track_result(val);
                Ok(FutResult::Done(val))
            }
            FutResult::Pending => Ok(FutResult::Pending),
            FutResult::Waiting => Ok(FutResult::Waiting),
        }
    }
}

pub fn test_simple_runner() -> Result<(), FutError> {
    let mut runner = SimpleRunner::new();
    runner.schedule(Done::new(42));

    let future_chain = Chain::new(Done::new(10), |x| Done::new(x + 5));
    runner.schedule(future_chain);
    runner.run()?;

    debug!("Simple runner completed successfully");
    Ok(())
}

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
    Ok(())
}

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

pub fn test_failed_future() {
    let mut f = Failed::new(FutError::PolledAfterCompletion);
    let result = f.poll();

    debug!("Polling Failed future returned: {result:?}");
    assert!(result.is_err());
}
