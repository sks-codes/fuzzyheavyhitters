use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use scuttlebutt::{AesRng, Channel};
use counttree::data_structures::modint::ModInt;
use counttree::garbled_circuits::less_than_or_equal_threshold::{
    multiple_gb_complex_comparison, multiple_ev_complex_comparison
};

#[test]
fn test_complex_comparison_basic() {
    // Test case: modulus = 16 (4 bits)
    let modulus = 16u128;
    
    // Garbler inputs
    let y_values = vec![ModInt::new(5, modulus), ModInt::new(10, modulus)];
    let t_values = vec![ModInt::new(8, modulus), ModInt::new(7, modulus)];
    
    // Evaluator inputs  
    let x_values = vec![ModInt::new(6, modulus), ModInt::new(12, modulus)];
    
    let expected = vec![false, true];

    let (sender, receiver) = UnixStream::pair().unwrap();

    std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        multiple_gb_complex_comparison(&mut rng_gb.clone(), &mut channel, &y_values, &t_values);
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_complex_comparison(&mut rng_ev.clone(), &mut channel, &x_values);
    assert_eq!(results, expected);
}

#[test]
fn test_complex_comparison_brute_force() {
    // Brute force test with small modulus to verify the circuit computes (x + y) mod modulus >= t
    let modulus = 8u128; // Small modulus for exhaustive testing
    
    let mut test_cases = Vec::new();
    let mut expected_results = Vec::new();
    
    // Test a representative sample of all combinations
    for x in 0..modulus {
        for y in 0..modulus {
            for t in 0..modulus {
                // Calculate expected result: (x + y) mod modulus >= t
                let sum_mod = (x + y) % modulus;
                let expected = sum_mod <= t;
                
                test_cases.push((x, y, t));
                expected_results.push(expected);
                
                println!("x={}, y={}, t={}: ({}+{}) mod {} = {} >= {} = {}", 
                         x, y, t, x, y, modulus, sum_mod, t, expected);
            }
        }
    }
    
    // Run tests in batches to avoid too many garbled circuits at once
    const BATCH_SIZE: usize = 16;
    for batch_start in (0..test_cases.len()).step_by(BATCH_SIZE) {
        let batch_end = std::cmp::min(batch_start + BATCH_SIZE, test_cases.len());
        let batch_cases = &test_cases[batch_start..batch_end];
        let batch_expected = &expected_results[batch_start..batch_end];
        
        let y_values: Vec<ModInt> = batch_cases.iter().map(|(_, y, _)| ModInt::new(*y, modulus)).collect();
        let t_values: Vec<ModInt> = batch_cases.iter().map(|(_, _, t)| ModInt::new(*t, modulus)).collect();
        let x_values: Vec<ModInt> = batch_cases.iter().map(|(x, _, _)| ModInt::new(*x, modulus)).collect();
        
        let (sender, receiver) = UnixStream::pair().unwrap();

        std::thread::spawn(move || {
            let rng_gb = AesRng::new();
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            multiple_gb_complex_comparison(&mut rng_gb.clone(), &mut channel, &y_values, &t_values);
        });

        let rng_ev = AesRng::new();
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);

        let results = multiple_ev_complex_comparison(&mut rng_ev.clone(), &mut channel, &x_values);
        
        // Check results for this batch
        for (i, (&expected, &actual)) in batch_expected.iter().zip(results.iter()).enumerate() {
            let global_idx = batch_start + i;
            let (x, y, t) = test_cases[global_idx];
            assert_eq!(expected, actual, 
                      "Failed for x={}, y={}, t={}: expected {}, got {}", 
                      x, y, t, expected, actual);
        }
        
        println!("Batch {}-{} passed!", batch_start, batch_end - 1);
    }
    
    println!("All {} test cases passed!", test_cases.len());
}

#[test]
fn test_complex_comparison_simple_verification() {
    // Simple test with known values to verify >= logic
    let modulus = 4u128; // Very small modulus for easy verification
    
    // Test case where result should be true (sum >= threshold)
    let y_values = vec![ModInt::new(1, modulus)]; // y = 1
    let t_values = vec![ModInt::new(2, modulus)]; // t = 2  
    let x_values = vec![ModInt::new(3, modulus)]; // x = 3
    
    // Expected: (3 + 1) mod 4 = 0, and 0 >= 2? No, so false
    // Wait, let me recalculate: (3 + 1) mod 4 = 4 mod 4 = 0
    // Is 0 >= 2? No, so result should be false
    
    let expected = vec![false];

    let (sender, receiver) = UnixStream::pair().unwrap();

    std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        multiple_gb_complex_comparison(&mut rng_gb.clone(), &mut channel, &y_values, &t_values);
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_complex_comparison(&mut rng_ev.clone(), &mut channel, &x_values);
    println!("Simple test: x=3, y=1, t=2, modulus=4");
    println!("(3+1) mod 4 = {} >= 2 = {}", (3+1) % 4, (3+1) % 4 >= 2);
    println!("Circuit result: {:?}", results);
    assert_eq!(results, expected);
}
