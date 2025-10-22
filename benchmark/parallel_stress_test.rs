use mosaic::okvs_f2k::RbOkvsF2k;
use std::time::Instant;
use rand::Rng;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::env;

fn main() {
    println!("=== OKVS Parallel Stress Test Benchmark ===");
    
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!("Usage: {} <kv_count> <band_width> <columns> <total_runs>", args[0]);
        eprintln!("Example: {} 20 40 41 1048576", args[0]);
        eprintln!("         {} 20 40 41 $((2**20))", args[0]);
        std::process::exit(1);
    }
    
    // Parse parameters from command line
    let kv_count: usize = args[1].parse().unwrap_or_else(|_| {
        eprintln!("Error: kv_count must be a positive integer");
        std::process::exit(1);
    });
    
    let band_width: usize = args[2].parse().unwrap_or_else(|_| {
        eprintln!("Error: band_width must be a positive integer");
        std::process::exit(1);
    });
    
    let columns: usize = args[3].parse().unwrap_or_else(|_| {
        eprintln!("Error: columns must be a positive integer");
        std::process::exit(1);
    });
    
    let total_runs: usize = args[4].parse().unwrap_or_else(|_| {
        eprintln!("Error: total_runs must be a positive integer");
        std::process::exit(1);
    });
    
    // Validate parameters
    if kv_count == 0 || band_width == 0 || columns == 0 || total_runs == 0 {
        eprintln!("Error: All parameters must be positive integers");
        std::process::exit(1);
    }
    
    if columns <= kv_count {
        eprintln!("Warning: columns ({}) should typically be greater than kv_count ({}) for better success rates", columns, kv_count);
    }
    
    let chunk_size = std::cmp::min(10000, total_runs / 100).max(1); // Adaptive chunk size
    
    println!("Configuration:");
    println!("  Key-Value pairs: {}", kv_count);
    println!("  Band width: {}", band_width);
    println!("  Columns: {}", columns);
    println!("  Total runs: {}", total_runs);
    if total_runs == 1048576 {
        println!("    (2^20 = {})", total_runs);
    } else if total_runs.is_power_of_two() {
        let power = total_runs.trailing_zeros();
        println!("    (2^{} = {})", power, total_runs);
    }
    println!("  Parallel processing: {} threads", rayon::current_num_threads());
    println!("  Chunk size: {}", chunk_size);
    println!();
    
    // Shared counters for thread-safe access
    let success_count = Arc::new(AtomicUsize::new(0));
    let failure_count = Arc::new(AtomicUsize::new(0));
    let total_encode_time = Arc::new(Mutex::new(std::time::Duration::new(0, 0)));
    
    // Create progress bar
    let num_chunks = (total_runs + chunk_size - 1) / chunk_size;
    let pb = ProgressBar::new(num_chunks as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>4}/{len:4} chunks ({eta}) {msg}"
        )
        .unwrap()
        .progress_chars("##-")
    );
    pb.set_message("Running parallel OKVS stress test...");
    
    println!("Starting parallel stress test...");
    let overall_start = Instant::now();
    
    // Create chunks and process them in parallel
    let chunks: Vec<usize> = (0..total_runs).step_by(chunk_size).collect();
    
    chunks.par_iter().enumerate().for_each(|(chunk_idx, &start_run)| {
        // Each thread gets its own RNG
        let rng = rand::rng();
        
        let end_run = std::cmp::min(start_run + chunk_size, total_runs);
        let chunk_successes = Arc::new(AtomicUsize::new(0));
        let chunk_failures = Arc::new(AtomicUsize::new(0));
        let chunk_time = Arc::new(Mutex::new(std::time::Duration::new(0, 0)));
        
        // Process this chunk
        (start_run..end_run).into_par_iter().for_each(|run| {
            // Generate fresh seeds for each run
            let mut local_rng = rand::rng();
            let mut r1: [u8; 16] = [0; 16];
            let mut r2: [u8; 16] = [0; 16];
            local_rng.fill(&mut r1);
            local_rng.fill(&mut r2);
            
            // Create OKVS instance with fresh seeds for this run
            let okvs: RbOkvsF2k<u128> = RbOkvsF2k::new(kv_count, columns, band_width, &r1, &r2);
            
            // Generate fresh test data for each run
            let mut keys = Vec::new();
            let mut values = Vec::new();
            
            // Generate test data for this run
            
            for i in 0..kv_count {
                let key_length = local_rng.random_range(20..40);
                let key: Vec<bool> = (0..key_length).map(|_| local_rng.random_bool(0.5)).collect();
                let value: u128 = (i as u128) * 12345 + (run as u128) * 67890 + 123456789;
                
                keys.push(key);
                values.push(value);
            }
            
            // Attempt encoding and measure time
            let encode_start = Instant::now();
            match okvs.encode(&keys, &values) {
                Ok(_) => {
                    chunk_successes.fetch_add(1, Ordering::Relaxed);
                    let elapsed = encode_start.elapsed();
                    let mut time_guard = chunk_time.lock().unwrap();
                    *time_guard += elapsed;
                }
                Err(_) => {
                    chunk_failures.fetch_add(1, Ordering::Relaxed);
                    let elapsed = encode_start.elapsed();
                    let mut time_guard = chunk_time.lock().unwrap();
                    *time_guard += elapsed;
                }
            }
        });
        
        // Update global counters
        success_count.fetch_add(chunk_successes.load(Ordering::Relaxed), Ordering::Relaxed);
        failure_count.fetch_add(chunk_failures.load(Ordering::Relaxed), Ordering::Relaxed);
        
        {
            let mut global_time = total_encode_time.lock().unwrap();
            let chunk_time_value = chunk_time.lock().unwrap();
            *global_time += *chunk_time_value;
        }
        
        // Update progress bar
        pb.inc(1);
        let current_successes = success_count.load(Ordering::Relaxed);
        let current_failures = failure_count.load(Ordering::Relaxed);
        let current_total = current_successes + current_failures;
        
        if current_total > 0 {
            let success_rate = (current_successes as f64 / current_total as f64) * 100.0;
            pb.set_message(format!(
                "Success: {:.2}% | Processed: {} | Failures: {}", 
                success_rate, current_total, current_failures
            ));
        }
    });
    
    // Finish the progress bar
    let final_successes = success_count.load(Ordering::Relaxed);
    let final_failures = failure_count.load(Ordering::Relaxed);
    pb.finish_with_message(format!("Completed! Success: {}, Failures: {}", final_successes, final_failures));
    
    let total_time = overall_start.elapsed();
    let total_encode_time_final = *total_encode_time.lock().unwrap();
    
    println!("\n=== Parallel Stress Test Results ===");
    println!("Total runs: {}", total_runs);
    println!("Successful encodings: {}", final_successes);
    println!("Failed encodings: {}", final_failures);
    println!("Success rate: {:.6}% ({}/{}) ", 
             (final_successes as f64 / total_runs as f64) * 100.0,
             final_successes, total_runs);
    
    if final_failures > 0 {
        let log2_failure_metric = (total_runs as f64 / final_failures as f64).log2();
        println!("Failure rate: {:.6}% ({}/{}) | log2(runs/failures): {:.2}", 
                 (final_failures as f64 / total_runs as f64) * 100.0,
                 final_failures, total_runs, log2_failure_metric);
    } else {
        println!("Failure rate: 0.000000% (0/{}) | log2(runs/failures): ∞ (no failures)", total_runs);
    }
    println!();
    
    println!("=== Performance Statistics ===");
    println!("Total test time: {:?}", total_time);
    println!("Average time per run: {:?}", total_time / total_runs as u32);
    println!("Total encoding time: {:?}", total_encode_time_final);
    if final_successes > 0 {
        println!("Average encoding time (successful): {:?}", total_encode_time_final / final_successes as u32);
    }
    println!("Throughput: {:.2} runs/second", total_runs as f64 / total_time.as_secs_f64());
    println!("Parallelization speedup: ~{}x (estimated)", rayon::current_num_threads());
    println!();
    
    println!("=== Reliability Analysis ===");
    if final_failures == 0 {
        println!("🎉 Perfect reliability! No failures in {} runs", total_runs);
        println!("   This suggests the OKVS parameters are well-chosen");
    } else {
        println!("⚠️  Failures detected:");
        println!("   Failure probability: {:.2e}", final_failures as f64 / total_runs as f64);
        println!("   Expected failures per million runs: {:.0}", 
                 (final_failures as f64 / total_runs as f64) * 1_000_000.0);
        
        if final_failures as f64 / total_runs as f64 > 0.01 {
            println!("   🚨 High failure rate! Consider adjusting parameters:");
            println!("      - Increase columns relative to kv_count");
            println!("      - Adjust band_width");
            println!("      - Check for implementation issues");
        } else if final_failures as f64 / total_runs as f64 > 0.001 {
            println!("   ⚠️  Moderate failure rate - may need parameter tuning");
        } else {
            println!("   ✅ Low failure rate - parameters seem reasonable");
        }
    }
    
    // Statistical significance note
    println!();
    println!("=== Statistical Notes ===");
    if final_failures > 0 {
        let confidence_interval = 1.96 * ((final_failures as f64 / total_runs as f64) * 
                                         (1.0 - final_failures as f64 / total_runs as f64) / 
                                         total_runs as f64).sqrt();
        println!("95% confidence interval for failure rate: ±{:.2e}", confidence_interval);
    }
    println!("Sample size: {} runs provides good statistical power", total_runs);
    println!("Parallel execution with {} threads", rayon::current_num_threads());
    
    if total_runs >= 1_000_000 {
        println!("Large sample size enables detection of rare failure modes");
    }
}
