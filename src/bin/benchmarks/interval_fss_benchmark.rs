use mosaic::{
    fss::interval::IntervalFSSKey,
    data_structures::ringvec::RingVec,
};
use std::time::Instant;
use rand::Rng;

fn main() {
    println!("IntervalFSS Key Generation Benchmark");
    println!("===================================");

    const N: usize = 2;
    let modulus = 1u128 << 20; // 2^20
    println!("Using modulus: 2^20 = {}", modulus);
    println!("Using N = {}", N);

    // Test different domain sizes (bit lengths)
    let domain_sizes = [8, 12, 16, 20, 24, 28, 32];
    
    for &domain_size in &domain_sizes {
        println!("\n--- Domain size: {} bits ---", domain_size);
        
        // Generate random alpha and beta bit strings
        let mut rng = rand::rng();
        let mut alpha_bits: Vec<bool> = (0..domain_size).map(|_| rng.random()).collect();
        let mut beta_bits: Vec<bool> = (0..domain_size).map(|_| rng.random()).collect();

        if alpha_bits > beta_bits {
            (beta_bits, alpha_bits) = (alpha_bits, beta_bits);
        }
        
        // Generate random payload vectors
        let a = RingVec::<N>::random(modulus);
        let b = RingVec::<N>::random(modulus);
        let c = RingVec::<N>::random(modulus);
        
        // Benchmark key generation
        let iterations = if domain_size <= 16 { 1000 } else if domain_size <= 24 { 100 } else { 10 };
        
        let start = Instant::now();
        for _ in 0..iterations {
            let (_key0, _key1) = IntervalFSSKey::<N>::gen_interval_fss_key(
                &alpha_bits,
                &beta_bits,
                &a,
                &b,
                &c,
                modulus
            );
        }
        let keygen_duration = start.elapsed();
        
        println!("Key generation x {}: {:?}", iterations, keygen_duration);
        println!("Average per key generation: {:?}", keygen_duration / iterations);
        
        // Benchmark a single key generation to get detailed stats
        let start = Instant::now();
        let (key0, _key1) = IntervalFSSKey::<N>::gen_interval_fss_key(
            &alpha_bits,
            &beta_bits,
            &a,
            &b,
            &c,
            modulus
        );
        let single_keygen_duration = start.elapsed();
        
        println!("Single key generation: {:?}", single_keygen_duration);
        println!("Number of correction words: {}", key0.cor_words.len());
        
        // Benchmark key serialization
        let start = Instant::now();
        let key0_bytes = key0.to_bytes();
        let serialization_duration = start.elapsed();
        println!("Key serialization: {:?} ({} bytes)", serialization_duration, key0_bytes.len());
        
        // Benchmark key deserialization
        let start = Instant::now();
        let (_deserialized_key, _) = IntervalFSSKey::<N>::from_bytes(&key0_bytes, modulus);
        let deserialization_duration = start.elapsed();
        println!("Key deserialization: {:?}", deserialization_duration);
        
        // Benchmark key evaluation
        let eval_iterations = if domain_size <= 20 { 10000 } else { 1000 };
        let test_input: Vec<bool> = (0..domain_size).map(|_| rng.gen()).collect();
        
        let start = Instant::now();
        for _ in 0..eval_iterations {
            let _result = key0.eval_interval_fss(&test_input, modulus);
        }
        let eval_duration = start.elapsed();
        
        println!("Key evaluation x {}: {:?}", eval_iterations, eval_duration);
        println!("Average per evaluation: {:?}", eval_duration / eval_iterations);
        
        // Memory usage estimation
        let key_size_bytes = key0_bytes.len();
        println!("Key size: {} bytes ({:.2} KB)", key_size_bytes, key_size_bytes as f64 / 1024.0);
    }
    
    // Benchmark different payload vector sizes
    println!("\n=== Payload Vector Size Impact ===");
    let domain_size = 16;
    let payload_sizes = [1, 2, 4, 8, 16];
    
    for &payload_size in &payload_sizes {
        println!("\n--- Payload vector size: {} ---", payload_size);
        
        let mut rng = rand::thread_rng();
        let mut alpha_bits: Vec<bool> = (0..domain_size).map(|_| rng.random()).collect();
        let mut beta_bits: Vec<bool> = (0..domain_size).map(|_| rng.random()).collect();
        
        // Ensure alpha < beta
        for i in 0..domain_size {
            if alpha_bits[i] > beta_bits[i] {
                (beta_bits[i], alpha_bits[i]) = (alpha_bits[i], beta_bits[i]);
            }
        }
        
        let iterations = 10000;
        
        match payload_size {
            1 => {
                let a = RingVec::<1>::random(modulus);
                let b = RingVec::<1>::random(modulus);
                let c = RingVec::<1>::random(modulus);
                
                let start = Instant::now();
                for _ in 0..iterations {
                    let (_key0, _key1) = IntervalFSSKey::<1>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
                }
                let duration = start.elapsed();
                println!("Key generation x {}: {:?}", iterations, duration);
                println!("Average: {:?}", duration / iterations);
            },
            2 => {
                let a = RingVec::<2>::random(modulus);
                let b = RingVec::<2>::random(modulus);
                let c = RingVec::<2>::random(modulus);
                
                let start = Instant::now();
                for _ in 0..iterations {
                    let (_key0, _key1) = IntervalFSSKey::<2>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
                }
                let duration = start.elapsed();
                println!("Key generation x {}: {:?}", iterations, duration);
                println!("Average: {:?}", duration / iterations);
            },
            4 => {
                let a = RingVec::<4>::random(modulus);
                let b = RingVec::<4>::random(modulus);
                let c = RingVec::<4>::random(modulus);
                
                let start = Instant::now();
                for _ in 0..iterations {
                    let (_key0, _key1) = IntervalFSSKey::<4>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
                }
                let duration = start.elapsed();
                println!("Key generation x {}: {:?}", iterations, duration);
                println!("Average: {:?}", duration / iterations);
            },
            8 => {
                let a = RingVec::<8>::random(modulus);
                let b = RingVec::<8>::random(modulus);
                let c = RingVec::<8>::random(modulus);
                
                let start = Instant::now();
                for _ in 0..iterations {
                    let (_key0, _key1) = IntervalFSSKey::<8>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
                }
                let duration = start.elapsed();
                println!("Key generation x {}: {:?}", iterations, duration);
                println!("Average: {:?}", duration / iterations);
            },
            16 => {
                let a = RingVec::<16>::random(modulus);
                let b = RingVec::<16>::random(modulus);
                let c = RingVec::<16>::random(modulus);
                
                let start = Instant::now();
                for _ in 0..iterations {
                    let (_key0, _key1) = IntervalFSSKey::<16>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
                }
                let duration = start.elapsed();
                println!("Key generation x {}: {:?}", iterations, duration);
                println!("Average: {:?}", duration / iterations);
            },
            _ => {}
        }
    }
    
    // Memory allocation pattern benchmark
    println!("\n=== Memory Allocation Pattern ===");
    let domain_size = 16;
    let iterations = 1000;
    
    let mut rng = rand::rng();
    let mut alpha_bits: Vec<bool> = (0..domain_size).map(|_| rng.gen()).collect();
    let mut beta_bits: Vec<bool> = (0..domain_size).map(|_| rng.gen()).collect();

    for i in 0..domain_size {
        if alpha_bits[i] > beta_bits[i] {
            (beta_bits[i], alpha_bits[i]) = (alpha_bits[i], beta_bits[i]);
        }
    }
        
    
    // Pre-allocated vectors
    let a = RingVec::<N>::random(modulus);
    let b = RingVec::<N>::random(modulus);
    let c = RingVec::<N>::random(modulus);
    
    let start = Instant::now();
    for _ in 0..iterations {
        let (_key0, _key1) = IntervalFSSKey::<N>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus);
    }
    let pre_alloc_duration = start.elapsed();
    
    // Fresh allocation each time
    let start = Instant::now();
    for _ in 0..iterations {
        let a_fresh = RingVec::<N>::random(modulus);
        let b_fresh = RingVec::<N>::random(modulus);
        let c_fresh = RingVec::<N>::random(modulus);
        let (_key0, _key1) = IntervalFSSKey::<N>::gen_interval_fss_key(&alpha_bits, &beta_bits, &a_fresh, &b_fresh, &c_fresh, modulus);
    }
    let fresh_alloc_duration = start.elapsed();
    
    println!("Pre-allocated vectors: {:?}", pre_alloc_duration);
    println!("Fresh allocation each time: {:?}", fresh_alloc_duration);

    println!("\nBenchmark completed!");
}