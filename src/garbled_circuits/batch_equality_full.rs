use crate::channel::CommTrackingChannel;
use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng};

use std::fmt::Debug;
use rand::Rng;

/// A structure that contains both the garbler and the evaluators
/// wires for batch equality testing. This structure simplifies the API of the garbled circuit.
struct BatchEQInputs<F> {
    pub garbler_wires: BinaryBundle<F>, // Flattened vector of all garbler wires
    pub evaluator_wires: BinaryBundle<F>, // Flattened vector of all evaluator wires
    pub item_length: usize, // Length of each item in the batch
    pub item_count: usize, // Number of items in the batch
}

/// Batch equality test for garbler side
/// Takes a vector of vectors of ModInt and returns a vector of booleans
/// Each boolean indicates whether all ModInt values in the corresponding inner vector are equal to zero
pub fn batch_gb_equality_test(
    rng: &mut AesRng,
    channel: &mut CommTrackingChannel,
    inputs: &[Vec<bool>]
) -> Vec<bool>
// where
//     C: AbstractChannel + Clone,
{
    let mut gb = Garbler::<CommTrackingChannel, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let results: Vec<bool> = (0..inputs.len()).map(|_| rand::rng().random::<bool>()).collect();
    let wires = gb_set_batch_equality_inputs(&mut gb, inputs, &results);
    let eq_results = batch_fancy_equality(&mut gb, wires).unwrap();
    gb.outputs(eq_results.wires()).unwrap();
    let mut ack = [0u8; 1];
    channel.flush().unwrap();
    channel.read_bytes(&mut ack).unwrap();
    
    results
}

/// The garbler's wire exchange method for batch equality
fn gb_set_batch_equality_inputs<F, E>(
    gb: &mut F, 
    inputs: &[Vec<bool>], 
    results: &[bool], 
) -> BatchEQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let mut garbler_circuit_inputs = results.iter().map(|&r| r as u16).collect::<Vec<u16>>();
    inputs.iter().for_each(|input| {
        garbler_circuit_inputs.extend(input.iter().map(|&x| x as u16));
    });

    // Single call to encode all garbler inputs
    let garbler_wires = 
        BinaryBundle::new(gb.encode_many(&garbler_circuit_inputs, &vec![2; garbler_circuit_inputs.len()]).unwrap());

    let item_length = inputs[0].len();
    let item_count = inputs.len();
    // Single call to receive all evaluator inputs
    let evaluator_wires =  
        BinaryBundle::new(gb.receive_many(&vec![2; item_count * item_length]).unwrap());

    BatchEQInputs {
        garbler_wires,
        evaluator_wires,
        item_length,
        item_count,
    }
}

/// Batch equality test for evaluator side
pub fn batch_ev_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<bool>]
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let wires = ev_set_batch_fancy_inputs(&mut ev, inputs);
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
    inputs: &[Vec<bool>], 
) -> BatchEQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let item_length = inputs[0].len();
    let item_count = inputs.len();
    // Single call to receive all garbler inputs
    let start = std::time::Instant::now();
    let garbler_wires = 
        BinaryBundle::new(ev.receive_many(&vec![2; item_count * item_length + item_count]).unwrap());
    println!("Garbler wires set up in {:?}", start.elapsed());

    let mut evaluator_circuit_inputs = Vec::new();   
    inputs.iter().for_each(|input| {
        evaluator_circuit_inputs.extend(input.iter().map(|&x| x as u16));
    });
    println!("Evaluator circuit inputs in: {:?}", start.elapsed());
    let evaluator_wires = 
        BinaryBundle::new(ev.encode_many(&evaluator_circuit_inputs, &vec![2; evaluator_circuit_inputs.len()]).unwrap());
    println!("Evaluator wires set up in {:?}", start.elapsed());

    BatchEQInputs {
        garbler_wires,
        evaluator_wires,
        item_length,
        item_count,
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
    let evaluator_wires = wire_inputs.evaluator_wires;
    let item_length = wire_inputs.item_length;
    let item_count = wire_inputs.item_count;

    let mut final_results = Vec::with_capacity(item_count);

    // Process each batch separately using flattened indices
    for item_idx in 0..item_count {
        let garbler_item_start = item_idx * item_length + item_count;
        let evaluator_item_start = item_idx * item_length;

        let equality_result = f.bin_eq_bundles(
            &BinaryBundle::new(garbler_wires.wires()[garbler_item_start..garbler_item_start+item_length].to_vec()),
            &BinaryBundle::new(evaluator_wires.wires()[evaluator_item_start..evaluator_item_start+item_length].to_vec()),
        )?;

        // XOR with the result mask for this batch
        let final_result = f.xor(&equality_result, &garbler_wires[item_idx])?;
        final_results.push(final_result);
    }

    Ok(BinaryBundle::new(final_results))
}