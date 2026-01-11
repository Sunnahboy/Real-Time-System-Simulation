
// Component A: Sensor Data Simulator
//simuletes periodic  sensor data generation, processing,
//synchronization and tansmission .it also benchmarks timing to
//measures latency , jitter and througput under simuluted real time constraints
//Handles data generation, filtering, synchronization, and transmission.

pub mod sensor;
pub mod processor;
pub mod sync_manager;
pub mod transmitter;

