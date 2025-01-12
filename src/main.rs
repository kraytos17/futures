use futures::{
    fut_test::{
        test_chained_futures, test_poll_runner, test_sequential_execution, test_simple_runner,
    },
    futures::FutError,
};

mod futures;

fn main() -> Result<(), FutError> {
    println!("=== Testing Simple Runner ===\n");
    test_simple_runner()?;

    println!("\n=== Testing Poll Runner ===\n");
    test_poll_runner()?;

    println!("\n=== Testing Sequential Future Execution ===\n");
    test_sequential_execution()?;

    println!("\n=== Testing Chained Future Execution ===\n");
    test_chained_futures()?;

    Ok(())
}
