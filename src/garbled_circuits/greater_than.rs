use fancy_garbling::{
    AllWire, BinaryBundle, BinaryGadgets, Fancy, FancyArithmetic, FancyBinary, FancyInput,
    FancyReveal,
    twopac::semihonest::{Evaluator, Garbler},
};
use ocelot::{ot::AlszReceiver as OtReceiver, ot::AlszSender as OtSender};
use scuttlebutt::{AbstractChannel, AesRng, Block};
use std::fmt::Debug;

/// Bit width for comparison
#[derive(Clone, Copy)]
pub enum BitWidth {
    Bits128,
    Bits256,
}

/// Input structure for multiple comparisons
struct MultiComparisonInputs<F> {
    pub garbler_wires: Vec<BinaryBundle<F>>,
    pub evaluator_wires: Vec<BinaryBundle<F>>,
}

/// Convert a u128 to Vec<u16> bits (LSB first)
fn u128_to_u16_bits(x: u128, n: usize) -> Vec<u16> {
    (0..n).map(|i| ((x >> i) & 1) as u16).collect()
}

/// Convert a Vec<bool> (LSB first) into u128 values
fn bool_vec_to_value(bits: &[bool], bit_width: BitWidth) -> Vec<u128> {
    match bit_width {
        BitWidth::Bits128 => {
            let value = bits.iter()
                .enumerate()
                .fold(0u128, |acc, (i, &b)| acc | ((b as u128) << i));
            vec![value]
        }
        BitWidth::Bits256 => {
            assert!(bits.len() <= 256, "Input too large for 256-bit comparison");
            let mut values = vec![0u128; 2];

            // First 128 bits are low bits (LSB first)
            for (i, &b) in bits.iter().take(128).enumerate() {
                values[0] |= (b as u128) << i;
            }

            // Next 128 bits are high bits (if they exist)
            for (i, &b) in bits.iter().skip(128).enumerate() {
                values[1] |= (b as u128) << i;
            }

            values
        }
    }
}

/// Garbler side for multiple comparisons
pub fn multiple_gb_greater_than<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<u16>],
    bit_width: BitWidth,
) where
    C: AbstractChannel + Clone,
{
    let mut gb = Garbler::<C, AesRng, OtSender, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let circuit_wires = gb_set_multi_inputs(&mut gb, inputs, bit_width);
    let results = fancy_multi_greater_than(&mut gb, circuit_wires).unwrap();
    gb.outputs(results.wires()).unwrap();
    channel.flush().unwrap();
    let mut ack = [0u8; 1];
    channel.read_bytes(&mut ack).unwrap();
}

/// Evaluator side for multiple comparisons
pub fn multiple_ev_greater_than<C>(
    rng: &mut AesRng,
    channel: &mut C,
    inputs: &[Vec<u16>],
    bit_width: BitWidth,
) -> Vec<bool>
where
    C: AbstractChannel + Clone,
{
    let mut ev = Evaluator::<C, AesRng, OtReceiver, AllWire>::new(channel.clone(), rng.clone()).unwrap();
    let circuit_wires = ev_set_multi_inputs(&mut ev, inputs, bit_width);
    let results = fancy_multi_greater_than(&mut ev, circuit_wires).unwrap();
    let outputs = ev.outputs(results.wires()).unwrap().unwrap();
    channel.write_bytes(&[1u8]).unwrap();
    channel.flush().unwrap();
    outputs.iter().map(|&x| x == 0).collect()
}

/// Core comparison logic
fn fancy_multi_greater_than<F>(
    f: &mut F,
    wire_inputs: MultiComparisonInputs<F::Item>,
) -> Result<BinaryBundle<F::Item>, F::Error>
where
    F: FancyReveal + Fancy + BinaryGadgets + FancyBinary + FancyArithmetic,
{
    let mut comparison_results = Vec::new();

    for (garbler_wires, evaluator_wires) in wire_inputs.garbler_wires.into_iter()
        .zip(wire_inputs.evaluator_wires.into_iter())
    {
        let res = f.bin_lt(&garbler_wires, &evaluator_wires)?;
        comparison_results.push(res);
    }

    Ok(BinaryBundle::new(comparison_results))
}

/// Helper to set multiple garbler inputs
fn gb_set_multi_inputs<F, E>(
    gb: &mut F,
    inputs: &[Vec<u16>],
    bit_width: BitWidth,
) -> MultiComparisonInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let nbits = inputs[0].len();
    let values: Vec<Vec<u128>> = inputs.iter()
        .map(|input| {
            // Convert u16 bits back to bool for value conversion
            let bits = input.iter().map(|&x| x != 0).collect::<Vec<bool>>();
            bool_vec_to_value(&bits, bit_width)
        })
        .collect();

    let flat_values: Vec<u128> = values.iter().flatten().cloned().collect();

    let values_per_input = match bit_width {
        BitWidth::Bits128 => 1,
        BitWidth::Bits256 => 2,
    };

    let garbler_wires = gb.bin_encode_many(&flat_values, nbits / values_per_input).unwrap();

    let garbler_wires = garbler_wires.chunks(values_per_input)
        .map(|chunk| {
            if chunk.len() == 1 {
                chunk[0].clone()
            } else {
                let mut combined = chunk[0].wires().to_vec();
                combined.extend(chunk[1].wires().to_vec());
                BinaryBundle::new(combined)
            }
        })
        .collect();

    let evaluator_wires = gb.bin_receive_many(inputs.len(), nbits).unwrap();

    MultiComparisonInputs { garbler_wires, evaluator_wires }
}

/// Helper to set multiple evaluator inputs
fn ev_set_multi_inputs<F, E>(
    ev: &mut F,
    inputs: &[Vec<u16>],
    bit_width: BitWidth,
) -> MultiComparisonInputs<F::Item>
where
    F: FancyInput<Item = AllWire, Error = E>,
    E: Debug,
{
    let nbits = inputs[0].len();
    let garbler_wires = ev.bin_receive_many(inputs.len(), nbits).unwrap();

    let values: Vec<Vec<u128>> = inputs.iter()
        .map(|input| {
            let bits = input.iter().map(|&x| x != 0).collect::<Vec<bool>>();
            bool_vec_to_value(&bits, bit_width)
        })
        .collect();

    let flat_values: Vec<u128> = values.iter().flatten().cloned().collect();

    let values_per_input = match bit_width {
        BitWidth::Bits128 => 1,
        BitWidth::Bits256 => 2,
    };

    let evaluator_wires = ev.bin_encode_many(&flat_values, nbits / values_per_input).unwrap();

    let evaluator_wires = evaluator_wires.chunks(values_per_input)
        .map(|chunk| {
            if chunk.len() == 1 {
                chunk[0].clone()
            } else {
                let mut combined = chunk[0].wires().to_vec();
                combined.extend(chunk[1].wires().to_vec());
                BinaryBundle::new(combined)
            }
        })
        .collect();

    MultiComparisonInputs { garbler_wires, evaluator_wires }
}

#[test]
fn test_multi_greater_than_128() {
    let gb_values = vec![
        u128_to_u16_bits(5, 4),   // 0101 (LSB first)
        u128_to_u16_bits(10, 5),  // 01010
        u128_to_u16_bits(15, 5),  // 01111
    ];
    let ev_values = vec![
        u128_to_u16_bits(3, 4),   // 1100
        u128_to_u16_bits(12, 5),  // 00110
        u128_to_u16_bits(15, 5),  // 01111
    ];

    let expected: Vec<bool> = gb_values.iter()
        .zip(ev_values.iter())
        .map(|(g, e)| {
            let g_val = bool_vec_to_value(&g.iter().map(|&x| x != 0).collect::<Vec<bool>>(), BitWidth::Bits128)[0];
            let e_val = bool_vec_to_value(&e.iter().map(|&x| x != 0).collect::<Vec<bool>>(), BitWidth::Bits128)[0];
            g_val >= e_val
        })
        .collect();

    let (sender, receiver) = UnixStream::pair().unwrap();

    std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        multiple_gb_greater_than(&mut rng_gb.clone(), &mut channel, &gb_values, BitWidth::Bits128);
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_greater_than(&mut rng_ev.clone(), &mut channel, &ev_values, BitWidth::Bits128);
    assert_eq!(results, expected);
}

#[test]
fn test_multi_greater_than_256() {
    // Helper to create LSB-ordered 256-bit values
    fn make_value(high: u128, low: u128) -> Vec<u16> {
        let mut bits = Vec::with_capacity(256);
        // Low bits first (LSB)
        bits.extend(u128_to_u16_bits(low, 128));
        // Then high bits
        bits.extend(u128_to_u16_bits(high, 128));
        bits
    }

    // Garbler values
    let gb_values = vec![
        make_value(1, 0),  // High=1, Low=0
        make_value(0, 1),  // High=0, Low=1
    ];

    // Evaluator values
    let ev_values = vec![
        make_value(0, 1),  // High=0, Low=1
        make_value(1, 0),  // High=1, Low=0
    ];

    // Expected results: gb_value >= ev_value
    let expected = vec![true, false];

    let (sender, receiver) = UnixStream::pair().unwrap();

    std::thread::spawn(move || {
        let rng_gb = AesRng::new();
        let reader = BufReader::new(sender.try_clone().unwrap());
        let writer = BufWriter::new(sender);
        let mut channel = Channel::new(reader, writer);
        multiple_gb_greater_than(&mut rng_gb.clone(), &mut channel, &gb_values, BitWidth::Bits256);
    });

    let rng_ev = AesRng::new();
    let reader = BufReader::new(receiver.try_clone().unwrap());
    let writer = BufWriter::new(receiver);
    let mut channel = Channel::new(reader, writer);

    let results = multiple_ev_greater_than(&mut rng_ev.clone(), &mut channel, &ev_values, BitWidth::Bits256);
    assert_eq!(results, expected);
}

pub fn block_to_bits(block: Block, lsb_first: bool) -> Vec<u16> {
    let bytes = block.as_ref();
    let mut bits = Vec::with_capacity(128);

    if lsb_first {
        for byte in bytes.iter() {
            for i in 0..8 {
                bits.push(((*byte >> i) & 1) as u16);
            }
        }
    } else {
        for byte in bytes.iter().rev() {
            for i in (0..8).rev() {
                bits.push(((*byte >> i) & 1) as u16);
            }
        }
    }

    bits
}