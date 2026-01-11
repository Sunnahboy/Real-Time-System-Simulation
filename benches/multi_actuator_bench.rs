use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use std::{
    time::Instant,
    sync::Arc,
};

use rts_simulation::component_b::{
    multi_actuator::MultiActuator,
    feedback::FeedbackLoop,
};
use rts_simulation::component_a::{
    processor::ProcessedPacket,
    sensor::SensorType,
    sync_manager::{SyncManager, SyncMode},

};
use rts_simulation::utils::metrics::{SharedMetrics, EventRecorder};

fn multi_actuator_dispatch_bench(c: &mut Criterion) {
    let sync = Arc::new(SyncManager::new(SyncMode::LockFree));
    let event_recorder = Arc::new(EventRecorder::new());
    let actuators = MultiActuator::new(sync.clone(),FeedbackLoop::new(500, event_recorder.clone()).0.clone(),SharedMetrics::default().clone(), event_recorder.clone());

    let sensors = [
        SensorType::Force,
        SensorType::Position,
        SensorType::Temperature,
    ];

    let mut group = c.benchmark_group("multi_actuator_dispatch");

    for sensor in sensors {
        group.bench_with_input(
            BenchmarkId::new("dispatch", format!("{:?}", sensor)),
            &sensor,
            |b, s| {
                let sync_clone = sync.clone();
                b.iter(|| {
                    let pkt = ProcessedPacket {
                        sensor_type: s.clone(),
                        filtered: 10.0,
                        raw: 10.0,
                        timestamp: Instant::now(),
                        seq: 1,
                    };
                    actuators.dispatch(pkt, sync_clone.clone());
                })
            },
        );
    }

    group.finish();
}

criterion_group!(benches, multi_actuator_dispatch_bench);
criterion_main!(benches);
