use criterion::{Criterion, criterion_group, criterion_main};

fn open_large_file(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.txt");
    std::fs::write(
        &path,
        "set http-service \"HTTP\" 10.0.0.1 enable\n".repeat(240_000),
    )
    .unwrap();
    c.bench_function("open 10 MB text file", |b| {
        b.iter(|| editor_formats::load_file(&path).unwrap())
    });
}

criterion_group!(benches, open_large_file);
criterion_main!(benches);
