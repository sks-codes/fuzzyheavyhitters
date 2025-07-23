use crate::data_structures::modint::{ModInt, get_bit_width_from_modint};

use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal, util,
    twopac::semihonest::{Evaluator, Garbler},
};
use fancy_garbling::util::RngExt;

use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use ocelot::ot::Sender;
use scuttlebutt::{AbstractChannel, AesRng, Channel, SyncChannel};

use std::fmt::{Binary, Debug};

use std::{
    io::{BufReader, BufWriter},
    os::unix::net::UnixStream,
};
use std::io::{Read, Write};
use std::time::Instant;
use rayon::prelude::*;

use rand::Rng;

/// A structure that contains both the garbler and the evaluators
/// wires. This structure simplifies the API of the garbled circuit.
struct EQInputs<F> {
    pub garbler_wires: Vec<BinaryBundle<F>>,
    pub results_wire: F, // Single wire instead of vector
    pub evaluator_wires: Vec<BinaryBundle<F>>,
}

pub fn garbler_preprocess_equality_test(input: &[ModInt]) -> Vec<u128> {
    input.iter().map(|x| x.val).collect()
}

pub fn multiple_gb_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[ModInt]
) -> bool
where
    C: AbstractChannel + Clone,
{
    println!("Garbler gb equality test modulo: {}", inputs[0].modulus());
    let x_values = garbler_preprocess_equality_test(inputs);
    let bit_width = get_bit_width_from_modint(&inputs[0]);
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();

    // Mask for the output - now just one boolean since we're ANDing everything
    let result = rand::rng().random::<bool>();
    let wires = gb_set_fancy_inputs(&mut gb, &x_values, result, bit_width);

    let eq = fancy_equality(&mut gb, wires).unwrap();
    gb.outputs(eq.wires()).unwrap();

    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();
    result // Return single boolean
}

/// The garbler's wire exchange method
fn gb_set_fancy_inputs<F, E>(gb: &mut F, input: &[u128], result: bool, bit_width: usize) -> EQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    // The garbler encodes their input into binary wires
    let garbler_wires: Vec<BinaryBundle<F::Item>> = gb.bin_encode_many(input, bit_width).unwrap();

    // Encode the single result mask into a binary wire
    let result_wire: F::Item = gb.encode(result as u16, 2).unwrap();

    // The evaluator receives their input labels using Oblivious Transfer (OT)
    let evaluator_wires: Vec<BinaryBundle<F::Item>> = gb.bin_receive_many(input.len(), bit_width).unwrap();

    EQInputs {
        garbler_wires,
        results_wire: result_wire, // Single result wire
        evaluator_wires,
    }
}

fn evaluator_preprocess_equality_test(input: &[ModInt]) -> Vec<u128> {
    input.iter().map(|x| x.val).collect()
}

pub fn multiple_ev_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[ModInt]
) -> bool
where
    C: AbstractChannel + Clone,
{
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let y_values = evaluator_preprocess_equality_test(inputs);
    let bit_width = get_bit_width_from_modint(&inputs[0]);
    let wires = ev_set_fancy_inputs(&mut ev, &y_values, bit_width);
    let eq = fancy_equality(&mut ev, wires).unwrap();
    let output = ev.outputs(eq.wires()).unwrap().unwrap();
    let result = output[0] == 1; // Single boolean result

    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();

    result
}

/// The evaluator's wire exchange method
fn ev_set_fancy_inputs<F, E>(ev: &mut F, input: &[u128], bit_width: usize) -> EQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    // The evaluator receives the garblers input labels.
    let garbler_wires: Vec<BinaryBundle<F::Item>> = ev.bin_receive_many(input.len(), bit_width).unwrap();
    // The evaluator receives the single result mask
    let result_wire: F::Item = ev.receive(2).unwrap();
    // The evaluator receives their input labels using Oblivious Transfer (OT).
    let evaluator_wires: Vec<BinaryBundle<F::Item>> = ev.bin_encode_many(input, bit_width).unwrap();

    EQInputs {
        garbler_wires,
        results_wire: result_wire, // Single result wire
        evaluator_wires,
    }
}

fn fancy_equality<F>(
    f: &mut F,
    wire_inputs: EQInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let garbler_wires = wire_inputs.garbler_wires;
    let result_wire = &wire_inputs.results_wire; // Single result wire
    let evaluator_wires = wire_inputs.evaluator_wires;

    let mut equality_results = Vec::new();

    for i in 0..garbler_wires.len() {
        // Perform equality check for each garbler wire against the evaluator wire
        let eq = f.bin_eq_bundles(&garbler_wires[i], &evaluator_wires[i])?;
        equality_results.push(eq);
    }

    let and_result = f.and_many(&equality_results)?;

    // XOR with the result mask
    let final_result = f.xor(&and_result, result_wire)?;

    Ok(BinaryBundle::new(vec![final_result]))
}