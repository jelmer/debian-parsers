use criterion::{criterion_group, criterion_main, Criterion};

#[cfg(feature = "deb822-fast")]
use deb822_fast::Deb822;

#[cfg(feature = "deb822-fast")]
fn parse_deb822_benchmark(c: &mut Criterion) {
    let control_data =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/bench/Sources"))
            .expect("Could not read control file");

    c.bench_function("parse_deb822_lossy", |b| {
        b.iter(|| {
            let _deb822: Deb822 = control_data.parse().unwrap();
        });
    });
}

#[cfg(not(feature = "deb822-fast"))]
fn parse_deb822_benchmark(_c: &mut Criterion) {
    // Skip benchmark when deb822-fast feature is not enabled
}

criterion_group!(benches, parse_deb822_benchmark);
criterion_main!(benches);
