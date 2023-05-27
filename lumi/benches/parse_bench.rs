use criterion::{criterion_group, criterion_main, Criterion};
use lumi::Ledger;

fn parse_text_ledger(path: &str) -> Ledger {
    let (ledger, _) = Ledger::from_file(path);
    return ledger;
}

fn criterion_benchmark(c: &mut Criterion) {
    let input = std::env::var("LUMI_BENCH_INPUT").unwrap();
    c.bench_function("Parse text", |b| b.iter(|| parse_text_ledger(&input)));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
