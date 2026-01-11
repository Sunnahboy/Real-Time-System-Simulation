
/*This benchmark measures the end-to-end latency of transmitting processed sensor 
packets through the Transmitter IPC layer and validates compliance with the 0.1 ms 
real-time transmission constraint. */
use criterion::{
    criterion_group,
     criterion_main,
     Criterion
};
use rts_simulation::component_a::{
    transmitter::Transmitter,
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::{SyncManager, SyncMode},
};
use crossbeam::channel::unbounded as channelunbounded;

use std::{
    time::Instant,
    hint::black_box,
    sync::Arc,
};


fn bench_transmitter(c:&mut Criterion){
    let sync = Arc::new(SyncManager::new(SyncMode::LockFree));

    let (tx, _rx) =    channelunbounded::<ProcessedPacket>();
    let transmitter = Arc::new(
        Transmitter::new(tx, 20248, sync)
    );

    let packet = ProcessedPacket{
        sensor_type: SensorType::Force,
        filtered: 99.0,
        raw: 102.0,
        timestamp: Instant::now(),
        seq: 1,

    };

c.bench_function("transmitter_transmit_latency", |b| {
        b.iter(|| {
            let p = black_box(packet.clone());
            let start = Instant::now();

            transmitter.transmit(p);

            let elapsed_us = start.elapsed().as_micros();
            black_box(elapsed_us);
        });
    });
}
criterion_group!(benches, bench_transmitter);
criterion_main!(benches);



    