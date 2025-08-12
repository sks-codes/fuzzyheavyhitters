use crate::data_structures::modint::{ModInt, get_bit_width_from_modint};

use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Block};
use std::fmt::Debug;
use rand::Rng;

/// Input structure for greater than secret sharing comparison
struct GreaterThanSSInputs<F> {
    pub garbler_z1_wires: Vec<BinaryBundle<F>>, // (t - y) mod modulus
    pub garbler_z2_wires: Vec<BinaryBundle<F>>, // y  
    pub garbler_b_wires: Vec<F>,                // y >= t
    pub garbler_mask_wires: Vec<F>,             // masks for outputs
    pub evaluator_z3_wires: Vec<BinaryBundle<F>>, // x
}

fn garbler_preprocess_greater_than_ss(inputs_y: &[ModInt], inputs_t: &[ModInt]) -> (Vec<u128>, Vec<u128>, Vec<bool>) {
    assert_eq!(inputs_y.len(), inputs_t.len(), "y and t inputs must have same length");
    
    let mut z1_values = Vec::new();
    let mut z2_values = Vec::new(); 
    let mut b_values = Vec::new();
    
    for (y, t) in inputs_y.iter().zip(inputs_t.iter()) {
        assert_eq!(y.modulus(), t.modulus(), "y and t must have same modulus");
        
        let modulus = y.modulus();
        let bit_width = get_bit_width_from_modint(y);
        
        // z1 = modulus - 1 - (t - y) mod modulus
        let z1_modint = *t - *y;
        let z1 = z1_modint.val();
        
        // z2 = y  
        let z2 = y.val();
        
        // b = y <= t
        let b = y.val() > t.val();
        
        z1_values.push(z1);
        z2_values.push(z2);
        b_values.push(b);
    }
    
    (z1_values, z2_values, b_values)
}

pub fn multiple_gb_greater_than_ss<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_y: &[ModInt], // Garbler's y values
    inputs_t: &[ModInt], // Garbler's t values (thresholds)
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    assert_eq!(inputs_y.len(), inputs_t.len(), "y and t inputs must have same length");

    let (z1_values, z2_values, b_values) = garbler_preprocess_greater_than_ss(inputs_y, inputs_t);
    let bit_width = get_bit_width_from_modint(&inputs_y[0]);
    
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();

    // Mask for the output
    let results = (0..inputs_y.len())
        .map(|_| rand::rng().random::<bool>())
        .collect::<Vec<bool>>();
    let circuit_wires = gb_set_greater_than_ss_inputs(&mut gb, &z1_values, &z2_values, &b_values, &results, bit_width);

    let greater_than_ss = fancy_greater_than_ss(&mut gb, circuit_wires).unwrap();
    gb.outputs(greater_than_ss.wires()).unwrap();
    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();

    results
}

/// Helper to set garbler inputs for greater than secret sharing comparison
fn gb_set_greater_than_ss_inputs<F, E>(
    gb: &mut F,
    z1_values: &[u128], // modulus - 1 - (t - y) mod modulus
    z2_values: &[u128], // y  
    b_values: &[bool],  // y > t
    results: &[bool], // masks for outputs
    bit_width: usize,
) -> GreaterThanSSInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    assert!(bit_width > 0, "Bit width must be greater than 0");
    assert!(bit_width < 128, "Bit width must be less than 128 for this simplified implementation");
    assert_eq!(z1_values.len(), z2_values.len(), "z1 and z2 must have same length");
    assert_eq!(z1_values.len(), b_values.len(), "z1 and b must have same length");

    // Encode garbler's z1 and z2 values
    let garbler_z1_wires = gb.bin_encode_many(z1_values, bit_width).unwrap();
    let garbler_z2_wires = gb.bin_encode_many(z2_values, bit_width).unwrap();
    
    // Encode garbler's b bits - each as a single bit with modulus 2
    let b_u16_values: Vec<u16> = b_values.iter().map(|&b| b as u16).collect();
    let garbler_b_wires = gb.encode_many(&b_u16_values, &vec![2; b_u16_values.len()]).unwrap();

    // Encode garbler's output masks
    let results_u16: Vec<u16> = results.iter().map(|&r| r as u16).collect();
    let garbler_mask_wires = gb.encode_many(&results_u16, &vec![2; results_u16.len()]).unwrap();
    
    // Receive evaluator's z3 values
    let evaluator_z3_wires = gb.bin_receive_many(z1_values.len(), bit_width).unwrap();

    GreaterThanSSInputs {
        garbler_z1_wires,
        garbler_z2_wires,
        garbler_b_wires,
        garbler_mask_wires,
        evaluator_z3_wires,
    }
}

/// Evaluator preprocessing: x
fn evaluator_preprocess_greater_than_ss(inputs_x: &[ModInt]) -> Vec<u128> {
    inputs_x.iter().map(|x| {
        let z3 = x.val();
        z3
    }).collect()
}

pub fn multiple_ev_greater_than_ss<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_x: &[ModInt], // x
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let z3_values = evaluator_preprocess_greater_than_ss(inputs_x);
    let bit_width = get_bit_width_from_modint(&inputs_x[0]);
    
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let circuit_wires = ev_set_greater_than_ss_inputs(&mut ev, &z3_values, bit_width);
    let results = fancy_greater_than_ss(&mut ev, circuit_wires).unwrap();
    let outputs = ev.outputs(results.wires()).unwrap().unwrap();
    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();
    
    // Convert outputs to boolean results
    outputs.iter().map(|&x| x != 0).collect()
}

/// Helper to set evaluator inputs for greater than secret sharing comparison
fn ev_set_greater_than_ss_inputs<F, E>(
    ev: &mut F,
    z3_values: &[u128], // x
    bit_width: usize,
) -> GreaterThanSSInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    assert!(bit_width > 0, "Bit width must be greater than 0");
    assert!(bit_width < 128, "Bit width must be less than 128 for this simplified implementation");

    // Receive garbler's z1 and z2 values
    let garbler_z1_wires = ev.bin_receive_many(z3_values.len(), bit_width).unwrap();
    let garbler_z2_wires = ev.bin_receive_many(z3_values.len(), bit_width).unwrap();
    
    // Receive garbler's b bits 
    let garbler_b_wires = ev.receive_many(&vec![2; z3_values.len()]).unwrap();
    
    // Receive output masks
    let garbler_mask_wires = ev.receive_many(&vec![2; z3_values.len()]).unwrap();
    
    // Encode evaluator's z3 values
    let evaluator_z3_wires = ev.bin_encode_many(z3_values, bit_width).unwrap();

    GreaterThanSSInputs {
        garbler_z1_wires,
        garbler_z2_wires,
        garbler_b_wires,
        garbler_mask_wires,
        evaluator_z3_wires,
    }
}

fn fancy_greater_than_ss<F>(
    f: &mut F,
    wire_inputs: GreaterThanSSInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let mut final_results = Vec::new();

    for i in 0..wire_inputs.garbler_z1_wires.len() {
        let z1_wires = &wire_inputs.garbler_z1_wires[i];
        let z2_wires = &wire_inputs.garbler_z2_wires[i];
        let b_wire = &wire_inputs.garbler_b_wires[i];
        let mask_wire = &wire_inputs.garbler_mask_wires[i];
        let z3_wires = &wire_inputs.evaluator_z3_wires[i];
        let z3_flipped = z3_wires
            .wires()
            .iter()
            .map(|x| f.negate(x))
            .collect::<Result<Vec<F::Item>, F::Error>>()
            .map(BinaryBundle::new)?;
        
        // Compute overflow(z3 + z2)
        // 1 is equivalent to x+y > mod - 1 
        // 0 is equivalent to x+y <= mod - 1
        let (sum1, carry1) = f.bin_addition(z3_wires, z2_wires)?;
        let x_plus_y_overflow = carry1; 
        let x_plus_y_not_overflow = f.negate(&x_plus_y_overflow)?;
        
        // Compute overflow(z3_flipped + z1) 
        // 1 is equivalent to x >= (t-y) mod modulus
        // 0 is equivalent to x < (t-y) mod modulus
        let (sum2, carry2) = f.bin_addition(&z3_flipped, z1_wires)?;
        let overflow2 = carry2; 
        let x_gt_t_minus_y = f.negate(&overflow2)?;
        
        let b_and_overflow2 = f.and(b_wire, &x_gt_t_minus_y)?;
        let first_part = f.and(&x_plus_y_overflow, &b_and_overflow2)?;
        
        let overflow2_or_b = f.or(&x_gt_t_minus_y, b_wire)?;
        let second_part = f.and(&x_plus_y_not_overflow, &overflow2_or_b)?;

        // This computes (x + y) mod modulus >= t
        let ge_result = f.or(&first_part, &second_part)?;
        
        let final_result = f.xor(&ge_result, mask_wire)?;
        
        final_results.push(final_result);
    }

    Ok(BinaryBundle::new(final_results))
}

/// Convert a Block to Vec<bool> bits
/// Note: This always produces exactly 128 bits, but you can truncate to desired bit_width
pub fn block_to_bool_bits(block: Block, lsb_first: bool) -> Vec<bool> {
    let bytes = block.as_ref();
    let mut bits = Vec::with_capacity(128);

    if lsb_first {
        for byte in bytes.iter() {
            for i in 0..8 {
                bits.push(((*byte >> i) & 1) == 1);
            }
        }
    } else {
        for byte in bytes.iter().rev() {
            for i in (0..8).rev() {
                bits.push(((*byte >> i) & 1) == 1);
            }
        }
    }

    bits
}

/// Convert a Vec<bool> (LSB first) into a single u128 value
/// Note: Bit width is constrained to be less than 128 for simplicity
pub fn bool_vec_to_value(bits: &[bool], bit_width: usize) -> u128 {
    assert!(bit_width > 0, "Bit width must be greater than 0");
    assert!(bit_width < 128, "Bit width must be less than 128 for this simplified implementation");
    assert!(bits.len() == bit_width, "Input length {} doesn't match expected bit width {}", bits.len(), bit_width);
    
    let mut value = 0u128;
    for (i, &b) in bits.iter().enumerate() {
        if b {
            value |= 1u128 << i;
        }
    }
    
    value
}

/// Convert a u128 to Vec<bool> bits (LSB first)
/// Note: Bit width is constrained to be less than 128
pub fn u128_to_bool_bits(x: u128, n: usize) -> Vec<bool> {
    assert!(n < 128, "Bit width must be less than 128 for this simplified implementation");
    (0..n).map(|i| ((x >> i) & 1) == 1).collect()
}

/// Create a value with specified bit width from a single u128
/// Note: Bit width is constrained to be less than 128
fn make_value_from_u128(value: u128, bit_width: usize) -> Vec<bool> {
    assert!(bit_width < 128, "Bit width must be less than 128 for this simplified implementation");
    u128_to_bool_bits(value, bit_width)
}
