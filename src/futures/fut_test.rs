use super::futures::{Done, FutError, FutResult, FutState, Future};
use crate::futures::futures::Then;
use log::debug;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::rc::Rc;

pub struct SimpleRunner {
    futs: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
}

impl SimpleRunner {
    pub fn new() -> Self {
        Self {
            futs: VecDeque::new(),
        }
    }

    pub fn schedule<F>(&mut self, fut: F)
    where
        F: Future<Output = usize, Error = FutError> + 'static,
    {
        self.futs.push_back(Box::new(fut));
    }

    pub fn is_empty(&self) -> bool {
        self.futs.is_empty()
    }

    pub fn run(&mut self) -> Result<(), FutError> {
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
                            f.destroy();
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Default)]
pub struct PollRunner {
    active: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
    pending: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
    sleeping: VecDeque<Box<dyn Future<Output = usize, Error = FutError>>>,
}

impl PollRunner {
    pub fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    pub fn schedule<F>(&mut self, fut: F)
    where
        F: Future<Output = usize, Error = FutError> + 'static,
    {
        self.pending.push_back(Box::new(fut));
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty() && self.sleeping.is_empty() && self.pending.is_empty()
    }

    pub fn run(&mut self) -> Result<(), FutError> {
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
                    } => future.destroy(),
                }
            }

            self.handle_sleeping_futures();
        }
        Ok(())
    }

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

pub fn test_simple_runner() -> Result<(), FutError> {
    let mut runner = SimpleRunner::new();
    runner.schedule(Done::new(42));

    let future_chain = Then::new(Done::new(10), |x| Done::new(x + 5));
    runner.schedule(future_chain);
    runner.run()?;

    debug!("Simple runner completed successfully");

    Ok(())
}

pub fn test_poll_runner() -> Result<(), FutError> {
    let mut runner = PollRunner::new();

    runner.schedule(Done::new(1));
    runner.schedule(Done::new(2));

    let complex_chain = Then::new(Done::new(3), |x| {
        Then::new(Done::new(x + 1), |y| Done::new(y * 2))
    });

    runner.schedule(complex_chain);
    runner.run()?;

    debug!("Poll runner completed successfully");

    Ok(())
}

#[derive(Debug, Default)]
struct TestTracker {
    execution_order: Vec<String>,
    results: Vec<usize>,
}

impl TestTracker {
    pub fn track_exec_order(&mut self, step: &str) {
        self.execution_order.push(step.to_string());
    }

    pub fn track_result(&mut self, res: usize) {
        self.results.push(res);
    }
}

#[derive(Debug)]
struct TrackDone<T> {
    inner: Done<T>,
    tracker: Rc<RefCell<TestTracker>>,
    id: String,
}

impl<T: Debug> TrackDone<T> {
    pub fn new(val: T, tracker: Rc<RefCell<TestTracker>>, id: &str) -> Self {
        tracker
            .borrow_mut()
            .track_exec_order(&format!("Creating {}", id));

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

    fn destroy(&mut self) {
        self.tracker
            .borrow_mut()
            .track_exec_order(&format!("Destroying {}", self.id));
        self.inner.destroy();
    }
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

    assert!(tracker
        .execution_order
        .contains(&"Creating Future1".to_string()));

    assert!(tracker
        .execution_order
        .contains(&"Creating Future2".to_string()));

    assert!(tracker
        .execution_order
        .contains(&"Polling Future1".to_string()));

    assert!(tracker
        .execution_order
        .contains(&"Polling Future2".to_string()));

    Ok(())
}

pub fn test_chained_futures() -> Result<(), FutError> {
    let tracker = Rc::new(RefCell::new(TestTracker::default()));
    let mut runner = PollRunner::new();

    let initial = TrackDone::new(5, Rc::clone(&tracker), "Initial");
    let tracker_clone = Rc::clone(&tracker);
    let chain = Then::new(initial, move |x| {
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
