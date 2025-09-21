# futures

A minimal, custom future-like implementation in Rust for learning and experimentation. It models polling, states, chaining, and simple combinators without relying on `std::future::Future` or executors.

This library is fully synchronous and small enough to read end-to-end.

Note: The folder is named `futures`. If you also use the official `futures` crate, prefer explicit module paths to avoid confusion.

## Module layout

- `src/futures/fut_core.rs` — core types, futures, and combinators

## Build

- Requires Rust stable

```bash
cargo build
```

## Core API

- Error
  - `FutError`
    - `PolledAfterCompletion`
- Poll result
  - `FutResult<T>`: `Pending | Waiting | Done(T)`
- Trait
  - `trait Fut { type Output; fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError>; fn then<F2, FN>(self, f: FN) -> Chain<_, _, _>; }`
- Provided futures and combinators
  - `Done<T>`: immediately completed (yields once, then errors on further polls)
  - `Failed`: always errors (never yields a value; `Output = Infallible`)
  - `Chain<F1, F2, FN>`: run `F1`, then build and run `F2` from its output
  - `Map<F, FN>`: transform a successful value
  - `Join<F1, F2>`: wait for both to complete, return a tuple
  - `OrElse<F1, F2>`: on primary error, switch to fallback
  - `Race<F1, F2>`: completes when the first of two completes (success or error)
- Extension trait
  - `FutExt` adds: `and_then` (alias for `then`), `map`, `join`, `or_else`, `race`

All combinators are single-threaded and synchronous.

## Quick primer: states

- `Pending`: not ready yet; poll again soon
- `Waiting`: temporarily blocked (e.g., pretend I/O/backoff); poll later
- `Done(T)`: completed with value `T`

## Mini executor loop

A tiny helper you can adapt in examples below:

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError};

fn run_to_completion<F: Fut>(mut fut: F) -> Result<F::Output, FutError> {
    loop {
        match fut.poll()? {
            FutResult::Done(v) => return Ok(v),
            FutResult::Pending | FutResult::Waiting => continue,
        }
    }
}
```

## Examples

Assume the module path is `crate::futures::fut_core`. Adjust imports for your project.

### 1) Done: an immediately completed future

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError, Done};

fn example_done() -> Result<i32, FutError> {
    let mut fut = Done::new(42);

    // Poll once: returns Done(42)
    match fut.poll()? {
        FutResult::Done(v) => Ok(v),
        _ => unreachable!("Done returns immediately"),
    }
}
```

Polling `Done` again will return `Err(FutError::PolledAfterCompletion)`.

### 2) Failed: always errors

```rust
use crate::futures::fut_core::{Fut, FutError, Failed};

fn example_failed() {
    let mut fut = Failed::new(FutError::PolledAfterCompletion);
    match fut.poll() {
        Ok(_) => println!("Unexpected success"),
        Err(e) => println!("Error: {:?}", e),
    }
}
```

`Failed` has `Output = Infallible` and never yields a value.

### 3) Chain: sequence two futures

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError, Done, Chain};

fn example_chain() -> Result<i32, FutError> {
    let mut fut = Chain::new(Done::new(2), |x| Done::new(x * 3));

    loop {
        match fut.poll()? {
            FutResult::Done(v) => return Ok(v), // 6
            FutResult::Pending | FutResult::Waiting => continue,
        }
    }
}
```

Or via the extension method `then` / `and_then`:

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError, Done, FutExt};

fn example_then() -> Result<i32, FutError> {
    let mut fut = Done::new(2).then(|x| Done::new(x + 40));
    run_to_completion(fut) // -> Ok(42)
}
```

### 4) Map: transform a value

```rust
use crate::futures::fut_core::{Fut, FutError, FutExt, Done};

fn example_map() -> Result<String, FutError> {
    let fut = Done::new(5).map(|x| format!("val={x}"));
    run_to_completion(fut)
}
```

### 5) Join: wait for both

```rust
use crate::futures::fut_core::{Fut, FutError, FutExt, Done};

fn example_join() -> Result<(i32, &'static str), FutError> {
    let fut = Done::new(2).join(Done::new("ok"));
    run_to_completion(fut) // -> Ok((2, "ok"))
}
```

### 6) OrElse: fallback on error

You need a primary future that can error (not `Failed`, since `Failed`’s `Output = Infallible`).
Here’s a simple flaky primary that errors once:

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError, FutExt, Done};

#[derive(Debug)]
struct FlakyOnce(bool);

impl Fut for FlakyOnce {
    type Output = i32;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        if !self.0 {
            self.0 = true;
            Err(FutError::PolledAfterCompletion) // simulate an error
        } else {
            Ok(FutResult::Done(1))
        }
    }
}

fn example_or_else() -> Result<i32, FutError> {
    let fut = FlakyOnce(false).or_else(Done::new(99));
    run_to_completion(fut) // -> Ok(99), since primary errors and we fall back
}
```

### 7) Race: complete with the first result

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError, FutExt, Done};

#[derive(Debug)]
struct Counter {
    at: u8,
    until: u8,
}

impl Counter {
    fn new(until: u8) -> Self { Self { at: 0, until } }
}

impl Fut for Counter {
    type Output = &'static str;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        if self.at + 1 >= self.until {
            Ok(FutResult::Done("counter done"))
        } else {
            self.at += 1;
            Ok(FutResult::Pending)
        }
    }
}

fn example_race() -> Result<&'static str, FutError> {
    // Counter will need a few polls; Done wins the race immediately.
    let fut = Counter::new(3).race(Done::new("instant"));
    run_to_completion(fut) // -> Ok("instant")
}
```

### 8) Custom future

Implement your own future by implementing `Fut`:

```rust
use crate::futures::fut_core::{Fut, FutResult, FutError};

#[derive(Debug)]
struct OneWaitThenDone(bool);

impl Fut for OneWaitThenDone {
    type Output = &'static str;

    fn poll(&mut self) -> Result<FutResult<Self::Output>, FutError> {
        if !self.0 {
            self.0 = true;
            Ok(FutResult::Waiting)
        } else {
            Ok(FutResult::Done("ready"))
        }
    }
}
```

## Semantics and notes

- Polling a completed `Done`, `Chain`, `OrElse` (after `Done`), etc., returns `Err(FutError::PolledAfterCompletion)`.
- `Failed` always returns `Err` and never yields a value (`Output = Infallible`).
- `Waiting` is a hint to back off; this library does not implement timers or wakers.
- All combinators are non-allocating and store their inner futures by value.

## Caveats

- This is a teaching/experiment crate. It is synchronous and not compatible with async executors.
- If your project also depends on the official `futures` crate, use explicit module paths to avoid name confusion (e.g., `crate::futures::fut_core`).