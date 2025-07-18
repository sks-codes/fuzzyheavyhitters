use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use std::thread;
use scuttlebutt::{AesRng, Channel};
use counttree::data_structures::modint::ModInt;
use counttree::garbled_circuits::less_than_or_equal_threshold::{
    multiple_gb_complex_comparison as gb_le, multiple_ev_complex_comparison as ev_le,
};
use counttree::garbled_circuits::greater_than_or_equal_threshold::{
    multiple_gb_complex_comparison as gb_ge, multiple_ev_complex_comparison as ev_ge,
};

/// Test the <= threshold comparison with various cases
#[test]
fn test_less_than_or_equal_threshold() {
    let modulus = 64u128; // Power of 2 for testing
    
    // Test cases: (x, y, t) -> expected result for (x + y) mod modulus <= t
    let test_cases = vec![
        (5, 10, 20, true),   // 15 <= 20 
        (10, 10, 20, true),  // 20 <= 20
        (15, 10, 20, false), // 25 <= 20
        (50, 20, 20, true),  // (50 + 20) mod 64 = 6 <= 20
        (30, 40, 20, true),  // (30 + 40) mod 64 = 6 <= 20
        (20, 50, 20, true), // (20 + 50) mod 64 = 6 <= 20
        (1, 1, 5, true),     // 2 <= 5
        (3, 3, 5, false),    // 6 <= 5
    ];
    
    for (i, (x_val, y_val, t_val, expected)) in test_cases.iter().enumerate() {
        println!("Test case {}: x={}, y={}, t={}, expected={}", i, x_val, y_val, t_val, expected);
        
        let x = ModInt::new(*x_val, modulus);
        let y = ModInt::new(*y_val, modulus);
        let t = ModInt::new(*t_val, modulus);
        
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut rng0 = AesRng::new();
        let mut rng1 = AesRng::new();
        
        let garbler_inputs_y = vec![y];
        let garbler_inputs_t = vec![t];
        let evaluator_inputs_x = vec![x];
        
        let handle = thread::spawn(move || {
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            gb_le(&mut rng0, &mut channel, &garbler_inputs_y, &garbler_inputs_t);
        });
        
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);
        let results = ev_le(&mut rng1, &mut channel, &evaluator_inputs_x);
        
        handle.join().unwrap();
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], *expected, 
            "Test case {} failed: x={}, y={}, t={}, got {}, expected {}", 
            i, x_val, y_val, t_val, results[0], expected);
    }
}

/// Test the >= threshold comparison with various cases  
#[test]
fn test_greater_than_or_equal_threshold() {
    let modulus = 64u128; // Power of 2 for testing
    
    // Test cases: (x, y, t) -> expected result for (x + y) mod modulus >= t
    let test_cases = vec![
        (5, 10, 20, false),  // 15 >= 20 
        (10, 10, 20, true),  // 20 >= 20
        (15, 10, 20, true),  // 25 >= 20
        (50, 20, 20, false), // (50 + 20) mod 64 = 6 >= 20
        (30, 40, 20, false), // (30 + 40) mod 64 = 6 >= 20
        (20, 50, 20, true),  // (20 + 50) mod 64 = 6 >= 20, but this is edge case
        (1, 1, 5, false),    // 2 >= 5
        (3, 3, 5, true),     // 6 >= 5
    ];
    
    for (i, (x_val, y_val, t_val, expected)) in test_cases.iter().enumerate() {
        println!("Test case {}: x={}, y={}, t={}, expected={}", i, x_val, y_val, t_val, expected);
        
        let x = ModInt::new(*x_val, modulus);
        let y = ModInt::new(*y_val, modulus);
        let t = ModInt::new(*t_val, modulus);
        
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut rng0 = AesRng::new();
        let mut rng1 = AesRng::new();
        
        let garbler_inputs_y = vec![y];
        let garbler_inputs_t = vec![t];
        let evaluator_inputs_x = vec![x];
        
        let handle = thread::spawn(move || {
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            gb_ge(&mut rng0, &mut channel, &garbler_inputs_y, &garbler_inputs_t);
        });
        
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);
        let results = ev_ge(&mut rng1, &mut channel, &evaluator_inputs_x);
        
        handle.join().unwrap();
        
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], *expected, 
            "Test case {} failed: x={}, y={}, t={}, got {}, expected {}", 
            i, x_val, y_val, t_val, results[0], expected);
    }
}

/// Test that <= and >= are complements of each other
#[test]
fn test_threshold_complementarity() {
    let modulus = 64u128;
    
    // Test various combinations
    let test_cases = vec![
        (5, 10, 20),
        (10, 10, 20),
        (15, 10, 20),
        (50, 20, 20),
        (30, 40, 20),
        (1, 1, 5),
        (3, 3, 5),
        (0, 0, 0),
        (60, 60, 59),
    ];
    
    for (x_val, y_val, t_val) in test_cases {
        let x = ModInt::new(x_val, modulus);
        let y = ModInt::new(y_val, modulus);
        let t = ModInt::new(t_val, modulus);
        
        // Test <= threshold
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut rng0 = AesRng::new();
        let mut rng1 = AesRng::new();
        
        let garbler_inputs_y = vec![y];
        let garbler_inputs_t = vec![t];
        let evaluator_inputs_x = vec![x];
        
        let handle = thread::spawn(move || {
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            gb_le(&mut rng0, &mut channel, &garbler_inputs_y, &garbler_inputs_t);
        });
        
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);
        let le_results = ev_le(&mut rng1, &mut channel, &evaluator_inputs_x);
        handle.join().unwrap();
        
        // Test >= threshold
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut rng0 = AesRng::new();
        let mut rng1 = AesRng::new();
        
        let garbler_inputs_y = vec![y];
        let garbler_inputs_t = vec![t];
        let evaluator_inputs_x = vec![x];
        
        let handle = thread::spawn(move || {
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            gb_ge(&mut rng0, &mut channel, &garbler_inputs_y, &garbler_inputs_t);
        });
        
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);
        let ge_results = ev_ge(&mut rng1, &mut channel, &evaluator_inputs_x);
        handle.join().unwrap();
        
        // For strict inequality, <= and >= should be complements
        // However, for the case where (x + y) mod modulus == t, both should be true
        let sum_mod = (x_val + y_val) % modulus;
        if sum_mod != t_val {
            assert_ne!(le_results[0], ge_results[0], 
                "For x={}, y={}, t={}, sum_mod={}, <= and >= should be complements", 
                x_val, y_val, t_val, sum_mod);
        } else {
            assert_eq!(le_results[0], true, 
                "For x={}, y={}, t={}, sum_mod={}, <= should be true", 
                x_val, y_val, t_val, sum_mod);
            assert_eq!(ge_results[0], true, 
                "For x={}, y={}, t={}, sum_mod={}, >= should be true", 
                x_val, y_val, t_val, sum_mod);
        }
    }
}

/// Test multiple inputs at once
#[test]
fn test_multiple_threshold_comparisons() {
    let modulus = 64u128;
    
    let x_values = vec![5, 10, 15, 50];
    let y_values = vec![10, 10, 10, 20];
    let t_values = vec![20, 20, 20, 20];
    
    let x_inputs: Vec<ModInt> = x_values.iter().map(|&x| ModInt::new(x, modulus)).collect();
    let y_inputs: Vec<ModInt> = y_values.iter().map(|&y| ModInt::new(y, modulus)).collect();
    let t_inputs: Vec<ModInt> = t_values.iter().map(|&t| ModInt::new(t, modulus)).collect();
    
    // Test <= threshold
    let (sender, receiver) = UnixStream::pair().unwrap();
    let mut rng0 = AesRng::new();
    let mut rng1 = AesRng::new();
    
    let garbler_inputs_y = y_inputs.clone();
    let garbler_inputs_t = t_inputs.clone();
    let evaluator_inputs_x = x_inputs.clone();
    
    let handle = thread::spawn(move || {
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        gb_le(&mut rng0, &mut channel, &garbler_inputs_y, &garbler_inputs_t);
    });
    
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);
    let le_results = ev_le(&mut rng1, &mut channel, &evaluator_inputs_x);
    handle.join().unwrap();
    
    // Expected results for <= comparison
    let expected_le = vec![true, true, false, true]; // 15<=20, 20<=20, 25<=20, 6<=20
    
    assert_eq!(le_results.len(), 4);
    for i in 0..4 {
        assert_eq!(le_results[i], expected_le[i], 
            "Multiple <= test failed at index {}: got {}, expected {}", 
            i, le_results[i], expected_le[i]);
    }
}
