use criterion::{criterion_group, criterion_main, Criterion};
use std::{
    hint::black_box,
    time::Instant,
    sync::Arc,
};

use rts_simulation::component_b::{
    controller::Controller,
    feedback::FeedbackLoop,
};
use rts_simulation::component_a::{
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::{SyncManager, SyncMode},
};

use rts_simulation::utils::metrics::{SharedMetrics, EventRecorder};

fn pid_compute_bench(c: &mut Criterion) {
    let sync = Arc::new(SyncManager::new(SyncMode::Atomics));
    let event_recorder = Arc::new(EventRecorder::new());
    let mut controller = Controller::new(sync,FeedbackLoop::new(500, event_recorder.clone()).0.clone(),SharedMetrics::default(), event_recorder.clone());

    let pkt = ProcessedPacket {
        sensor_type: SensorType::Force,
        filtered: 98.5,
        raw: 100.0,
        timestamp: Instant::now(),
        seq: 1,
    };

    c.bench_function("pid_compute_and_actuate", |b| {
        b.iter(|| {
            controller.handle_packet(black_box(&pkt));
        })
    });
}

criterion_group!(benches, pid_compute_bench);
criterion_main!(benches);
