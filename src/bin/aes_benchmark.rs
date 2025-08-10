use std::time::Instant;
use counttree::aes::FixedKeyPrgStream;
use rand_core::RngCore;

fn main() {
    println!("AES Stream Operations Benchmark");
    println!("==============================");

    let mut stream = FixedKeyPrgStream::new();
    let key = [0u8; 16]; // Test key
    let iterations = 1_000_000;

    // Benchmark set_key operation
    println!("\nBenchmarking set_key() operation...");
    let start = Instant::now();
    for _ in 0..iterations {
        stream.set_key(&key);
    }
    let set_key_duration = start.elapsed();
    println!("set_key() x {}: {:?}", iterations, set_key_duration);
    println!("Average per set_key(): {:?}", set_key_duration / iterations);

    // Benchmark refill operation
    println!("\nBenchmarking refill() operation...");
    stream.set_key(&key);
    let start = Instant::now();
    for _ in 0..iterations {
        stream.refill();
    }
    let refill_duration = start.elapsed();
    println!("refill() x {}: {:?}", iterations, refill_duration);
    println!("Average per refill(): {:?}", refill_duration / iterations);

    // Benchmark fill_bytes operation with different buffer sizes
    let buffer_sizes = [16, 32, 64, 128, 256, 512, 1024, 4096];
    
    for &size in &buffer_sizes {
        println!("\nBenchmarking fill_bytes() with {} byte buffer...", size);
        stream.set_key(&key);
        stream.refill();
        
        let mut buffer = vec![0u8; size];
        let test_iterations = 100_000;
        
        let start = Instant::now();
        for _ in 0..test_iterations {
            stream.fill_bytes(&mut buffer);
        }
        let fill_bytes_duration = start.elapsed();
        
        println!("fill_bytes({} bytes) x {}: {:?}", size, test_iterations, fill_bytes_duration);
        println!("Average per fill_bytes(): {:?}", fill_bytes_duration / test_iterations);
        println!("Throughput: {:.2} MB/s", 
                (size as f64 * test_iterations as f64) / (1024.0 * 1024.0) / fill_bytes_duration.as_secs_f64());
    }

    // Combined operation benchmark (realistic usage pattern)
    println!("\nBenchmarking combined operations (set_key + refill + fill_bytes)...");
    let combined_iterations = 10_000;
    let buffer_size = 256;
    let mut buffer = vec![0u8; buffer_size];
    
    let start = Instant::now();
    for i in 0..combined_iterations {
        // let start = Instant::now();
        let test_key = [(i % 256) as u8; 16]; // Different key each time
        stream.set_key(&test_key);
        // println!("Set key {} took {:?}", i + 1, start.elapsed());
        // let start = Instant::now();
        stream.refill();
        // println!("Refill took {:?}", start.elapsed());
        // let start = Instant::now();
        stream.fill_bytes(&mut buffer);
        // println!("Fill bytes took {:?}", start.elapsed());
        // println!("Combined operation {} took {:?}", i + 1, start.elapsed());
    }
    let combined_duration = start.elapsed();
    
    println!("Combined operations x {}: {:?}", combined_iterations, combined_duration);
    println!("Average per combined operation: {:?}", combined_duration / combined_iterations);

    // Memory allocation impact test
    println!("\nBenchmarking memory allocation impact...");
    let alloc_iterations = 50_000;
    stream.set_key(&key);
    
    // Pre-allocated buffer
    let mut pre_buffer = vec![0u8; 1024];
    let start = Instant::now();
    for _ in 0..alloc_iterations {
        stream.refill();
        stream.fill_bytes(&mut pre_buffer);
    }
    let pre_alloc_duration = start.elapsed();
    
    // Fresh allocation each time
    let start = Instant::now();
    for _ in 0..alloc_iterations {
        stream.refill();
        let mut fresh_buffer = vec![0u8; 1024];
        stream.fill_bytes(&mut fresh_buffer);
    }
    let fresh_alloc_duration = start.elapsed();
    
    println!("Pre-allocated buffer: {:?}", pre_alloc_duration);
    println!("Fresh allocation each time: {:?}", fresh_alloc_duration);
    println!("Allocation overhead: {:?}", fresh_alloc_duration - pre_alloc_duration);

    // Test thread-local performance impact
    println!("\nBenchmarking thread-local access pattern...");
    let tl_iterations = 100_000;
    let buffer_size = 128;
    
    let start = Instant::now();
    for _ in 0..tl_iterations {
        // Simulate the pattern used in gen_layer_data
        let mut buffer1 = vec![0u8; buffer_size];
        let mut buffer2 = vec![0u8; buffer_size]; 
        let mut buffer3 = vec![0u8; buffer_size];
        
        stream.set_key(&key);
        stream.refill();
        stream.fill_bytes(&mut buffer1);
        stream.refill();
        stream.fill_bytes(&mut buffer2);
        stream.refill();
        stream.fill_bytes(&mut buffer3);
    }
    let tl_duration = start.elapsed();
    
    println!("Thread-local pattern x {}: {:?}", tl_iterations, tl_duration);
    println!("Average per thread-local operation: {:?}", tl_duration / tl_iterations);

    println!("\nBenchmark completed!");
}