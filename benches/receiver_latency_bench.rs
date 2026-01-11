












use crossbeam::channel::bounded;
use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use std::{
    hint::black_box,
    time::{Duration, Instant},
    sync::Arc,
};

use rts_simulation::component_b::{
    receiver::Receiving,
    multi_actuator::MultiActuator,
    feedback::FeedbackLoop,
};
use rts_simulation::component_a::{
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::{SyncManager, SyncMode},
};
use rts_simulation::utils::metrics::{SharedMetrics, EventRecorder};

fn receiver_latency_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("receiver_latency");
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(50);

    group.bench_function("recv_latency", |b| {
        b.iter_batched(
            || {
                // SETUP: Prepare everything before the timer starts
                let sync = Arc::new(SyncManager::new(SyncMode::Atomics));
                let (tx, rx) = bounded(1);
                let event_recorder = Arc::new(EventRecorder::new());

                let receiver = Receiving::new(
                    rx,
                    sync.clone(),
                    MultiActuator::new(
                        sync.clone(),
                        FeedbackLoop::new(500, event_recorder.clone()).0.clone(),
                        SharedMetrics::default().clone(),
                        event_recorder.clone()
                    ),
                    FeedbackLoop::new(500, event_recorder.clone()).0.clone(),
                    SharedMetrics::default().clone(),
                    event_recorder.clone()
                );

                let pkt = ProcessedPacket {
                    sensor_type: SensorType::Force,
                    filtered: 100.0,
                    raw: 100.0,
                    timestamp: Instant::now(),
                    seq: 1,
                };

                (tx, receiver, pkt)
            },
            |(tx, mut receiver, pkt)| {
                // MEASURE: This block is timed
                tx.send(pkt).unwrap();
                
                // CRITICAL: Drop the sender immediately. 
                // This makes self.rx.recv() return Err inside receiver.run() 
                // after the packet is processed, breaking the while loop.
                drop(tx); 

                black_box(receiver.run());
            },
            BatchSize::SmallInput
        )
    });

    group.finish();
}

criterion_group!(benches, receiver_latency_bench);
criterion_main!(benches);
