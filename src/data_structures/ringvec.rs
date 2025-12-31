use std::ops::{Add, Index, IndexMut, Mul, Sub};

use anyhow::{ensure, Result};
use rand::Rng;

/// Runtime-length ring vector stored on the heap.
#[derive(Clone, Debug, PartialEq)]
pub struct RingVec {
    vals: Vec<u128>,
    modulus_mask: u128, // Stores modulus - 1 for efficient bitwise modulo
}

impl RingVec {
    /// Create a new ring vector from a Vec, masking all inputs by the modulus.
    pub fn new(vals: Vec<u128>, modulus: u128) -> Result<Self> {
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and greater than 0"
        );
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
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and greater than 0"
        );
        Ok(Self {
            vals: vec![0; len],
            modulus_mask: modulus - 1,
        })
    }

    /// Create a random ring vector of the given length.
    pub fn random_with_len(len: usize, modulus: u128) -> Result<Self> {
        ensure!(
            modulus.is_power_of_two(),
            "Modulus must be a power of 2 and greater than 0"
        );
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
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let bit_width = self.modulus_bit_width();
        let total_bits = self.len() * bit_width;
        let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
        let total_bytes = padded_bits / 8;

        let mut compressed = vec![0u8; total_bytes];

        // Pack all values bit by bit into the byte array.
        for (element_idx, &value) in self.vals.iter().enumerate() {
            let bit_start = element_idx * bit_width;

            // Extract each bit from the value and pack it.
            for bit_pos in 0..bit_width {
                let bit_value = (value >> bit_pos) & 1;
                let global_bit_pos = bit_start + bit_pos;

                if bit_value == 1 {
                    let byte_idx = global_bit_pos / 8;
                    let bit_in_byte = global_bit_pos % 8;
                    compressed[byte_idx] |= 1u8 << bit_in_byte;
                }
            }
        }

        Ok(compressed)
    }

    /// Create RingVec from compressed byte representation.
    /// Returns (RingVec, bytes_consumed) tuple.
    pub fn from_bytes(bytes: &[u8], modulus: u128, length: usize) -> Result<(Self, usize)> {
        let bit_width = modulus_to_bit_width(modulus);

        let total_bits = length * bit_width;
        let padded_bits = (total_bits + 7) & !7;
        let data_bytes = padded_bits / 8;
        ensure!(
            bytes.len() >= data_bytes,
            "Not enough bytes to read RingVec values: need {}, have {}",
            data_bytes,
            bytes.len()
        );

        let mut values = vec![0u128; length];
        for element_idx in 0..length {
            let bit_start = element_idx * bit_width;
            let mut value = 0u128;
            for bit_pos in 0..bit_width {
                let global_bit_pos = bit_start + bit_pos;
                let byte_idx = global_bit_pos / 8;
                let bit_in_byte = global_bit_pos % 8;

                ensure!(
                    byte_idx < bytes.len(),
                    "Not enough bytes to decompress element {}",
                    element_idx
                );

                let bit_value = (bytes[byte_idx] >> bit_in_byte) & 1;
                if bit_value == 1 {
                    value |= 1u128 << bit_pos;
                }
            }
            values[element_idx] = value;
        }

        let ring_vec = RingVec::from_vec(values, modulus)?;
        Ok((ring_vec, data_bytes))
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
        RingVec::byte_size_for_modulus_len(N, modulus)
    }

    /// Calculate the uncompressed size in bytes (using u128 per element).
    pub fn uncompressed_size_bytes(len: usize) -> usize {
        len * std::mem::size_of::<u128>()
    }
}

impl Index<usize> for RingVec {
    type Output = u128;
    fn index(&self, index: usize) -> &Self::Output {
        &self.vals[index]
    }
}

impl IndexMut<usize> for RingVec {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.vals[index]
    }
}

impl Add for RingVec {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        self.checked_add(&other)
            .expect("RingVec add requires matching length and modulus")
    }
}

impl Add<&RingVec> for &RingVec {
    type Output = RingVec;
    fn add(self, other: &RingVec) -> RingVec {
        self.checked_add(other)
            .expect("RingVec add requires matching length and modulus")
    }
}

impl Sub for RingVec {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        self.checked_sub(&other)
            .expect("RingVec sub requires matching length and modulus")
    }
}

impl Sub<&RingVec> for &RingVec {
    type Output = RingVec;
    fn sub(self, other: &RingVec) -> RingVec {
        self.checked_sub(other)
            .expect("RingVec sub requires matching length and modulus")
    }
}

impl Mul for RingVec {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        self.checked_mul(&other)
            .expect("RingVec mul requires matching length and modulus")
    }
}

impl Mul<&RingVec> for &RingVec {
    type Output = RingVec;
    fn mul(self, other: &RingVec) -> RingVec {
        self.checked_mul(other)
            .expect("RingVec mul requires matching length and modulus")
    }
}

impl Add<u128> for RingVec {
    type Output = Self;
    fn add(self, rhs: u128) -> Self::Output {
        self.scalar_op(rhs, |a, b| a + b)
    }
}

impl Sub<u128> for RingVec {
    type Output = Self;
    fn sub(self, rhs: u128) -> Self::Output {
        let modulus = self.modulus();
        self.scalar_op(rhs, |a, b| a + modulus - b)
    }
}

impl Mul<u128> for RingVec {
    type Output = Self;
    fn mul(self, rhs: u128) -> Self::Output {
        self.scalar_op(rhs, |a, b| a * b)
    }
}

fn modulus_to_bit_width(modulus: u128) -> usize {
    if modulus <= 1 {
        1
    } else {
        (128 - (modulus - 1).leading_zeros()) as usize
    }
}
