use std::ops::{Add, Sub, Mul, BitAnd, BitXor, Index, IndexMut};
use rand::Rng;

#[derive(Clone, Debug, Copy, PartialEq)]
pub struct RingVec<const N: usize> {
    val: [u128; N],
    modulus: u128,
    modulus_mask: u128, // Stores modulus - 1 for efficient bitwise modulo
}

impl<const N: usize> RingVec<N> {
    pub fn new(val: [u128; N], modulus: u128) -> Self {
        if modulus == 0 || (modulus & (modulus - 1)) != 0 {
            panic!("Modulus must be a power of 2 and greater than 0");
        }
        let modulus_mask = modulus - 1;
        let mut new_val = [0u128; N];
        for i in 0..N {
            new_val[i] = val[i] & modulus_mask; // Apply modulus initially
        }
        RingVec {
            val: new_val,
            modulus,
            modulus_mask,
        }
    }

    pub fn zero(modulus: u128) -> Self {
        if modulus == 0 || (modulus & (modulus - 1)) != 0 {
            panic!("Modulus must be a power of 2 and greater than 0");
        }
        RingVec {
            val: [0; N],
            modulus,
            modulus_mask: modulus - 1,
        }
    }

    pub fn random(modulus: u128) -> Self {
        if modulus == 0 || (modulus & (modulus - 1)) != 0 {
            panic!("Modulus must be a power of 2 and greater than 0");
        }
        let modulus_mask = modulus - 1;
        let mut val = [0u128; N];
        for i in 0..N {
            val[i] = rand::rng().random::<u128>() & modulus_mask;
        }
        RingVec {
            val,
            modulus,
            modulus_mask,
        }
    }

    /// Calculate the minimum number of bits needed to represent all values
    pub fn calculate_bit_width(&self) -> usize {
        let max_val = self.val.iter().max().unwrap_or(&0);
        if *max_val == 0 {
            1 // Need at least 1 bit even for zero
        } else {
            (128 - max_val.leading_zeros()) as usize
        }
    }

    /// Calculate bit width from modulus
    pub fn modulus_bit_width(&self) -> usize {
        if self.modulus <= 1 {
            1
        } else {
            (128 - (self.modulus - 1).leading_zeros()) as usize
        }
    }

    fn element_wise_ringvec_op<F>(mut self, other: &Self, op: F) -> Self
    where
        F: Fn(u128, u128) -> u128,
    {
        debug_assert_eq!(self.modulus, other.modulus, "RingVec operations require matching moduli.");

        for i in 0..N {
            self.val[i] = op(self.val[i], other.val[i]) & self.modulus_mask;
        }
        self
    }

    fn element_wise_scalar_op<F>(mut self, scalar: u128, op: F) -> Self
    where
        F: Fn(u128, u128) -> u128,
    {
        for i in 0..N {
            self.val[i] = op(self.val[i], scalar) & self.modulus_mask;
        }
        self
    }

    /// Returns the modulus of this RingVec.
    pub fn modulus(&self) -> u128 {
        self.modulus
    }

    /// Returns the value array of this RingVec.
    pub fn val(&self) -> &[u128; N] {
        &self.val
    }
}

impl<const N: usize> RingVec<N> {
    /// Create a RingVec from a Vec, useful for dynamic creation
    pub fn from_vec(vec: Vec<u128>, modulus: u128) -> Result<Self, String> {
        if vec.len() != N {
            return Err(format!("Expected vector of length {}, got {}", N, vec.len()));
        }
        
        let mut arr = [0u128; N];
        for (i, val) in vec.into_iter().enumerate() {
            arr[i] = val;
        }
        
        Ok(Self::new(arr, modulus))
    }
    
    /// Convert to a Vec, useful for dynamic operations
    pub fn to_vec(&self) -> Vec<u128> {
        self.val.to_vec()
    }
    
    /// Convert RingVec to compressed byte representation
    /// Uses modulus-based bit width for optimal compression
    pub fn to_bytes(&self) -> Vec<u8> {
        let bit_width = self.modulus_bit_width();
        compress_ring_vec_data_adaptive(&self.val, bit_width)
    }
    
    /// Create RingVec from compressed byte representation
    /// Returns (RingVec, bytes_consumed) tuple to match pattern used in interval structures
    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize), String> {
        // Calculate bit width from modulus
        let bit_width = if modulus <= 1 {
            1
        } else {
            (128 - (modulus - 1).leading_zeros()) as usize
        };
        
        let total_bits = N * bit_width;
        let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
        let bytes_consumed = padded_bits / 8;
        
        if bytes.len() < bytes_consumed {
            return Err(format!("Not enough bytes: need {}, got {}", bytes_consumed, bytes.len()));
        }
        
        let val = decompress_ring_vec_data_adaptive::<N>(&bytes[..bytes_consumed], bit_width)?;
        
        let ring_vec = RingVec {
            val,
            modulus,
            modulus_mask: modulus - 1,
        };
        
        Ok((ring_vec, bytes_consumed))
    }
    
    /// Calculate the byte size needed for this RingVec's compressed representation
    pub fn byte_size(&self) -> usize {
        let bit_width = self.modulus_bit_width();
        let total_bits = N * bit_width;
        let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
        padded_bits / 8
    }
    
    /// Calculate the byte size needed for a given modulus and array size
    pub fn byte_size_for_modulus(modulus: u128) -> usize {
        let bit_width = if modulus <= 1 {
            1
        } else {
            (128 - (modulus - 1).leading_zeros()) as usize
        };
        let total_bits = N * bit_width;
        let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
        padded_bits / 8
    }
    
    /// Calculate the uncompressed size in bytes (using u128 per element)
    pub fn uncompressed_size_bytes() -> usize {
        N * std::mem::size_of::<u128>()
    }
    
}

// --- RingVec-to-RingVec Operations ---
// Note: These implementations implicitly assume matching moduli.
// In a real-world scenario, you might want to return a Result or panic
// if moduli don't match, or define a clear conversion strategy.

impl<const N: usize> Add for RingVec<N> {
    type Output = Self;
    fn add(self, other: Self) -> Self::Output {
        self.element_wise_ringvec_op(&other, |a, b| a + b)
    }
}

impl<const N: usize> Sub for RingVec<N> {
    type Output = Self;
    fn sub(self, other: Self) -> Self::Output {
        let modulus = self.modulus;
        self.element_wise_ringvec_op(&other, |a, b| a + modulus - b)
    }
}

impl<const N: usize> Mul for RingVec<N> {
    type Output = Self;
    fn mul(self, other: Self) -> Self::Output {
        self.element_wise_ringvec_op(&other, |a, b| a * b)
    }
}

// --- Indexing Implementations ---

impl<const N: usize> Index<usize> for RingVec<N> {
    type Output = u128;
    fn index(&self, index: usize) -> &Self::Output {
        &self.val[index]
    }
}

impl<const N: usize> IndexMut<usize> for RingVec<N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let val_ref = &mut self.val[index];
        val_ref
    }
}


// --- Implement Scalar Operations (RingVec<N> op u128) ---

impl<const N: usize> Add<u128> for RingVec<N> {
    type Output = Self;
    fn add(self, rhs: u128) -> Self::Output {
        self.element_wise_scalar_op(rhs, |a, b| a + b)
    }
}

impl<const N: usize> Sub<u128> for RingVec<N> {
    type Output = Self;
    fn sub(self, rhs: u128) -> Self::Output {
        let modulus = self.modulus;
        self.element_wise_scalar_op(rhs, |a, b| a + modulus - b)
    }
}

impl<const N: usize> Mul<u128> for RingVec<N> {
    type Output = Self;
    fn mul(self, rhs: u128) -> Self::Output {
        self.element_wise_scalar_op(rhs, |a, b| a * b)
    }
}

/// Compress an array of u128 values using adaptive bit width
/// Uses bit-perfect packing: concatenate all bits then pack into bytes
fn compress_ring_vec_data_adaptive(values: &[u128], bits_per_element: usize) -> Vec<u8> {
    if bits_per_element == 0 {
        // Special case: all values are 0 (modulus = 1)
        return Vec::new(); // No bits needed
    }
    
    // Calculate total bits: N elements * B bits each
    let total_bits = values.len() * bits_per_element;
    
    // Pad to make divisible by 8 (byte boundary)
    let padded_bits = (total_bits + 7) & !7; // Round up to nearest multiple of 8
    let total_bytes = padded_bits / 8;
    
    let mut compressed = vec![0u8; total_bytes];
    
    // Pack all values bit by bit into the byte array
    for (element_idx, &value) in values.iter().enumerate() {
        let bit_start = element_idx * bits_per_element;
        
        // Extract each bit from the value and pack it
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

/// Decompress byte data back into an array of u128 values
/// Reverses the bit-perfect packing process
fn decompress_ring_vec_data_adaptive<const N: usize>(compressed: &[u8], bits_per_element: usize) -> Result<[u128; N], String> {
    let mut values = [0u128; N];
    
    if bits_per_element == 0 {
        // Special case: all values are 0 (modulus = 1)
        return Ok(values);
    }
    
    // Extract values bit by bit from the byte array
    for element_idx in 0..N {
        let bit_start = element_idx * bits_per_element;
        let mut value = 0u128;
        
        // Reconstruct each element by extracting its bits
        for bit_pos in 0..bits_per_element {
            let global_bit_pos = bit_start + bit_pos;
            let byte_idx = global_bit_pos / 8;
            let bit_in_byte = global_bit_pos % 8;
            
            if byte_idx >= compressed.len() {
                return Err(format!("Not enough bytes to decompress element {}", element_idx));
            }
            
            let bit_value = (compressed[byte_idx] >> bit_in_byte) & 1;
            if bit_value == 1 {
                value |= 1u128 << bit_pos;
            }
        }
        
        values[element_idx] = value;
    }
    
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_bytes_from_bytes() {
        // Test with 10-bit values (modulus = 1024)
        let modulus = 1024u128; // 2^10
        let values = [123, 456, 789, 1000];
        let ring_vec = RingVec::new(values, modulus);
        
        // Test byte conversion
        let bytes = ring_vec.to_bytes();
        let (reconstructed, bytes_consumed) = RingVec::<4>::from_bytes(&bytes, modulus).unwrap();
        
        // Check that values are preserved
        for i in 0..4 {
            assert_eq!(ring_vec[i], reconstructed[i]);
        }
        
        // Verify modulus is preserved
        assert_eq!(ring_vec.modulus(), reconstructed.modulus());
        
        // Verify bytes consumed matches bytes length
        assert_eq!(bytes_consumed, bytes.len());
        
        // Check compression effectiveness
        let uncompressed_size = RingVec::<4>::uncompressed_size_bytes();
        let compressed_size = bytes.len();
        println!("10-bit compression: {} bytes -> {} bytes ({:.1}x reduction)", 
                 uncompressed_size, compressed_size, 
                 uncompressed_size as f64 / compressed_size as f64);
        
        // Should be significantly compressed (64 bytes -> ~5 bytes for 10-bit values)
        assert!(compressed_size < uncompressed_size / 10);
        
        // Verify expected byte size calculation
        assert_eq!(compressed_size, ring_vec.byte_size());
        assert_eq!(compressed_size, RingVec::<4>::byte_size_for_modulus(modulus));
    }

    #[test]
    fn test_binary_compression() {
        // Test with 1-bit values (modulus = 2)
        let modulus = 2u128;
        let values = [0, 1, 0, 1, 1, 0, 1, 0];
        let ring_vec = RingVec::new(values, modulus);
        
        let bytes = ring_vec.to_bytes();
        let (reconstructed, _) = RingVec::<8>::from_bytes(&bytes, modulus).unwrap();
        
        for i in 0..8 {
            assert_eq!(ring_vec[i], reconstructed[i]);
        }
        
        // Verify extreme compression for binary values
        let compressed_size = bytes.len();
        let uncompressed_size = RingVec::<8>::uncompressed_size_bytes();
        println!("Binary compression: {} bytes -> {} bytes ({:.1}x reduction)", 
                 uncompressed_size, compressed_size,
                 uncompressed_size as f64 / compressed_size as f64);
        
        // Should compress 8*16=128 bytes down to 1 byte for binary values
        assert!(compressed_size <= 1);
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
            let values = [1, 2, 3, 4];
            let ring_vec = RingVec::<4>::new(values, modulus);
            
            assert_eq!(ring_vec.modulus_bit_width(), expected_bits);
            
            let bytes = ring_vec.to_bytes();
            let (reconstructed, bytes_consumed) = RingVec::<4>::from_bytes(&bytes, modulus).unwrap();
            
            for i in 0..4 {
                assert_eq!(ring_vec[i], reconstructed[i]);
            }
            
            // Expected byte size: ceil(4 * expected_bits / 8)
            let expected_byte_size = (4 * expected_bits + 7) / 8;
            assert_eq!(bytes.len(), expected_byte_size);
            assert_eq!(bytes_consumed, expected_byte_size);
            
            println!("Modulus {} ({} bits): {} bytes", modulus, expected_bits, bytes.len());
        }
    }

    #[test]
    fn test_error_cases() {
        // Test invalid modulus
        let bytes = vec![0x12, 0x34];
        
        // Non-power-of-2 modulus should fail in RingVec::new
        let result = std::panic::catch_unwind(|| {
            RingVec::<4>::from_bytes(&bytes, 3) // 3 is not a power of 2
        });
        assert!(result.is_err());
        
        // Test insufficient bytes
        let short_bytes = vec![0x01]; // Too few bytes for 4 elements with 10-bit modulus
        let result = RingVec::<4>::from_bytes(&short_bytes, 1024);
        assert!(result.is_err());
    }
}
