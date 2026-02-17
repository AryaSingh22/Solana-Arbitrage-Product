use criterion::{black_box, criterion_group, criterion_main, Criterion};
use solana_arb_core::{
    events::{EventBus, TradingEvent},
    rate_limiter::RateLimiter,
};
use tokio::runtime::Runtime;

fn benchmark_rate_limiter_acquire(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    // High limit to measure acquire overhead without sleeping
    let limiter = RateLimiter::per_second(1_000_000);

    c.bench_function("rate_limiter_acquire_async", |b| {
        b.to_async(&rt).iter(|| async {
            limiter.acquire().await;
        })
    });
}

fn benchmark_rate_limiter_try_acquire(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let limiter = RateLimiter::per_second(1_000_000);

    c.bench_function("rate_limiter_try_acquire_async", |b| {
        b.to_async(&rt).iter(|| async {
            let _ = limiter.try_acquire().await;
        })
    });
}

fn benchmark_event_bus_publish(c: &mut Criterion) {
    let event_bus = EventBus::new(10000);
    // Add a subscriber so publish has work to do (iterating subscribers)
    let _rx = event_bus.subscribe();

    c.bench_function("event_bus_publish", |b| {
         b.iter(|| {
             event_bus.publish(black_box(TradingEvent::SystemStarted {
                 mode: "benchmark".to_string(),
             }));
         })
    });
}

criterion_group!(benches, benchmark_rate_limiter_acquire, benchmark_rate_limiter_try_acquire, benchmark_event_bus_publish);
criterion_main!(benches);
