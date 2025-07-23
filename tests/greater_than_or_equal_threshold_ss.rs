use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use scuttlebutt::{AesRng, Channel};
use counttree::data_structures::modint::ModInt;
use counttree::garbled_circuits::greater_than_or_equal_threshold::{
    multiple_gb_greater_than_ss, multiple_ev_greater_than_ss
};

#[test]
fn test_greater_than_or_equal_threshold_ss() {
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
                let expected = sum_mod >= t;
                
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

        let (result_sender, result_receiver) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let rng_gb = AesRng::new();
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            let garbler_results = multiple_gb_greater_than_ss(&mut rng_gb.clone(), &mut channel, &y_values, &t_values);
            result_sender.send(garbler_results).unwrap();
        });

        let rng_ev = AesRng::new();
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);

        let evaluator_results = multiple_ev_greater_than_ss(&mut rng_ev.clone(), &mut channel, &x_values);
        let garbler_results = result_receiver.recv().unwrap();

        // XOR the garbler and evaluator results to get the actual result
        let results: Vec<bool> = garbler_results
            .iter()
            .zip(evaluator_results.iter())
            .map(|(&garbler, &evaluator)| garbler ^ evaluator)
            .collect();
        
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
