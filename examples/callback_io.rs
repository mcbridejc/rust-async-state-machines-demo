use std::{pin::Pin, sync::Arc, time::{Duration, Instant}};

use async_state_machine_example::poll_once;
use crossbeam::atomic::AtomicCell;
use futures::pending;



#[derive(Debug, Default)]
pub struct Database {
    value: u32,
}

impl Database {
    pub fn store_value(&mut self, value: u32) {
        self.value = value;
    }
}

#[derive(Default, Debug)]
struct SharedData {
    init_cmd: AtomicCell<bool>,
    running: AtomicCell<bool>,
    sensor_data: AtomicCell<Option<u32>>,
}


struct CallbackDrivenModule {
    fsm: Pin<Box<dyn Future<Output = ()>>>,
    data: Arc<SharedData>,
}

impl CallbackDrivenModule {
    pub fn new() -> Self {
        let data = Arc::new(SharedData::default());
        Self {
            fsm: Box::pin(task(data.clone())),
            data,
        }
    }

    pub fn run(&mut self, db: &mut Database) {
        // poll the async task one time
        poll_once(self.fsm.as_mut());
        // If a new sample has come in, store it
        if let Some(value) = self.data.sensor_data.take() {
            db.store_value(value);
        }
    }

    pub fn enable(&mut self, value: bool) {
        self.data.init_cmd.store(value);
    }

    pub fn running(&self) -> bool {
        self.data.running.load()
    }
}

/// Fake init task. It just waits for 1 second before completing
async fn do_init() {
    let done_time = Instant::now() + Duration::from_secs(1);

    loop {
        if Instant::now() >= done_time {
            break;
        }
        // Yield back execution
        pending!()
    }
}


async fn task(data: Arc<SharedData>) {

    // Wait for init flag to start up
    while !data.init_cmd.load() {
        pending!()
    }

    // Simulated initialization task
    // This could be, e.g. sending out UDP packets and waiting for responses, signalling to another thread, etc. Anything that has to be checked back on.
    do_init().await;

    data.running.store(true);

    const SENSOR_PERIOD: Duration = Duration::from_millis(100);
    let mut next_data_time = Instant::now() + SENSOR_PERIOD;
    while data.init_cmd.load() {
        if Instant::now() >= next_data_time {
            next_data_time += SENSOR_PERIOD;
            data.sensor_data.store(Some(42));
        }
        pending!()
    }

    data.running.store(false);
}


fn main() {

    let mut db = Database::default();
    let mut module = CallbackDrivenModule::new();

    // Run once
    module.run(&mut db);
    assert_eq!(db.value, 0);
    assert_eq!(module.running(), false);

    // Set flag to enable module
    module.enable(true);

    module.run(&mut db);
    // Still initializing
    assert_eq!(module.running(), false);

    std::thread::sleep(Duration::from_secs(2));

    module.run(&mut db);
    // Now the init process should have completed
    assert_eq!(module.running(), true);
    std::thread::sleep(Duration::from_millis(200));
    module.run(&mut db);
    assert_eq!(db.value, 42);
}