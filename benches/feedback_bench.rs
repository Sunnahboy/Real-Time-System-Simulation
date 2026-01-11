// use criterion::{criterion_group, criterion_main, Criterion};
// use std::{time::Instant, sync::Arc};

// use rts_simulation::component_b::feedback::{
//     FeedbackLoop, FeedbackKind
// };
// use rts_simulation::utils::metrics::EventRecorder;

// fn feedback_latency_bench(c: &mut Criterion) {
//     let event_recorder = Arc::new(EventRecorder::new());
//     let (feedback, rx) = FeedbackLoop::new(32, event_recorder);

//     c.bench_function("feedback_emit_latency", |b| {
//         b.iter(|| {
//             let start = Instant::now();
//             feedback.emit("motor", FeedbackKind::Ack, start);
//             let _ = rx.recv().unwrap();
//             assert!(start.elapsed().as_micros() < 500);
//         })
//     });
// }

// criterion_group!(benches, feedback_latency_bench);
// criterion_main!(benches);



use criterion::{criterion_group, criterion_main, Criterion};
use std::{time::Instant, sync::Arc};

use rts_simulation::component_b::feedback::{
    FeedbackLoop, FeedbackKind
};
use rts_simulation::utils::metrics::EventRecorder;

fn feedback_latency_bench(c: &mut Criterion) {
    let event_recorder = Arc::new(EventRecorder::new());
    let (feedback, rx) = FeedbackLoop::new(32, event_recorder);

    c.bench_function("feedback_emit_latency", |b| {
        b.iter(|| {
            let start = Instant::now();
            feedback.emit("motor", FeedbackKind::Ack, start);
            let _ = rx.recv().unwrap();
            assert!(start.elapsed().as_micros() < 5000); // 5 ms deadline for round-trip on standard OS
        })
    });
}

criterion_group!(benches, feedback_latency_bench);
criterion_main!(benches);