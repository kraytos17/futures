use crate::futures::fut_test;
use log::{debug, error, info};
use simple_logger::SimpleLogger;

mod futures;

fn main() {
    SimpleLogger::new().init().unwrap();
    info!("Application started");

    debug!("=== Testing Simple Runner ===\n");
    if let Err(e) = fut_test::test_simple_runner() {
        error!("Simple runner test failed: {e:?}");
    }

    debug!("=== Testing Poll Runner ===\n");
    if let Err(e) = fut_test::test_poll_runner() {
        error!("Poll runner test failed: {e:?}");
    }

    debug!("=== Testing Sequential Execution ===\n");
    if let Err(e) = fut_test::test_sequential_execution() {
        error!("Sequential execution test failed: {e:?}");
    }

    debug!("=== Testing Chained Futures ===\n");
    if let Err(e) = fut_test::test_chained_futures() {
        error!("Chained futures test failed: {e:?}");
    }

    debug!("=== Testing Failed Future ===\n");
    fut_test::test_failed_future();

    info!("All tests completed");
}
