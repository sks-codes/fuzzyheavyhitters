# OKVS Benchmarks

This folder contains benchmarks for the OKVS implementation.

## Benchmark Files

### 1. Simple Timing Benchmark (`okvs_timing.rs`)
Basic performance measurement using `std::time::Instant`.

**Run with:**
```bash
cargo run --bin okvs_benchmark --release
```

**What it measures:**
- Encoding time for 20 key-value pairs
- Decoding time
- Throughput (pairs/ms)
- Correctness verification

### 2. Parallel Stress Test (`parallel_stress_test.rs`)
Configurable reliability test that runs OKVS encoding with command-line parameters.

**Run with:**
```bash
cargo run --bin okvs_parallel_stress_test --release -- <kv_count> <band_width> <columns> <total_runs>
```

**Examples:**
```bash
# Standard test with 2^20 runs
cargo run --bin okvs_parallel_stress_test --release -- 20 40 41 1048576

# Quick test
cargo run --bin okvs_parallel_stress_test --release -- 10 20 25 1000

# Large-scale test with 2^22 runs
cargo run --bin okvs_parallel_stress_test --release -- 20 40 41 4194304
```

**Parameters:**
- `kv_count`: Number of key-value pairs to encode
- `band_width`: OKVS band width parameter
- `columns`: Number of columns in the OKVS matrix
- `total_runs`: Total number of encoding attempts

**What it measures:**
- Success/failure rate over many runs
- Statistical reliability analysis
- Performance under stress
- Confidence intervals for failure rates
- Real-time progress tracking

**Parallel version benefits:**
- Uses all CPU cores for faster execution
- Progress bar shows completion status
- Adaptive chunk sizing
- Real-time success rate updates

**Parameter Guidelines:**
- `columns` should typically be > `kv_count` for better success rates
- Higher `band_width` may improve success rates but increases computation
- `total_runs` of 2^20 (1,048,576) provides good statistical power

## Expected Results

### Simple Timing
```
Encoding time: ~200-500µs
Decoding time: ~10-50µs  
Throughput: ~40-100 pairs/ms
```

### Stress Test
```
Total runs: 1,048,576
Success rate: 99.9%+ (hopefully!)
Failure rate: <0.1%
```

## Notes

- Always use `--release` flag for accurate measurements
- Stress test shows progress updates every 1%
- Results will vary based on hardware and system load
- High failure rates may indicate need for parameter tuning
