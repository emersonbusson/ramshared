use criterion::{Criterion, criterion_group, criterion_main};
use ramshared_agent::swap::nbd_args;
use ramshared_broker::protocol::NbdEndpoint;
use std::hint::black_box;

pub fn bench_nbd_args(c: &mut Criterion) {
    let endpoint_unix = NbdEndpoint::Unix {
        path: "/run/ramshared/s0.sock".into(),
    };
    let endpoint_tcp = NbdEndpoint::Tcp {
        host: "192.168.1.100".into(),
        port: 10809,
    };

    c.bench_function("nbd_args_unix", |b| {
        b.iter(|| {
            nbd_args(
                black_box(&endpoint_unix),
                black_box("s0"),
                black_box("/dev/nbd0"),
            )
        })
    });

    c.bench_function("nbd_args_tcp", |b| {
        b.iter(|| {
            nbd_args(
                black_box(&endpoint_tcp),
                black_box("s0"),
                black_box("/dev/nbd0"),
            )
        })
    });
}

criterion_group!(benches, bench_nbd_args);
criterion_main!(benches);
