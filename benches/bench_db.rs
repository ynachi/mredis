use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, black_box};
use csv::ReaderBuilder;
use rayon::prelude::*;

use mredis::db::Storage;

const THREADS: usize = 16;

pub fn read_csv_file() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let file_path = Path::new("/Users/ynachi/codes/github.com/rust/mredis/benches/surnames.csv");
    let file = File::open(file_path)?;
    let mut rdr = ReaderBuilder::new().delimiter(b',').from_reader(file);
    let mut records = vec![];
    for result in rdr.records() {
        let record = result?;
        // let entry = db::CacheEntry::new(&record[0], &record[2], Instant::now());
        records.push((record[0].to_string(), record[2].to_string()));
    }
    Ok(records)
}

fn storage_write(test_data: &Vec<(String, String)>) -> Arc<Storage> {
    let test_size = test_data.len(); // Number of entries to insert and retrieve
    let threads = THREADS; // Example: Using 4 threads, adjust as needed

    // Sharded map
    let sharded_map = Arc::new(Storage::new(5_000_000, 16));
    test_data.par_chunks(test_size / threads).for_each(|chunk| {
        let map = Arc::clone(&sharded_map);
        chunk.iter().for_each(|(key, value)| {
            map.set_kv(key, value, Duration::from_millis(600));
        });
    });
    sharded_map
}

fn storage_read(test_data: &Vec<(String, String)>, map: &Arc<Storage>) {
    let test_size = test_data.len();
    let threads = THREADS;
    test_data.par_chunks(test_size / threads).for_each(|chunk| {
        let map = Arc::clone(map);
        chunk.iter().for_each(|(key, _)| {
            map.get_v(key);
        });
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let test_data = read_csv_file().unwrap();
    c.bench_function("sharded map write", |b| {
        b.iter(|| storage_write(black_box(&test_data)))
    });
    let map = storage_write(&test_data);
    c.bench_function("sharded map read", |b| {
        b.iter(|| storage_read(black_box(&test_data), black_box(&map)))
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(100) // Set your parameters here
        .measurement_time(std::time::Duration::new(100, 800));
    targets = criterion_benchmark
);

criterion_main!(benches);
