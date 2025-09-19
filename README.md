# futures

A minimal, custom future-like implementation in Rust for learning and experimentation. It models polling, states, chaining, and cleanup without relying on `std::future::Future`.

This is intentionally small and synchronous, making it easy to read, step through, and extend.

## Highlights

- `Future` trait with:
  - `poll(&mut self) -> Result<FutResult<Output>, Error>`
  - `cleanup(&mut self)`
- `FutResult<T>` with explicit `FutState`:
  - `Pending`, `Waiting`, `Done`
- Built-in futures:
  - `Done<T>`: immediately completed
  - `Failed<T>`: always errors
  - `Chain<F1, F2, Fn>`: chain the output of one future into another
- Log-friendly (uses the `log` crate)

Note: The crate folder is named `futures`, which may shadow the official `futures` crate in examples. Adjust import paths if you integrate this with other code.

## Module layout

- `src/futures/fut_core.rs` — core types and implementations

## Install and build

- Requires Rust stable

Build:
```bash
cargo build
```

Enable logging with `env_logger`:

1) Add to `Cargo.toml`:
```toml
[dependencies]
log = "0.4"
env_logger = "0.11"
```

2) Initialize in your binary:
```rust
fn main() {
    env_logger::init();
    // ...
}
```

3) Run with:
```bash
RUST_LOG=debug cargo run
```

## Core concepts

- `FutState`:
  - `Pending`: not ready yet; poll again soon
  - `Waiting`: temporarily blocked (e.g., would wait for I/O); poll later
  - `Done`: completed; value is present (or it’s a bug)
- `FutResult<T>`:
  - Holds `state: FutState` and `value: Option<T>`
  - Helpers: `FutResult::pending()` and `FutResult::finished(val)`
- Errors:
  - `FutError::SleepingUnsupported`
  - `FutError::PolledAfterCompletion`
  - `FutError::CompletedWithoutValue`
- `Future` trait (custom):
  - `type Output`
  - `type Error`
  - `poll(&mut self) -> Result<FutResult<Output>, Error>`
  - `cleanup(&mut self)`

## Provided futures

- `Done<T>`: returns `Done` with the provided value on the first poll and errors on subsequent polls (`PolledAfterCompletion`)
- `Failed<T>`: returns `Err(T)` on poll
- `Chain<F1, F2, Fn>`: creates `F2` from the `Output` of `F1` using a transform function and then polls `F2` to completion
  - Requires `F2::Error = F1::Error`
  - Requires `F1::Error: Debug + From<FutError>`
  - Transform is `FnOnce(F1::Output) -> F2` but must be `Clone` to be held across polls

## How to use

Assume the module path is `crate::futures::fut_core`. Adjust for your project as needed.

### 1) Running a simple completed future (`Done`)

```rust
use crate::futures::fut_core::{Future, FutResult, FutState, FutError, Done};

fn run_done() -> Result<(), FutError> {
    let mut fut = Done::new(42);

    // Mini executor loop
    loop {
        match fut.poll()? {
            FutResult { state: FutState::Done, value: Some(v) } => {
                println!("Got: {v}");
                break;
            }
            FutResult { state: FutState::Pending, .. } => {
                // Immediately try again in this simple synchronous model
                continue;
            }
            FutResult { state: FutState::Waiting, .. } => {
                // Back off or schedule; for demo, just loop
                continue;
            }
            FutResult { state: FutState::Done, value: None } => {
                // Should never happen
                return Err(FutError::CompletedWithoutValue);
            }
        }
    }

    fut.cleanup();
    Ok(())
}
```

### 2) Handling a failed future (`Failed`)

```rust
use crate::futures::fut_core::{Future, Failed};

fn run_failed() {
    let mut fut = Failed::_new("boom".to_string());

    match fut.poll() {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Error: {:?}", e),
    }

    fut.cleanup();
}
```

### 3) Chaining futures (`Chain`)

```rust
use crate::futures::fut_core::{Future, FutResult, FutState, FutError, Done, Chain};

fn run_chain() -> Result<(), FutError> {
    // Start with Done(2), then multiply by 3 in the chained future
    let mut fut = Chain::new(Done::new(2), |x| Done::new(x * 3));

    loop {
        match fut.poll()? {
            FutResult { state: FutState::Done, value: Some(v) } => {
                println!("Chained value: {v}"); // 6
                break;
            }
            FutResult { state: FutState::Pending, .. } => continue,
            FutResult { state: FutState::Waiting, .. } => continue,
            FutResult { state: FutState::Done, value: None } => {
                return Err(FutError::CompletedWithoutValue);
            }
        }
    }

    fut.cleanup();
    Ok(())
}
```

### 4) Implementing your own future

A simple counter future that completes after N polls:

```rust
use crate::futures::fut_core::{Future, FutResult, FutState, FutError};

#[derive(Debug)]
struct Counter {
    at: u8,
    until: u8,
}

impl Counter {
    fn new(until: u8) -> Self {
        Self { at: 0, until }
    }
}

impl Future for Counter {
    type Output = String;
    type Error = FutError;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        if self.at + 1 >= self.until {
            Ok(FutResult::finished(format!("done at {}", self.until)))
        } else {
            self.at += 1;
            Ok(FutResult::pending())
        }
    }

    fn cleanup(&mut self) {
        // Free resources if necessary
    }
}

fn run_counter() -> Result<(), FutError> {
    let mut fut = Counter::new(3);

    loop {
        match fut.poll()? {
            FutResult { state: FutState::Done, value: Some(msg) } => {
                println!("Counter finished: {msg}");
                break;
            }
            FutResult { state: FutState::Pending, .. } => continue,
            FutResult { state: FutState::Waiting, .. } => continue,
            FutResult { state: FutState::Done, value: None } => {
                return Err(FutError::CompletedWithoutValue);
            }
        }
    }

    fut.cleanup();
    Ok(())
}
```

### 5) Emitting `Waiting`

You can explicitly indicate a “waiting” state to signal the executor to back off:

```rust
use crate::futures::fut_core::{Future, FutResult, FutState, FutError};

#[derive(Debug)]
struct OneWaitThenDone(bool);

impl Future for OneWaitThenDone {
    type Output = &'static str;
    type Error = FutError;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, Self::Error> {
        if !self.0 {
            self.0 = true;
            Ok(FutResult { state: FutState::Waiting, value: None })
        } else {
            Ok(FutResult::finished("ready"))
        }
    }

    fn cleanup(&mut self) {}
}
```

## Logging

This library uses `log` for instrumentation:
- You will see helpful debug messages during `poll` and `cleanup`.
- Initialize your logger (e.g., `env_logger::init()`) and use `RUST_LOG=debug`.

## Semantics and edge cases

- Polling a completed `Done` future returns `FutError::PolledAfterCompletion`.
- Chaining:
  - If the first future finishes with `None`, `Chain` returns `CompletedWithoutValue`.
  - Otherwise, it constructs the second future and continues.
- `cleanup` is best-effort resource cleanup; it can be called at any time.

## Why a custom future?

- Teaches polling/state machines without executors, wakers, or pinning.
- Good for experimenting with scheduling, retry/backoff, or instrumentation.
- Not a replacement for `std::future::Future`.

## Roadmap ideas

- Small executor example that understands `Waiting`
- Time-based backoff utilities
- More combinators (map, map_err, join)
