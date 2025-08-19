use crate::data_structures::modint::{ModInt, get_bit_width_from_modint};
use crate::util::u128_to_bits_msb;

use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Block};
use std::fmt::{Binary, Debug};
use rand::Rng;

/// Input structure for less than secret sharing comparison
struct LessThanSSInputs<F> {
    garbler_wires: BinaryBundle<F>,
    evaluator_wires: BinaryBundle<F>,
    item_length: usize,
    item_count: usize,
}

fn garbler_preprocess_less_than_ss(ys: &[ModInt], t: &ModInt) -> (Vec<Vec<bool>>, Vec<Vec<bool>>, Vec<bool>) {
    let mut z1_values = Vec::new();
    let mut z2_values = Vec::new(); 
    let mut b_values = Vec::new();
    let item_length = get_bit_width_from_modint(t);
    let item_count = ys.len();
    
    for y in ys.iter() {
        assert_eq!(y.modulus(), t.modulus(), "y and t must have same modulus");
        
        // z1 = modulus - 1 - (t - y) mod modulus
        let z1_modint = *y - *t - ModInt::one(y.modulus());
        let z1 = z1_modint.val();
        let z1_bool = u128_to_bits_msb(z1, item_length);
        
        // z2 = y  
        let z2 = y.val();
        let z2_bool = u128_to_bits_msb(z2, item_length);
        
        // b = y <= t
        let b = y.val() <= t.val();
        
        z1_values.push(z1_bool);
        z2_values.push(z2_bool);
        b_values.push(b);
    }
    
    (z1_values, z2_values, b_values)
}

pub fn multiple_gb_less_than_ss<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_y: &[ModInt], // Garbler's y values
    input_t: &ModInt, // Garbler's t values (thresholds)
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let (z1_values, z2_values, b_values) = garbler_preprocess_less_than_ss(inputs_y, input_t);
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();

    // Mask for the output
    let results = (0..inputs_y.len())
        .map(|_| rand::rng().random::<bool>())
        .collect::<Vec<bool>>();
    let circuit_wires = gb_set_less_than_ss_inputs(&mut gb, &z1_values, &z2_values, &b_values, &results);

    let less_than_ss = fancy_less_than_ss(&mut gb, circuit_wires).unwrap();
    gb.outputs(less_than_ss.wires()).unwrap();
    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();

    results
}

/// Helper to set garbler inputs for less than secret sharing comparison
fn gb_set_less_than_ss_inputs<F, E>(
    gb: &mut F,
    z1_values: &[Vec<bool>], // modulus - 1 - (t - y) mod modulus
    z2_values: &[Vec<bool>], // y  
    b_values: &[bool],  // y <= t bits
    results: &[bool], // masks for outputs
) -> LessThanSSInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    assert_eq!(z1_values.len(), z2_values.len(), "z1 and z2 must have same length");
    assert_eq!(z1_values.len(), b_values.len(), "z1 and b must have same length");

    let mut garbler_circuit_inputs = Vec::new();
    z1_values.iter().for_each(|z1| {
        garbler_circuit_inputs.extend(z1.iter().map(|&b| b as u16));
    });
    z2_values.iter().for_each(|z2| {
        garbler_circuit_inputs.extend(z2.iter().map(|&b| b as u16));
    });
    garbler_circuit_inputs.extend(b_values.iter().map(|&b| b as u16));
    garbler_circuit_inputs.extend(results.iter().map(|&r| r as u16));

    let garbler_wires = 
        BinaryBundle::new(gb.encode_many(&garbler_circuit_inputs, &vec![2; garbler_circuit_inputs.len()]).unwrap());

    let item_length = z1_values[0].len();
    let item_count = z1_values.len();

    let evaluator_wires = 
        BinaryBundle::new(gb.receive_many(&vec![2; item_count * item_length]).unwrap());

    LessThanSSInputs {
        garbler_wires,
        evaluator_wires,
        item_length,
        item_count,
    }
}

/// Evaluator preprocessing: x
fn evaluator_preprocess_less_than_ss(inputs_x: &[ModInt]) -> Vec<Vec<bool>> {
    let item_length = get_bit_width_from_modint(&inputs_x[0]);
    inputs_x.iter().map(|x| {
        let z3 = u128_to_bits_msb(x.val(), item_length);
        z3
    }).collect()
}

pub fn multiple_ev_less_than_ss<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_x: &[ModInt], // x
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let z3_values = evaluator_preprocess_less_than_ss(inputs_x);
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();

    let circuit_wires = ev_set_less_than_ss_inputs(&mut ev, &z3_values);
    let results = fancy_less_than_ss(&mut ev, circuit_wires).unwrap();
    let outputs = ev.outputs(results.wires()).unwrap().unwrap();
    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();
    
    // Convert outputs to boolean results
    outputs.iter().map(|&x| x != 0).collect()
}

/// Helper to set evaluator inputs for less than secret sharing comparison
fn ev_set_less_than_ss_inputs<F, E>(
    ev: &mut F,
    z3_values: &[Vec<bool>], // x
) -> LessThanSSInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let item_length = z3_values[0].len();
    let item_count = z3_values.len();

    let garbler_wires =
        BinaryBundle::new(ev.receive_many(&vec![2; 2 * item_count * item_length + 2 * item_count]).unwrap());

    let mut evaluator_circuit_inputs = Vec::new();
    z3_values.iter().for_each(|z3| {
        evaluator_circuit_inputs.extend(z3.iter().map(|&b| b as u16));
    });

    let evaluator_wires =
        BinaryBundle::new(ev.encode_many(&evaluator_circuit_inputs, &vec![2; evaluator_circuit_inputs.len()]).unwrap());

    LessThanSSInputs {
        garbler_wires,
        evaluator_wires,
        item_length,
        item_count,
    }
}

fn fancy_less_than_ss<F>(
    f: &mut F,
    wire_inputs: LessThanSSInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let mut final_results = Vec::new();
    let garbler_wires = wire_inputs.garbler_wires;
    let evaluator_wires = wire_inputs.evaluator_wires;
    let item_length = wire_inputs.item_length;
    let item_count = wire_inputs.item_count;

    for i in 0..item_count {
        let (_, carry1) = f.bin_addition(
            &BinaryBundle::new(garbler_wires.wires()[i*item_length..(i+1)*item_length].to_vec()),
            &BinaryBundle::new(evaluator_wires.wires()[i * item_length..(i + 1) * item_length].to_vec())
        )?;
        let x_plus_y_overflow = carry1;
        let x_plus_y_not_overflow = f.negate(&x_plus_y_overflow)?;

        let (_, carry2) = f.bin_addition(
            &BinaryBundle::new(garbler_wires.wires()[(item_count + i) * item_length..(item_count + i + 1) * item_length].to_vec()),
            &BinaryBundle::new(evaluator_wires.wires()[i * item_length..(i + 1) * item_length].to_vec())
        )?;
        let overflow2 = carry2;
        let x_lt_t_minus_y = f.negate(&overflow2)?; 

        let b_and_overflow2 = f.and(&garbler_wires[2 * item_count * item_length + i], &x_lt_t_minus_y)?;
        let first_part = f.and(&x_plus_y_not_overflow, &b_and_overflow2)?;

        let overflow2_or_b = f.or(&x_lt_t_minus_y, &garbler_wires[2 * item_count * item_length + i])?;
        let second_part = f.and(&x_plus_y_overflow, &overflow2_or_b)?;

        let final_result = f.or(&first_part, &second_part)?;
        let final_result = f.xor(&final_result, &garbler_wires[2 * item_count * item_length + item_count + i])?;

        final_results.push(final_result);
    }

    Ok(BinaryBundle::new(final_results))
}