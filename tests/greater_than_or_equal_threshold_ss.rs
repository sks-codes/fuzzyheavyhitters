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
    
    
    for t in 0..modulus {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        let mut xy = Vec::new();
        let mut expected_results = Vec::new();
        for x in 0..modulus {
            for y in 0..modulus {
                let sum_mod = (x + y) % modulus;
                let expected = sum_mod >= t;
                // let expected = x >= (t + modulus - y) % modulus;
                xy.push((x, y));
                xs.push(ModInt::new(x, modulus));
                ys.push(ModInt::new(y, modulus));
                expected_results.push(expected);
            }
        }
        let t_mod = ModInt::new(t, modulus);

        let (sender, receiver) = UnixStream::pair().unwrap();

        let (result_sender, result_receiver) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let rng_gb = AesRng::new();
            let reader = BufReader::new(sender.try_clone().unwrap());
            let writer = BufWriter::new(sender);
            let mut channel = Channel::new(reader, writer);
            let garbler_results = multiple_gb_greater_than_ss(&mut rng_gb.clone(), &mut channel, &ys, &t_mod);
            result_sender.send(garbler_results).unwrap();
        });

        let rng_ev = AesRng::new();
        let reader = BufReader::new(receiver.try_clone().unwrap());
        let writer = BufWriter::new(receiver);
        let mut channel = Channel::new(reader, writer);

        let evaluator_results = multiple_ev_greater_than_ss(&mut rng_ev.clone(), &mut channel, &xs);
        let garbler_results = result_receiver.recv().unwrap();

        // XOR the garbler and evaluator results to get the actual result
        let results: Vec<bool> = garbler_results
            .iter()
            .zip(evaluator_results.iter())
            .map(|(&garbler, &evaluator)| garbler ^ evaluator)
            .collect();

        println!("Garbler results: {:?}", garbler_results);
        println!("Evaluator results: {:?}", evaluator_results);
        println!("Results: {:?}", results);

        assert_eq!(results.len(), expected_results.len(), "Results and expected lengths do not match");

        // Compare results with expected
        for (i, &result) in results.iter().enumerate() {
            let (x, y) = xy[i];
            let expected = expected_results[i];
            assert_eq!(result, expected, "Failed for x={}, y={}, t={}: expected {}, got {}", x, y, t, expected, result);
        }
    }
}
