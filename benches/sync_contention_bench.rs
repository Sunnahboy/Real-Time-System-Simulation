/*use critirion::{criterion_group, criterion_main, Criterion, BatchSize};
use rts_simulation::component_a::sync_manager::{SyncManager, SyncMode};
use std::hint::black_box;


fn bench_record_sample(c: &mut Criterion){
    let mut group = c.benchmark_group("Sync_record_sample");

    for mode in [SyncMode::Mutex, SyncMode::Atomics, SyncMode::LockFree]{
        group.bench_function(format!("record_sample_{:?}",mode), |b| {
            let sync = SyncManager::new(mode);
            b.iter_batched(
                || sync.clone(),
                |s| {s.record_sample(black_box(1));},
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();

}

fn bench_record_jitter(c:&mut Criterion){
    let mut group = c.benchmark_group("Sync_record_jitter");

    for mode in [SyncMode::Mutex, SyncMode::Atomics, SyncMode::LockFree]{
        group.bench_function(format!("record_jitter_{:?}", mode), |b|{
            let sync = SyncManager::new(mode);
            b.iter(|| sync.record_jitter(black_box(1), black_box(1200)));
        });
    }
    group.finish();
}

fn bench_proc_miss(c:&mut Criterion){
    let mut group = c.benchmark_group("Sync_proc_miss");

    for mode in [SyncMode::Mutex, SyncMode::Atomics,tics, SyncMode::LockFree]{
        group.bench_function(format!("record_proc_miss_{:?}", mode), |b|{
            let sync = SyncManager::new(mode);
            b.iter(|| sync.record_proc_miss());
        });
    }
    group.finish();
}

criterion_group!(
    benches, 
    bench_record_sample,
    bench_record_jitter,
    bench_proc_miss,
);
criterion_main!(benches);

*/


/*
This benchmark measures how three synchronization strategies (Mutex, Atomics, Lock-Free) 
behave under high thread contention by spawning multiple threads that repeatedly call record_sample() 
on a shared SyncManager, allowing  to compare their scalability, contention cost, 
and suitability for real-time sensor systems. 
*/

//macros and types  required to define and run benchmarks.
use criterion::{
    criterion_group,
    criterion_main,
    Criterion,
    //BatchSize,
    BenchmarkId,
};

use rts_simulation::component_a::{
    sync_manager::{SyncManager, SyncMode}
};
use std::{
    sync::Arc,
    thread,
    hint::black_box,
};

//Number of concurrent threads contending on the same SyncManager
const THREAD_COUNTS: &[usize] = &[2, 4, 8, 16];

//Operations each thread performs on the shared resource
const OPS_PER_THREAD: usize = 50_000;

fn bench_sync_contention(c: &mut Criterion) {
    // Group results together for easy comparison
    let mut group = c.benchmark_group("sync_contention_write");

    // Compare all synchronization strategies
    for mode in [SyncMode::Mutex, SyncMode::Atomics, SyncMode::LockFree] {
        for &threads in THREAD_COUNTS {

            group.bench_with_input(
                BenchmarkId::new(format!("{:?}", mode), threads),
                &threads,
                |b, &threads| {

                    //One shared SyncManager for all threads (contention source)
                    let sync = Arc::new(SyncManager::new(mode));
                    black_box(&sync);

                    b.iter(|| {
                        let mut handles = Vec::with_capacity(threads);

                        // Spawn competing threads
                        for _ in 0..threads {
                            let s = Arc::clone(&sync);

                            handles.push(thread::spawn(move || {
                                let sensor_id = black_box(1u16);

                                //Tight loop: maximum contention
                                for _ in 0..OPS_PER_THREAD {
                                    s.record_sample(sensor_id);
                                }
                            }));
                        }

                        //Ensure all threads finish before next iteration
                        for h in handles {
                            let _ = h.join();
                        }
                    });
                },
            );
        }
    }

    group.finish();
}

criterion_group!(benches, bench_sync_contention);
criterion_main!(benches);






