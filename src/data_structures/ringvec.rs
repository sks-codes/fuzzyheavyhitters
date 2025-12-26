use std::convert::TryInto;
use std::ops::{Add, Index, IndexMut, Mul, Sub};

use anyhow::{anyhow, ensure, Result};
use rand::Rng;

/// Runtime-length ring vector stored on the heap.
#[derive(Clone, Debug, PartialEq)]
pub struct RingVecDyn {
    vals: Vec<u128>,
    modulus_mask: u128, // Stores modulus - 1 for efficient bitwise modulo
}

/// Primary RingVec type (runtime length).
pub type RingVec = RingVecDyn;

impl RingVecDyn {
    /// Create a new ring vector from a Vec, masking all inputs by the modulus.
    pub fn new(vals: Vec<u128>, modulus: u128) -> Result<Self> {
        ensure!(modulus.is_power_of_two(), "Modulus must be a power of 2 and greater than 0");
        let modulus_mask = modulus - 1;
        Ok(Self {
            vals: vals.into_iter().map(|v| v & modulus_mask).collect(),
            modulus_mask,
        })
    }

    /// Alias for compatibility.
    pub fn from_vec(vals: Vec<u128>, modulus: u128) -> Result<Self> {
        Self::new(vals, modulus)
    }

    /// Create a zero-initialized ring vector of the given length.
    pub fn zero_with_len(len: usize, modulus: u128) -> Result<Self> {
        ensure!(modulus.is_power_of_two(), "Modulus must be a power of 2 and greater than 0");
        Ok(Self {
            vals: vec![0; len],
            modulus_mask: modulus - 1,
        })
    }

    /// Create a random ring vector of the given length.
    pub fn random_with_len(len: usize, modulus: u128) -> Result<Self> {
        ensure!(modulus.is_power_of_two(), "Modulus must be a power of 2 and greater than 0");
        let modulus_mask = modulus - 1;
        let mut rng = rand::rng();
        let vals = (0..len)
            .map(|_| rng.random::<u128>() & modulus_mask)
            .collect();
        Ok(Self { vals, modulus_mask })
    }

    pub fn len(&self) -> usize {
        self.vals.len()
    }

    /// Returns the modulus of this RingVec.
    pub fn modulus(&self) -> u128 {
        self.modulus_mask + 1
    }

    /// Returns the value slice of this RingVec.
    pub fn values(&self) -> &[u128] {
        &self.vals
    }

    /// Calculate bit width from modulus.
    pub fn modulus_bit_width(&self) -> usize {
        modulus_to_bit_width(self.modulus())
    }

    /// Alias for clarity in old call sites.
    pub fn calculate_bit_width(&self) -> usize {
        self.modulus_bit_width()
    }

    fn check_compat(&self, other: &Self) -> Result<()> {
        ensure!(
            self.len() == other.len(),
            "RingVec length mismatch ({} vs {})",
            self.len(),
            other.len()
        );
        ensure!(
            self.modulus_mask == other.modulus_mask,
            "RingVec modulus mismatch ({} vs {})",
            self.modulus(),
            other.modulus()
        );
        Ok(())
    }

    fn bin_op<F>(&self, other: &Self, op: F) -> Result<Self>
    where
        F: Fn(u128, u128) -> u128,
    {
        self.check_compat(other)?;
        let vals = self
            .vals
            .iter()
            .zip(other.vals.iter())
            .map(|(&a, &b)| op(a, b) & self.modulus_mask)
            .collect();
        Ok(Self {
            vals,
            modulus_mask: self.modulus_mask,
        })
    }

    fn scalar_op<F>(&self, scalar: u128, op: F) -> Self
    where
        F: Fn(u128, u128) -> u128,
    {
        let vals = self
            .vals
            .iter()
            .map(|&a| op(a, scalar) & self.modulus_mask)
            .collect();
        Self {
            vals,
            modulus_mask: self.modulus_mask,
        }
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self> {
        self.bin_op(other, |a, b| a + b)
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self> {
        let modulus = self.modulus();
        self.bin_op(other, move |a, b| a + modulus - b)
    }

    pub fn checked_mul(&self, other: &Self) -> Result<Self> {
        self.bin_op(other, |a, b| a * b)
    }

    /// Convert RingVec to compressed byte representation (length-prefixed).
    pub fn to_bytes(&self) -> Vec<u8> {
        let bit_width = self.modulus_bit_width();
        let data = compress_ring_vec_data(&self.vals, bit_width);
        let mut out = Vec::with_capacity(8 + data.len());
        out.extend_from_slice(&(self.vals.len() as u64).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    /// Create RingVec from compressed byte representation.
    /// Returns (RingVec, bytes_consumed) tuple.
    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        ensure!(bytes.len() >= 8, "Not enough bytes to read RingVec length");
        let len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
        let bit_width = modulus_to_bit_width(modulus);
        let total_bits = len
            .checked_mul(bit_width)
            .ok_or_else(|| anyhow!("Bit width overflow"))?;
        let padded_bits = (total_bits + 7) & !7;
        let data_bytes = padded_bits / 8;
        ensure!(
            bytes.len() >= 8 + data_bytes,
            "Not enough bytes to read RingVec values: need {}, have {}",
            8 + data_bytes,
            bytes.len()
        );
        let vals = decompress_ring_vec_data(&bytes[8..8 + data_bytes], len, bit_width)?;
        let ring_vec = RingVecDyn::from_vec(vals, modulus)?;
        Ok((ring_vec, 8 + data_bytes))
    }

    /// Calculate the byte size of this RingVec's compressed representation.
    pub fn byte_size(&self) -> usize {
        let bit_width = self.modulus_bit_width();
        let total_bits = self.vals.len() * bit_width;
        let padded_bits = (total_bits + 7) & !7;
        8 + padded_bits / 8
    }

    /// Calculate the byte size for a given length and modulus.
    pub fn byte_size_for_modulus_len(len: usize, modulus: u128) -> usize {
        let bit_width = modulus_to_bit_width(modulus);
        let total_bits = len * bit_width;
        let padded_bits = (total_bits + 7) & !7;
        8 + padded_bits / 8
    }

    /// Convenience wrapper for const generic length.
    pub fn byte_size_for_modulus<const N: usize>(modulus: u128) -> usize {
        RingVecDyn::byte_size_for_modulus_len(N, modulus)
    }

    /// Calculate the uncompressed size in bytes (using u128 per element).
    pub fn uncompressed_size_bytes(len: usize) -> usize {
        len * std::mem::size_of::<u128>()
    }
}

impl Index<usize> for RingVecDyn {
    type Output = u128;
    fn index(&self, index: usize) -> &Self::Output {
        &self.vals[index]
    }
}

impl IndexMut<usize> for RingVecDyn {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.vals[index]
    }
}

impl Add for RingVecDyn {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        self.checked_add(&other)
            .expect("RingVec add requires matching length and modulus")
    }
}

impl Add<&RingVecDyn> for &RingVecDyn {
    type Output = RingVecDyn;
    fn add(self, other: &RingVecDyn) -> RingVecDyn {
        self.checked_add(other)
            .expect("RingVec add requires matching length and modulus")
    }
}

impl Sub for RingVecDyn {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        self.checked_sub(&other)
            .expect("RingVec sub requires matching length and modulus")
    }
}

impl Sub<&RingVecDyn> for &RingVecDyn {
    type Output = RingVecDyn;
    fn sub(self, other: &RingVecDyn) -> RingVecDyn {
        self.checked_sub(other)
            .expect("RingVec sub requires matching length and modulus")
    }
}

impl Mul for RingVecDyn {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        self.checked_mul(&other)
            .expect("RingVec mul requires matching length and modulus")
    }
}

impl Mul<&RingVecDyn> for &RingVecDyn {
    type Output = RingVecDyn;
    fn mul(self, other: &RingVecDyn) -> RingVecDyn {
        self.checked_mul(other)
            .expect("RingVec mul requires matching length and modulus")
    }
}

impl Add<u128> for RingVecDyn {
    type Output = Self;
    fn add(self, rhs: u128) -> Self::Output {
        self.scalar_op(rhs, |a, b| a + b)
    }
}

impl Sub<u128> for RingVecDyn {
    type Output = Self;
    fn sub(self, rhs: u128) -> Self::Output {
        let modulus = self.modulus();
        self.scalar_op(rhs, |a, b| a + modulus - b)
    }
}

impl Mul<u128> for RingVecDyn {
    type Output = Self;
    fn mul(self, rhs: u128) -> Self::Output {
        self.scalar_op(rhs, |a, b| a * b)
    }
}

/// Compress an array of u128 values using adaptive bit width.
/// Uses bit-perfect packing: concatenate all bits then pack into bytes.
fn compress_ring_vec_data(values: &[u128], bits_per_element: usize) -> Vec<u8> {
    if bits_per_element == 0 {
        return Vec::new();
    }

    let total_bits = values.len() * bits_per_element;
    let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
    let total_bytes = padded_bits / 8;

    let mut compressed = vec![0u8; total_bytes];

    // Pack all values bit by bit into the byte array.
    for (element_idx, &value) in values.iter().enumerate() {
        let bit_start = element_idx * bits_per_element;

        // Extract each bit from the value and pack it.
        for bit_pos in 0..bits_per_element {
            let bit_value = (value >> bit_pos) & 1;
            let global_bit_pos = bit_start + bit_pos;

            if bit_value == 1 {
                let byte_idx = global_bit_pos / 8;
                let bit_in_byte = global_bit_pos % 8;
                compressed[byte_idx] |= 1u8 << bit_in_byte;
            }
        }
    }

    compressed
}

/// Decompress byte data back into an array of u128 values.
fn decompress_ring_vec_data(
    compressed: &[u8],
    len: usize,
    bits_per_element: usize,
) -> Result<Vec<u128>> {
    if bits_per_element == 0 {
        return Ok(vec![0; len]);
    }

    let mut values = vec![0u128; len];

    for element_idx in 0..len {
        let bit_start = element_idx * bits_per_element;
        let mut value = 0u128;

        for bit_pos in 0..bits_per_element {
            let global_bit_pos = bit_start + bit_pos;
            let byte_idx = global_bit_pos / 8;
            let bit_in_byte = global_bit_pos % 8;

            ensure!(
                byte_idx < compressed.len(),
                "Not enough bytes to decompress element {}",
                element_idx
            );

            let bit_value = (compressed[byte_idx] >> bit_in_byte) & 1;
            if bit_value == 1 {
                value |= 1u128 << bit_pos;
            }
        }

        values[element_idx] = value;
    }

    Ok(values)
}

fn modulus_to_bit_width(modulus: u128) -> usize {
    if modulus <= 1 {
        1
    } else {
        (128 - (modulus - 1).leading_zeros()) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_bytes_from_bytes() {
        // Test with 10-bit values (modulus = 1024)
        let modulus = 1024u128; // 2^10
        let values = vec![123, 456, 789, 1000];
        let ring_vec = RingVecDyn::new(values, modulus).unwrap();

        // Test byte conversion
        let bytes = ring_vec.to_bytes();
        let (reconstructed, bytes_consumed) = RingVecDyn::from_bytes(&bytes, modulus).unwrap();

        // Check that values are preserved
        for i in 0..4 {
            assert_eq!(ring_vec[i], reconstructed[i]);
        }

        // Verify modulus is preserved
        assert_eq!(ring_vec.modulus(), reconstructed.modulus());

        // Verify bytes consumed matches bytes length
        assert_eq!(bytes_consumed, bytes.len());

        // Check compression effectiveness
        let uncompressed_size = RingVecDyn::uncompressed_size_bytes(4);
        let compressed_size = bytes.len();
        println!(
            "10-bit compression: {} bytes -> {} bytes ({:.1}x reduction)",
            uncompressed_size,
            compressed_size,
            uncompressed_size as f64 / compressed_size as f64
        );

        // Should be significantly compressed (64 bytes -> ~5 bytes for 10-bit values)
        assert!(compressed_size < uncompressed_size / 5);

        // Verify expected byte size calculation
        assert_eq!(compressed_size, ring_vec.byte_size());
        assert_eq!(
            compressed_size,
            RingVecDyn::byte_size_for_modulus_len(4, modulus)
        );
    }

    #[test]
    fn test_binary_compression() {
        // Test with 1-bit values (modulus = 2)
        let modulus = 2u128;
        let values = vec![0, 1, 0, 1, 1, 0, 1, 0];
        let ring_vec = RingVecDyn::new(values, modulus).unwrap();

        let bytes = ring_vec.to_bytes();
        let (reconstructed, _) = RingVecDyn::from_bytes(&bytes, modulus).unwrap();

        for i in 0..8 {
            assert_eq!(ring_vec[i], reconstructed[i]);
        }

        // Verify extreme compression for binary values
        let compressed_size = bytes.len();
        let uncompressed_size = RingVecDyn::uncompressed_size_bytes(8);
        println!(
            "Binary compression: {} bytes -> {} bytes ({:.1}x reduction)",
            uncompressed_size,
            compressed_size,
            uncompressed_size as f64 / compressed_size as f64
        );

        // Should compress 8*16=128 bytes down to a few bytes for binary values
        assert!(compressed_size <= 2);
    }

    #[test]
    fn test_different_moduli() {
        // Test various moduli to ensure correct bit width calculation
        let test_cases = [
            (2u128, 1),     // 1 bit
            (4u128, 2),     // 2 bits
            (8u128, 3),     // 3 bits
            (16u128, 4),    // 4 bits
            (256u128, 8),   // 8 bits
            (1024u128, 10), // 10 bits
            (65536u128, 16), // 16 bits
        ];

        for (modulus, expected_bits) in test_cases {
            let values = vec![1, 2, 3, 4];
            let ring_vec = RingVecDyn::new(values.clone(), modulus).unwrap();

            assert_eq!(ring_vec.modulus_bit_width(), expected_bits);

            let bytes = ring_vec.to_bytes();
            let (reconstructed, bytes_consumed) =
                RingVecDyn::from_bytes(&bytes, modulus).unwrap();

            for i in 0..4 {
                assert_eq!(ring_vec[i], reconstructed[i]);
            }

            // Expected byte size: ceil(4 * expected_bits / 8) + 8-byte length prefix
            let expected_byte_size = 8 + (4 * expected_bits + 7) / 8;
            assert_eq!(bytes.len(), expected_byte_size);
            assert_eq!(bytes_consumed, expected_byte_size);

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
        let result = RingVecDyn::from_bytes(&bytes, 3);
        assert!(result.is_err());

        // Test insufficient bytes (claims length 0 because only prefix? ensure checks)
        let short_bytes = vec![0x01]; // Too few bytes for length prefix
        let result = RingVecDyn::from_bytes(&short_bytes, 1024);
        assert!(result.is_err());
    }
}
