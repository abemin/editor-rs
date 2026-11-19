use criterion::{Criterion, criterion_group, criterion_main};
use editor_search::{Query, SearchOptions, SearchText, find_all};
use std::sync::atomic::AtomicBool;

fn regex_find_all(c: &mut Criterion) {
    let rope =
        ropey::Rope::from_str(&"set http-service \"HTTP\" 10.0.0.1 enable\n".repeat(240_000));
    let text = SearchText::new(&rope, &[]);
    let query = Query::compile(
        "e(n)able$",
        &SearchOptions {
            regex: true,
            ..Default::default()
        },
    )
    .unwrap();
    c.bench_function("regex find all in 10 MB", |b| {
        b.iter(|| find_all(&query, &text, None, &AtomicBool::new(false)).unwrap())
    });
}

criterion_group!(benches, regex_find_all);
criterion_main!(benches);
