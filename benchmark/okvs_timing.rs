use std::time::Instant;
use counttree::okvs_f2k::RbOkvsF2k;
use rand::Rng;

fn main() {
    println!("=== OKVS Simple Benchmark ===");
    
    // Your requested parameters
    let kv_count = 20;
    let band_width = 40;
    let columns = 41;
    
    // Create OKVS instance
    let r1: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let r2: [u8; 16] = [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
    
    let okvs: RbOkvsF2k<16> = RbOkvsF2k::new(kv_count, columns, band_width, &r1, &r2);
    
    // Generate test data
    let mut rng = rand::thread_rng();
    let mut keys = Vec::new();
    let mut values = Vec::new();
    
    for i in 0..kv_count {
        let key_length = rng.gen_range(20..40);
        let key: Vec<bool> = (0..key_length).map(|_| rng.gen_bool(0.5)).collect();
        let value: u128 = (i as u128) * 12345 + 67890;
        
        keys.push(key);
        values.push(value);
    }
    
    println!("Configuration:");
    println!("  Key-Value pairs: {}", kv_count);
    println!("  Band width: {}", band_width);
    println!("  Columns: {}", columns);
    println!("  Average key length: {:.1}", keys.iter().map(|k| k.len()).sum::<usize>() as f64 / keys.len() as f64);
    println!();
    
    // Benchmark encoding
    println!("Starting encoding benchmark...");
    let start_encode = Instant::now();
    let encoding = match okvs.encode(&keys, &values) {
        Ok(enc) => enc,
        Err(e) => {
            println!("❌ Encoding failed: {}", e);
            return;
        }
    };
    let encode_duration = start_encode.elapsed();
    
    println!("✅ Encoding completed successfully");
    println!("   Encoding time: {:?}", encode_duration);
    println!("   Encoding length: {}", encoding.len());
    println!("   Throughput: {:.2} pairs/ms", kv_count as f64 / encode_duration.as_millis() as f64);
    println!();
    
    // Benchmark decoding
    println!("Starting decoding benchmark...");
    let start_decode = Instant::now();
    let decoded_values = okvs.decode(&encoding, &keys);
    let decode_duration = start_decode.elapsed();
    
    println!("✅ Decoding completed successfully");
    println!("   Decoding time: {:?}", decode_duration);
    println!("   Decoded {} values", decoded_values.len());
    println!("   Throughput: {:.2} pairs/ms", kv_count as f64 / decode_duration.as_millis() as f64);
    println!();
    
    // Verify correctness
    let mut all_correct = true;
    for i in 0..kv_count {
        if decoded_values[i] != values[i] {
            println!("❌ Mismatch at index {}: expected {}, got {}", i, values[i], decoded_values[i]);
            all_correct = false;
        }
    }
    
    if all_correct {
        println!("✅ All values decoded correctly!");
    }
    
    // Summary
    let total_time = encode_duration + decode_duration;
    println!();
    println!("=== Summary ===");
    println!("Encoding time: {:?}", encode_duration);
    println!("Decoding time: {:?}", decode_duration);
    println!("Total time: {:?}", total_time);
    println!("End-to-end throughput: {:.2} pairs/ms", kv_count as f64 / total_time.as_millis() as f64);
    
    // Performance ratios
    if decode_duration.as_nanos() > 0 {
        let ratio = encode_duration.as_nanos() as f64 / decode_duration.as_nanos() as f64;
        println!("Encoding is {:.1}x slower than decoding", ratio);
    }
}
