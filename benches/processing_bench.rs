// // /*
// // This benchmark measures the execution latency of the Processor’s noise-reduction and anomaly-detection 
// // stage in isolation. A single sensor sample is processed using a moving-average filter and statistical
// //  anomaly detection while excluding I/O and scheduling effects. The measured execution time
// //  verify compliance with the simulated 0.2 ms real-time processing constraint.
// //   */

// use criterion::{criterion_group, criterion_main, Criterion};

// use rts_simulation::{component_a::{
//     processor::{ProcessedPacket, Processor}, 
//     sensor::{SensorData, SensorType}, 
//     sync_manager::{SyncManager, SyncMode}, 
//     transmitter::Transmitter,
// }};

// use rts_simulation::utils::metrics::{Metrics, EventRecorder};
// use rts_simulation::component_b::feedback::Feedback;

// use std::{
//     collections::{HashMap, VecDeque},
//     time::Instant,
//     hint::black_box,
//     sync::{Arc},
//     };

// fn bench_processing(c:&mut Criterion){
//     let sync = Arc::new(SyncManager::new(SyncMode::LockFree));
//     let event_recorder = Arc::new(EventRecorder::new());
//     let (_feedback_tx, feedback_rx) = crossbeam::channel::unbounded::<Feedback>();

//     let rx = crossbeam::channel::unbounded::<SensorData>().1;
//     let (tx, _) = crossbeam::channel::unbounded::<ProcessedPacket>();
//     let transmiterr = Arc::new(Transmitter::new(tx, 100, sync.clone()));
//     let metrics = Arc::new(std::sync::Mutex::new(Metrics::default()));
    
//     let processor = Processor::new(
//         rx,               
//         feedback_rx,       
//         10,                
//         3.0,               
//         5000,              
//         200,               
//         sync,              
//         transmiterr,       
//         metrics,           
//         event_recorder,    
        
//     );

//     let mut _buffers: HashMap<SensorType, VecDeque<f64>> = HashMap::new();

//     let sample = SensorData{
//         sensor_type: SensorType::Force,
//         reading: 100.0,
//         timestamp: Instant::now(),
//         seq: 1,
//     };


//     c.bench_function("processor_filter_anomaly_latency", |b| {
//         b.iter(|| {
//             //Fresh buffer per iteration 
//             let mut buffers: HashMap<SensorType, VecDeque<f64>> = HashMap::new();

//             let start = Instant::now();
//             let _ = processor.process_data(
//                 black_box(&sample),
//                 black_box(&mut buffers),
//             );
//             let elapsed_us = start.elapsed().as_micros();

//             black_box(elapsed_us);
//         })
//     });
// }


//     criterion_group!(benches, bench_processing);
//     criterion_main!(benches);



use crossbeam::channel::{bounded};
use criterion::{criterion_group, criterion_main, Criterion, BatchSize};
use std::{
    hint::black_box,
    time::{Duration, Instant},
    sync::Arc,
};

// Assuming these paths remain the same from your crate
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
    
    // Reasonable defaults for 2026 hardware to avoid long warmup hangs
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(50);

    group.bench_function("recv_latency", |b| {
        // Setup: This runs once per iteration batch
        b.iter_batched(
            || {
                let sync = Arc::new(SyncManager::new(SyncMode::Atomics));
                let event_recorder = Arc::new(EventRecorder::new());
                
                // Use a slightly larger bound to prevent immediate blocking
                let (tx, rx) = bounded(10); 

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
                // Timing Loop: We only time the actual send and process cycle
                tx.send(pkt).expect("Channel should not be full");
                
                // CRITICAL: receiver.run() must be non-blocking or 
                // modified to process only the available packet.
                black_box(receiver.run()); 
            },
            BatchSize::SmallInput
        )
    });

    group.finish();
}

criterion_group!(benches, receiver_latency_bench);
criterion_main!(benches);

