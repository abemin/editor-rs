use criterion::{Criterion, criterion_group, criterion_main};
use editor_core::{Document, Eol, SelectionSet};

fn typing_in_large_document(c: &mut Criterion) {
    let mut doc = Document::new(
        &"set http-service \"HTTP\" 10.0.0.1 enable\n".repeat(240_000),
        Eol::Lf,
    );
    doc.set_selections(SelectionSet::single(5_000_000));
    c.bench_function("type one character in 10 MB", |b| {
        b.iter(|| doc.insert_text("x").unwrap())
    });
}

criterion_group!(benches, typing_in_large_document);
criterion_main!(benches);
