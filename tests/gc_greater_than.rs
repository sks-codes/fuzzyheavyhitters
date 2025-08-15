use std::os::unix::net::UnixStream;
use std::io::{BufReader, BufWriter};
use scuttlebutt::{AesRng, Channel};
use counttree::garbled_circuits::greater_than::{
    multiple_gb_greater_than, multiple_ev_greater_than, BitWidth, u128_to_u16_bits,
};

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
