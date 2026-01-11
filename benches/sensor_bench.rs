/*
This benchmark measures sampling jitter for a 5 ms sensor loop by directly benchmarking the 
same timing primitives used in the Sensor module, comparing OS thread::sleep against SpinSleeper
 under identical conditions.
*/

use criterion::{
    criterion_group,
    criterion_main,
    Criterion,
    BenchmarkId,
    
};

use std::{
    hint::black_box,
    time::{Instant,Duration},
};
use spin_sleep::{SpinSleeper, SpinStrategy};

//Import sensor logic
use rts_simulation::component_a::sensor::{
    spin_sleep_tick,
    thread_sleep_tick,
};

const TARGET_PERIOD_US: u64 = 5_000;// Target sampling period: 

//Number of samples per run (large enough to expose jitter)
const SAMPLES: usize = 2_000;

fn bench_sensor_sampling_stability(c: &mut Criterion) {
    let mut group = c.benchmark_group("sensor_sampling_stability");

     group.sample_size(10);
     group.measurement_time(Duration::from_secs(10));

    //os schedular based sleep
    group.bench_function(
        BenchmarkId::new("thread_sleep", "5ms"),
        |b| {
            b.iter(|| {
                let mut last = Instant::now();
                let mut jitter = Vec::with_capacity(SAMPLES);

                for _ in 0..SAMPLES {
                    //measures deviation from 5 ms target
                    let j = thread_sleep_tick(TARGET_PERIOD_US, &mut last);
                    jitter.push(j);
                }

                black_box(jitter);
            });
        },
    );

      // Sensor implementation: SpinSleeper pacing
    group.bench_function(
        BenchmarkId::new("spin_sleeper", "5ms"),
        |b| {
            let sleeper = SpinSleeper::new(100_000)
                .with_spin_strategy(SpinStrategy::YieldThread);

            b.iter(|| {
                let mut last = Instant::now();
                let mut jitter = Vec::with_capacity(SAMPLES);
                
                for _ in 0..SAMPLES {
                     //Exact same logic used by Sensor::run()
                    let j = spin_sleep_tick(TARGET_PERIOD_US, &sleeper, &mut last);
                    jitter.push(j);
                }

                black_box(jitter);
            });
        },
    );

    group.finish();
}

criterion_group!(benches, bench_sensor_sampling_stability);
criterion_main!(benches);
