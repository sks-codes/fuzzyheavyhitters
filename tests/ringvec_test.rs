use mosaic::data_structures::ringvec::RingVec;
use std::mem;

#[test]
fn test_to_bytes_from_bytes() {
    // Test with 10-bit values (modulus = 1024)
    let modulus = 1024u128; // 2^10
    let values = vec![123, 456, 789, 1000];
    let ring_vec = RingVec::new(values, modulus).unwrap();

    // Test byte conversion
    let bytes = ring_vec.to_bytes().unwrap();
    let (reconstructed, bytes_consumed) =
        RingVec::from_bytes(&bytes, modulus, ring_vec.len()).unwrap();

    // Check that values are preserved
    for i in 0..4 {
        assert_eq!(ring_vec[i], reconstructed[i]);
    }

    // Verify modulus is preserved
    assert_eq!(ring_vec.modulus(), reconstructed.modulus());

    // Verify bytes consumed matches bytes length
    assert_eq!(bytes_consumed, bytes.len());

    // Check compression effectiveness
    let uncompressed_size = RingVec::uncompressed_size_bytes(4);
    let compressed_size = bytes.len();
    println!(
        "10-bit compression: {} bytes -> {} bytes ({:.1}x reduction)",
        uncompressed_size,
        compressed_size,
        uncompressed_size as f64 / compressed_size as f64
    );

    // Should be smaller than the naive uncompressed representation.
    assert!(compressed_size < uncompressed_size);

    // Verify expected byte size calculation
    let expected_total_size = mem::size_of::<usize>() + compressed_size;
    assert_eq!(expected_total_size, ring_vec.byte_size());
    assert_eq!(
        expected_total_size,
        RingVec::byte_size_for_modulus_len(4, modulus)
    );
}

#[test]
fn test_binary_compression() {
    // Test with 1-bit values (modulus = 2)
    let modulus = 2u128;
    let values = vec![0, 1, 0, 1, 1, 0, 1, 0];
    let ring_vec = RingVec::new(values, modulus).unwrap();

    let bytes = ring_vec.to_bytes().unwrap();
    let (reconstructed, _) = RingVec::from_bytes(&bytes, modulus, ring_vec.len()).unwrap();

    for i in 0..8 {
        assert_eq!(ring_vec[i], reconstructed[i]);
    }

    // Verify extreme compression for binary values
    let compressed_size = bytes.len();
    let uncompressed_size = RingVec::uncompressed_size_bytes(8);
    println!(
        "Binary compression: {} bytes -> {} bytes ({:.1}x reduction)",
        uncompressed_size,
        compressed_size,
        uncompressed_size as f64 / compressed_size as f64
    );

    // Should compress below the naive uncompressed size for binary values.
    assert!(compressed_size < uncompressed_size);
}

#[test]
fn test_different_moduli() {
    // Test various moduli to ensure correct bit width calculation
    let test_cases = [
        (2u128, 1),      // 1 bit
        (4u128, 2),      // 2 bits
        (8u128, 3),      // 3 bits
        (16u128, 4),     // 4 bits
        (256u128, 8),    // 8 bits
        (1024u128, 10),  // 10 bits
        (65536u128, 16), // 16 bits
    ];

    for (modulus, expected_bits) in test_cases {
        let values = vec![1, 2, 3, 4];
        let ring_vec = RingVec::new(values.clone(), modulus).unwrap();

        assert_eq!(ring_vec.modulus_bit_width(), expected_bits);

        let bytes = ring_vec.to_bytes().unwrap();
        let (reconstructed, bytes_consumed) =
            RingVec::from_bytes(&bytes, modulus, ring_vec.len()).unwrap();

        for i in 0..4 {
            assert_eq!(ring_vec[i], reconstructed[i]);
        }

        // Expected byte size: ceil(4 * expected_bits / 8) + 8-byte length prefix
        let expected_data_size = (4 * expected_bits + 7) / 8;
        let expected_byte_size = mem::size_of::<usize>() + expected_data_size;
        assert_eq!(bytes.len(), expected_data_size);
        assert_eq!(bytes_consumed, expected_data_size);
        assert_eq!(expected_byte_size, RingVec::byte_size_for_modulus_len(4, modulus));

        println!(
            "Modulus {} ({} bits): {} bytes",
            modulus,
            expected_bits,
            bytes.len()
        );
    }
}

#[test]
fn test_error_cases() {
    // Test invalid modulus
    let bytes = vec![0x12, 0x34];

    // Non-power-of-2 modulus should fail in RingVec::from_bytes -> RingVec::new
    let result = RingVec::from_bytes(&bytes, 3, 1);
    assert!(result.is_err());

    // Test insufficient bytes (claims length 0 because only prefix? ensure checks)
    let short_bytes = vec![0x01]; // Too few bytes for length prefix
    let result = RingVec::from_bytes(&short_bytes, 1024, 1);
    assert!(result.is_err());
}
