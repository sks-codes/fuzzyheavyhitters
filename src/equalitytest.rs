use fancy_garbling::{twopac::semihonest::{Evaluator, Garbler}, util, AllWire, BinaryBundle, BundleGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput, FancyReveal};

use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Channel, SyncChannel};

use std::fmt::Debug;

use std::{
    io::{BufReader, BufWriter},
    os::unix::net::UnixStream,
};
use std::io::{Read, Write};
use std::time::Instant;
use fancy_garbling::util::RngExt;
use ocelot::ot::Sender;
use rayon::prelude::*;

/// A structure that contains both the garbler and the evaluators
/// wires. This structure simplifies the API of the garbled circuit.
struct EQInputs<F> {
    pub garbler_wires: BinaryBundle<F>,
    pub evaluator_wires: BinaryBundle<F>,
}

pub fn multiple_gb_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<u16>]
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let num_tests = inputs.len();
    let mut results = Vec::with_capacity(num_tests);
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    // let start = Instant::now();
    // println!("Step 1");
    let masked_inputs =
        inputs.iter().map(|input| {
            let mask = rng.clone().gen_bool();
            results.push(mask);
            [input.as_slice(), &[mask as u16]].concat()
        }).collect::<Vec<Vec<u16>>>();

    // let step1_time = start.elapsed();
    // println!("time: {:?}", step1_time);
    // println!("Step 2");

    let wire_inputs = masked_inputs.into_iter().flatten().collect::<Vec<u16>>();
    let wires = gb_set_fancy_inputs(&mut gb, wire_inputs.as_slice(), inputs.len());

    // let step2_time = start.elapsed() - step1_time;
    // println!("time: {:?}", step2_time);
    // println!("Step 3");

    let eq = fancy_equality(&mut gb, wires, num_tests).unwrap();
    gb.outputs(eq.wires()).unwrap();
    // let step3_time = start.elapsed() - step1_time - step2_time;
    // println!("time: {:?}", step3_time);
    // println!("Step 4");
    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();
    results
}

/// The garbler's wire exchange method
fn gb_set_fancy_inputs<F, E>(gb: &mut F, input: &[u16], num_tests : usize) -> EQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    // The garbler encodes their input into binary wires
    let garbler_wires: BinaryBundle<F::Item> = gb.encode_bundle(&input, &vec![2; input.len()]).map(BinaryBundle::from).unwrap();
    // The evaluator receives their input labels using Oblivious Transfer (OT)
    let evaluator_wires: BinaryBundle<F::Item> = gb.bin_receive(input.len() - num_tests).unwrap();

    EQInputs {
        garbler_wires,
        evaluator_wires,
    }
}


pub fn multiple_ev_equality_test<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<u16>]
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let num_tests = inputs.len();
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let input_vec = inputs.to_vec().into_iter().flatten().collect::<Vec<u16>>();
    let ev_in = input_vec.as_slice();
    let wires = ev_set_fancy_inputs(&mut ev, &ev_in, num_tests);
    let eq = fancy_equality(&mut ev, wires, num_tests).unwrap();
    let output = ev.outputs(eq.wires()).unwrap().unwrap();
    let results = output.iter().map(|r| *r == 1).collect();

    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();

    results
}

/// The evaluator's wire exchange method
fn ev_set_fancy_inputs<F, E>(ev: &mut F, input: &[u16], num_tests : usize) -> EQInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    // The number of bits needed to represent a single input, in this case a u128
    let nwires = input.len();
    // The evaluator receives the garblers input labels.
    let garbler_wires: BinaryBundle<F::Item> = ev.bin_receive(nwires + num_tests).unwrap();
    // The evaluator receives their input labels using Oblivious Transfer (OT).
    let evaluator_wires: BinaryBundle<F::Item> = ev.encode_bundle(input, &vec![2; nwires]).map(BinaryBundle::from).unwrap();

    EQInputs {
        garbler_wires,
        evaluator_wires,
    }
}


/// Extension trait for `FancyBinary` providing gadgets that operate over binary bundles.
pub trait BinaryGadgets: FancyBinary + BundleGadgets {
    fn bin_eq_bundles(
        &mut self,
        x: &BinaryBundle<Self::Item>,
        y: &BinaryBundle<Self::Item>,
    ) -> Result<Self::Item, Self::Error> {
        let zs = x
            .wires()
            .iter()
            .zip(y.wires().iter())
            .map(|(x_bit, y_bit)| {
                let xy = self.xor(x_bit, y_bit)?;
                self.negate(&xy)
            })
            .collect::<Result<Vec<Self::Item>, Self::Error>>()?;

        self.and_many(&zs)
    }

    fn bin_eq_bundles_shared(
        &mut self,
        x: &BinaryBundle<Self::Item>,
        y: &BinaryBundle<Self::Item>,
    ) -> Result<Self::Item, Self::Error> {
        assert_eq!(x.wires().len(), y.wires().len() + 1, "x must have one more wire than y");

        let (x_wires, mask) = x.wires().split_at(x.wires().len() - 1);
        let mask = &mask[0]; // Last wire is the mask

        let eq_result = self.bin_eq_bundles(&BinaryBundle::new(x_wires.to_vec()), y)?;

        self.xor(&eq_result, mask) // Obscure the output with the mask
    }

    fn multi_bin_eq_bundles_shared(
        &mut self,
        x: &BinaryBundle<Self::Item>,
        y: &BinaryBundle<Self::Item>,
        num_tests: usize,
    ) -> Result<BinaryBundle<Self::Item>, Self::Error> {
        assert_eq!(
            x.wires().len(),
            y.wires().len() + num_tests,
            "each string in x must have one extra mask bit"
        );
        assert_eq!(y.wires().len() % num_tests, 0);

        let string_len = y.wires().len() / num_tests;

        let mut results = Vec::with_capacity(num_tests);

        for i in 0..num_tests {
            let x_start = i * (string_len + 1);
            let y_start = i * string_len;
            let eq_result = self.bin_eq_bundles(
                &BinaryBundle::new(x.wires()[x_start..x_start+string_len].to_vec()),
                &BinaryBundle::new(y.wires()[y_start..y_start+string_len].to_vec()))?;

            let masked_result = self.xor(&eq_result, &x.wires()[x_start+string_len])?;
            results.push(masked_result);
        }
        Ok(BinaryBundle::new(results))
    }
}

/// Implement BinaryGadgets for `Garbler`
impl<C, R, S, W> BinaryGadgets for fancy_garbling::twopac::semihonest::Garbler<C, R, S, W>
where
    Self: FancyBinary + BundleGadgets,
{
}

/// Implement BinaryGadgets for `Evaluator`
impl<C, R, S, W> BinaryGadgets for fancy_garbling::twopac::semihonest::Evaluator<C, R, S, W>
where
    Self: FancyBinary + BundleGadgets,
{
}

/// Fancy equality test using garbled circuits
fn fancy_equality<F>(
    f: &mut F,
    wire_inputs: EQInputs<F::Item>,
    num_tests: usize,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let equality_bits = f.multi_bin_eq_bundles_shared(&wire_inputs.garbler_wires, &wire_inputs.evaluator_wires,num_tests)?;
    Ok(equality_bits)
}


#[test]
fn eq_gc() {
    let gb_value = vec![vec![0,1,1,0], vec![0,0,0,0], vec![1,1,1,0]];
    let ev_value = vec![vec![0,1,1,0], vec![0,0,0,0], vec![1,1,1,0]];
    let expected = gb_value.iter().enumerate().map(|(i, x)| *x == ev_value[i]).collect::<Vec<bool>>();

    let (sender, receiver) = UnixStream::pair().unwrap();

    let (result_sender, result_receiver) = std::sync::mpsc::channel();

    let x = std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        let masks = multiple_gb_equality_test(&mut rng_gb.clone(), &mut channel, gb_value.as_slice());
        result_sender.send(masks).unwrap();
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_equality_test(&mut rng_ev.clone(), &mut channel, ev_value.as_slice());

    let masks = result_receiver.recv().unwrap();
    x.join().unwrap();

    assert_eq!(
        masks.len(),
        results.len(),
        "Masks and results should have the same length"
    );

    for i in 0..results.len() {
        assert_eq!(
            (masks[i] ^ results[i]) as u16,
            expected[i] as u16,
            "The garbled circuit result is incorrect for index {} and should be {}",
            i,
            expected[i]
        );
    }
}

