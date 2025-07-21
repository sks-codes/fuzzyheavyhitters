use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal, util,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Channel, Block};
use crate::data_structures::modint::ModInt;
use std::fmt::Debug;
use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;

/// Input structure for complex comparisons with threshold
struct ComplexComparisonInputs<F> {
    pub garbler_z1_wires: Vec<BinaryBundle<F>>, // t - y
    pub garbler_z2_wires: Vec<BinaryBundle<F>>, // mod - 1 - y  
    pub garbler_b_wires: Vec<F>,                // b bits (y <= t)
    pub evaluator_z3_wires: Vec<BinaryBundle<F>>, // mod - 1 - x
}

/// Convert ModInt to bit width, ensuring it fits in less than 128 bits
fn get_bit_width_from_modint(modint: &ModInt) -> usize {
    let modulus = modint.modulus();
    let bit_width = (127 - modulus.leading_zeros()) as usize;
    assert!(bit_width < 128, "ModInt modulus requires {} bits, must be < 128", bit_width);
    bit_width
}

fn garbler_preprocess_complex(inputs_y: &[ModInt], inputs_t: &[ModInt]) -> (Vec<u128>, Vec<u128>, Vec<bool>) {
    assert_eq!(inputs_y.len(), inputs_t.len(), "y and t inputs must have same length");
    
    let mut z1_values = Vec::new();
    let mut z2_values = Vec::new(); 
    let mut b_values = Vec::new();
    
    for (y, t) in inputs_y.iter().zip(inputs_t.iter()) {
        assert_eq!(y.modulus(), t.modulus(), "y and t must have same modulus");
        
        let modulus = y.modulus();
        let bit_width = get_bit_width_from_modint(y);
        
        // z1 = t - y (mod modulus)
        let mut z1 = if t.val() >= y.val() {
            t.val() - y.val()
        } else {
            modulus - (y.val() - t.val())
        };
        z1 = modulus - 1 - z1;
        
        // z2 = modulus - 1 - y  
        let z2 = y.val();
        
        // b = y <= t
        let b = y.val() <= t.val();
        
        println!("y: {}, t: {}, z1: {}, z2: {}, b: {}", y.val(), t.val(), z1, z2, b);
        
        z1_values.push(z1);
        z2_values.push(z2);
        b_values.push(b);
    }
    
    (z1_values, z2_values, b_values)
}

/// Evaluator preprocessing: mod - 1 - x
fn evaluator_preprocess_complex(inputs_x: &[ModInt]) -> Vec<u128> {
    inputs_x.iter().map(|x| {
        let z3 = x.val();
        println!("x: {}, z3: {}", x.val(), z3);
        z3
    }).collect()
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

/// Garbler side for complex comparison with threshold
/// Computes whether (x + y) mod modulus >= t using overflow detection
/// Circuit: negation of the <= circuit
/// Where z1 = t - y, z2 = mod - 1 - y, z3 = mod - 1 - x, b = y <= t
pub fn multiple_gb_complex_comparison<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_y: &[ModInt], // Garbler's y values
    inputs_t: &[ModInt], // Garbler's t values (thresholds)
) where
    C: AbstractChannel + Clone,
{
    let (z1_values, z2_values, b_values) = garbler_preprocess_complex(inputs_y, inputs_t);
    let bit_width = get_bit_width_from_modint(&inputs_y[0]);
    
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let circuit_wires = gb_set_complex_inputs(&mut gb, &z1_values, &z2_values, &b_values, bit_width);
    let results = fancy_complex_comparison(&mut gb, circuit_wires).unwrap();
    gb.outputs(results.wires()).unwrap();
    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();
}

/// Evaluator side for complex comparison
/// Provides x values, circuit computes whether (x + y) mod modulus >= t
pub fn multiple_ev_complex_comparison<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs_x: &[ModInt], // Evaluator's x values
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let z3_values = evaluator_preprocess_complex(inputs_x);
    let bit_width = get_bit_width_from_modint(&inputs_x[0]);
    
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let circuit_wires = ev_set_complex_inputs(&mut ev, &z3_values, bit_width);
    let results = fancy_complex_comparison(&mut ev, circuit_wires).unwrap();
    let outputs = ev.outputs(results.wires()).unwrap().unwrap();
    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();
    
    // Convert outputs to boolean results
    outputs.iter().map(|&x| x != 0).collect()
}

/// Core circuit logic: Complex comparison implementing (x + y) mod modulus >= t
/// This is the negation of the <= circuit
/// Let z1 = (t - y) mod modulus, z2 = (modulus - 1 - y), z3 = (modulus - 1 - x)
fn fancy_complex_comparison<F>(
    f: &mut F,
    wire_inputs: ComplexComparisonInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let mut final_results = Vec::new();

    for i in 0..wire_inputs.garbler_z1_wires.len() {
        let z1_wires = &wire_inputs.garbler_z1_wires[i];
        let z2_wires = &wire_inputs.garbler_z2_wires[i];
        let b_wire = &wire_inputs.garbler_b_wires[i];
        let z3_wires = &wire_inputs.evaluator_z3_wires[i];
        
        // Compute overflow(z3 + z2)
        // 1 is equivalent to x+y > mod - 1 
        // 0 is equivalent to x+y <= mod - 1
        let (sum1, carry1) = f.bin_addition(z3_wires, z2_wires)?;
        let x_plus_y_overflow = carry1; 
        let x_plus_y_not_overflow = f.negate(&x_plus_y_overflow)?;
        
        // Compute overflow(z3 + z1) 
        // 1 is equivalent to x > (t-y) mod modulus
        // 0 is equivalent to x <= (t-y) mod modulus
        let (sum2, carry2) = f.bin_addition(z3_wires, z1_wires)?;
        let overflow2 = carry2; 
        let x_lt_t_minus_y = f.negate(&overflow2)?;
        
        // First part: overflow(z3 + z2) AND (b AND overflow(z3 + z1))
        let b_and_overflow2 = f.and(b_wire, &x_lt_t_minus_y)?;
        let first_part = f.and(&x_plus_y_not_overflow, &b_and_overflow2)?;
        
        // Second part: not overflow1 AND (overflow2 OR b_wire)
        let overflow2_or_b = f.or(&x_lt_t_minus_y, b_wire)?;
        let second_part = f.and(&x_plus_y_overflow, &overflow2_or_b)?;

        // This computes (x + y) mod modulus <= t
        let le_result = f.or(&first_part, &second_part)?;
        
        // For >= t, we need to negate the <= result
        let final_result = f.negate(&le_result)?;
        
        final_results.push(final_result);
    }

    Ok(BinaryBundle::new(final_results))
}

/// Helper to set garbler inputs for complex comparison
fn gb_set_complex_inputs<F, E>(
    gb: &mut F,
    z1_values: &[u128], // t - y values
    z2_values: &[u128], // mod - 1 - y values  
    b_values: &[bool],  // y <= t bits
    bit_width: usize,
) -> ComplexComparisonInputs<F::Item>
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
    let b_moduli: Vec<u16> = vec![2; b_values.len()]; // modulus 2 for each bit
    let garbler_b_wires = gb.encode_many(&b_u16_values, &b_moduli).unwrap();
    
    // Receive evaluator's z3 values
    let evaluator_z3_wires = gb.bin_receive_many(z1_values.len(), bit_width).unwrap();


    ComplexComparisonInputs {
        garbler_z1_wires,
        garbler_z2_wires,
        garbler_b_wires,
        evaluator_z3_wires,
    }
}

/// Helper to set evaluator inputs for complex comparison
fn ev_set_complex_inputs<F, E>(
    ev: &mut F,
    z3_values: &[u128], // mod - 1 - x values
    bit_width: usize,
) -> ComplexComparisonInputs<F::Item>
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
    let b_moduli: Vec<u16> = vec![2; z3_values.len()]; // modulus 2 for each bit
    let garbler_b_wires = ev.receive_many(&b_moduli).unwrap();

    // Encode evaluator's z3 values
    let evaluator_z3_wires = ev.bin_encode_many(z3_values, bit_width).unwrap();

    ComplexComparisonInputs {
        garbler_z1_wires,
        garbler_z2_wires,
        garbler_b_wires,
        evaluator_z3_wires,
    }
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
