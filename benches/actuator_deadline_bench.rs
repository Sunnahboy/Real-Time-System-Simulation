use criterion::{criterion_group, criterion_main, Criterion};
use std::{
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

fn actuator_deadline_bench(c: &mut Criterion) {
    let sync = Arc::new(SyncManager::new(SyncMode::Atomics));
    let event_recorder = Arc::new(EventRecorder::new());
    let mut controller = Controller::new(sync,FeedbackLoop::new(500, event_recorder.clone()).0.clone(),SharedMetrics::default(), event_recorder.clone());

    let pkt = ProcessedPacket {
        sensor_type: SensorType::Position,
        filtered: 0.1,
        raw: 0.1,
        timestamp: Instant::now(),
        seq: 1,
    };

    c.bench_function("actuator_deadline_check", |b| {
        b.iter(|| {
            let start = Instant::now();
            controller.handle_packet(&pkt);
            let elapsed = start.elapsed().as_micros();
            assert!(elapsed < 2_000);
        })
    });
}

criterion_group!(benches, actuator_deadline_bench);
criterion_main!(benches);
