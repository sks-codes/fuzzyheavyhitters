use crate::data_structures::modint::{ModInt, get_bit_width_from_modint};

use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng};

use std::fmt::Debug;
use std::io::Write;
use rayon::prelude::*;
use rand::Rng;

/// A structure that contains both the garbler and the evaluators
/// wires for batch equality testing. This structure simplifies the API of the garbled circuit.
struct BatchEQInputs<F> {
    pub garbler_wires: Vec<BinaryBundle<F>>, // Flattened vector of all garbler wires
    pub results_wires: Vec<F>, // Vector of result wires, one per batch
    pub evaluator_wires: Vec<BinaryBundle<F>>, // Flattened vector of all evaluator wires
    pub num_batches: usize, // Number of batches
    pub dimensions_per_batch: usize, // Number of dimensions per batch (d)
}

pub fn garbler_preprocess_batch_equality_test(inputs: &[Vec<ModInt>]) -> Vec<Vec<u128>> {
    inputs.iter().map(|inner_vec| 
        inner_vec.iter().map(|x| x.val).collect()
    ).collect()
}

/// Batch equality test for garbler side
/// Takes a vector of vectors of ModInt and returns a vector of booleans
/// Each boolean indicates whether all ModInt values in the corresponding inner vector are equal to zero
pub fn batch_gb_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<ModInt>]
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    if inputs.is_empty() {
        return Vec::new();
    }

    let x_values = garbler_preprocess_batch_equality_test(inputs);
    let bit_width = get_bit_width_from_modint(&inputs[0][0]);
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();

    // Generate random masks for each batch result
    let results: Vec<bool> = (0..inputs.len()).map(|_| rand::rng().random::<bool>()).collect();
    
    let wires = gb_set_batch_fancy_inputs(&mut gb, &x_values, &results, bit_width);

    let eq_results = batch_fancy_equality(&mut gb, wires).unwrap();
    gb.outputs(eq_results.wires()).unwrap();

    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();
    
    results
}

/// The garbler's wire exchange method for batch equality
fn gb_set_batch_fancy_inputs<F, E>(
    gb: &mut F, 
    inputs: &[Vec<u128>], 
    results: &[bool], 
    bit_width: usize
) -> BatchEQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let num_batches = inputs.len();
    let dimensions_per_batch = if num_batches > 0 { inputs[0].len() } else { 0 };
    
    // Flatten all inputs into a single vector
    let flattened_inputs: Vec<u128> = inputs.iter().flatten().cloned().collect();
    let total_elements = flattened_inputs.len();

    // Single call to encode all garbler inputs
    let garbler_wires: Vec<BinaryBundle<F::Item>> = 
        gb.bin_encode_many(&flattened_inputs, bit_width).unwrap();

    // Encode all result masks using encode_many
    let results_u16: Vec<u16> = results.iter().map(|&r| r as u16).collect();
    let results_wires = gb.encode_many(&results_u16, &vec![2; results_u16.len()]).unwrap();

    // Single call to receive all evaluator inputs
    let evaluator_wires: Vec<BinaryBundle<F::Item>> = 
        gb.bin_receive_many(total_elements, bit_width).unwrap();

    BatchEQInputs {
        garbler_wires,
        results_wires,
        evaluator_wires,
        num_batches,
        dimensions_per_batch,
    }
}

pub fn evaluator_preprocess_batch_equality_test(inputs: &[Vec<ModInt>]) -> Vec<Vec<u128>> {
    inputs.iter().map(|inner_vec| 
        inner_vec.iter().map(|x| x.val).collect()
    ).collect()
}

/// Batch equality test for evaluator side
pub fn batch_ev_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<ModInt>]
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    if inputs.is_empty() {
        return Vec::new();
    }

    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let y_values = evaluator_preprocess_batch_equality_test(inputs);
    let bit_width = get_bit_width_from_modint(&inputs[0][0]);
    
    let wires = ev_set_batch_fancy_inputs(&mut ev, &y_values, bit_width);
    let eq_results = batch_fancy_equality(&mut ev, wires).unwrap();
    let outputs = ev.outputs(eq_results.wires()).unwrap().unwrap();
    
    // Convert outputs to boolean results
    let results: Vec<bool> = outputs.iter().map(|&output| output == 1).collect();

    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();

    results
}

/// The evaluator's wire exchange method for batch equality
fn ev_set_batch_fancy_inputs<F, E>(
    ev: &mut F, 
    inputs: &[Vec<u128>], 
    bit_width: usize
) -> BatchEQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let num_batches = inputs.len();
    let dimensions_per_batch = if num_batches > 0 { inputs[0].len() } else { 0 };
    
    // Flatten all inputs into a single vector
    let flattened_inputs: Vec<u128> = inputs.iter().flatten().cloned().collect();
    let total_elements = flattened_inputs.len();

    // Single call to receive all garbler inputs
    let garbler_wires: Vec<BinaryBundle<F::Item>> = 
        ev.bin_receive_many(total_elements, bit_width).unwrap();

    // Receive all result masks using receive_many
    let results_wires = ev.receive_many(&vec![2; num_batches]).unwrap();

    // Single call to encode all evaluator inputs
    let evaluator_wires: Vec<BinaryBundle<F::Item>> = 
        ev.bin_encode_many(&flattened_inputs, bit_width).unwrap();

    BatchEQInputs {
        garbler_wires,
        results_wires,
        evaluator_wires,
        num_batches,
        dimensions_per_batch,
    }
}

/// Batch fancy equality circuit implementation
fn batch_fancy_equality<F>(
    f: &mut F,
    wire_inputs: BatchEQInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let garbler_wires = wire_inputs.garbler_wires;
    let results_wires = wire_inputs.results_wires;
    let evaluator_wires = wire_inputs.evaluator_wires;
    let num_batches = wire_inputs.num_batches;
    let dimensions_per_batch = wire_inputs.dimensions_per_batch;

    let mut final_results = Vec::new();

    // Process each batch separately using flattened indices
    for batch_idx in 0..num_batches {
        let start_idx = batch_idx * dimensions_per_batch;
        let end_idx = start_idx + dimensions_per_batch;
        
        let mut equality_results = Vec::new();

        // Perform equality check for each dimension in this batch
        for i in start_idx..end_idx {
            let eq = f.bin_eq_bundles(&garbler_wires[i], &evaluator_wires[i])?;
            equality_results.push(eq);
        }

        // AND all equality results for this batch
        let and_result = f.and_many(&equality_results)?;

        // XOR with the result mask for this batch
        let final_result = f.xor(&and_result, &results_wires[batch_idx])?;
        final_results.push(final_result);
    }

    Ok(BinaryBundle::new(final_results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scuttlebutt::{AesRng, Channel};
    use std::io::{BufReader, BufWriter};
    use std::os::unix::net::UnixStream;

    #[test]
    fn test_batch_equality() {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut rng_gb = AesRng::new();
        let mut rng_ev = AesRng::new();

        let modulus = 1 << 16;
        
        // Create test data: 3 batches
        // Batch 0: [0, 0] (should be equal)
        // Batch 1: [0, 1] (should not be equal) 
        // Batch 2: [0, 0, 0] (should be equal)
        let gb_inputs = vec![
            vec![ModInt::new(0, modulus), ModInt::new(0, modulus)],
            vec![ModInt::new(0, modulus), ModInt::new(1, modulus)],
            vec![ModInt::new(0, modulus), ModInt::new(0, modulus), ModInt::new(0, modulus)],
        ];

        let ev_inputs = vec![
            vec![ModInt::new(0, modulus), ModInt::new(0, modulus)],
            vec![ModInt::new(0, modulus), ModInt::new(0, modulus)],
            vec![ModInt::new(0, modulus), ModInt::new(0, modulus), ModInt::new(0, modulus)],
        ];

        std::thread::scope(|s| {
            s.spawn(|| {
                let mut channel = Channel::new(
                    BufReader::new(sender.try_clone().unwrap()),
                    BufWriter::new(sender),
                );
                let masks = batch_gb_equality_test(&mut rng_gb.clone(), &mut channel, &gb_inputs);
                println!("Garbler masks: {:?}", masks);
            });

            s.spawn(|| {
                let mut channel = Channel::new(
                    BufReader::new(receiver.try_clone().unwrap()),
                    BufWriter::new(receiver),
                );
                let results = batch_ev_equality_test(&mut rng_ev.clone(), &mut channel, &ev_inputs);
                println!("Evaluator results: {:?}", results);
                
                // Results should be: [true, false, true] when XORed with garbler masks
            });
        });
    }
}
