//! Key-Value Pair Preparation Strategies
//! 
//! This module contains different strategies for preparing key-value pairs
//! for OKVS encoding based on distance metrics and dictionary types.

use std::collections::HashSet;
use rand::Rng;
use blake3;
use crate::util::{u128_to_bits, u128_to_bits_msb};

/// Trait for different key-value pair preparation strategies
pub trait KeyValuePairStrategy {
    fn prepare_key_value_pairs(
        &self,
        dim: usize,
        left: u128,
        right: u128,
        x_i: u128,
        input_bit_length: usize,
        output_bit_length: usize,
    ) -> (Vec<Vec<bool>>, Vec<u128>, Vec<u128>);
}

/// Strategy for known dictionary with L-infinity distance
pub struct KnownLInfinityStrategy;

impl KeyValuePairStrategy for KnownLInfinityStrategy {
    fn prepare_key_value_pairs(
        &self,
        _dim: usize,
        left: u128,
        right: u128,
        _x_i: u128,
        input_bit_length: usize,
        output_bit_length: usize,
    ) -> (Vec<Vec<bool>>, Vec<u128>, Vec<u128>) {
        let mut keys = Vec::new();
        let mut values_0 = Vec::new();
        let mut values_1 = Vec::new();
        let modulus_mask = (1u128 << output_bit_length) - 1;

        for key in left..=right {
            let key_bits = u128_to_bits_msb(key, input_bit_length);
            
            let value = rand::rng().random::<u128>() & modulus_mask;
            // Generate u128 from Blake3 hash of key_bits
            let key_bits_bytes: Vec<u8> = key_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
            let hash = blake3::hash(&key_bits_bytes);
            let hash_bytes = hash.as_bytes();
            let value_mask = u128::from_le_bytes([
                hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
            ]) & modulus_mask;
            
            // For now, use simple secret sharing where both shares are identical
            let share0 = value;
            let share1 = (value + value_mask) & modulus_mask; // share0 XOR share1 = value
            
            keys.push(key_bits);
            values_0.push(share0);
            values_1.push(share1);
        }

        (keys, values_0, values_1)
    }
}

/// Strategy for unknown dictionary with L-infinity distance
pub struct UnknownLInfinityStrategy;

impl KeyValuePairStrategy for UnknownLInfinityStrategy {
    fn prepare_key_value_pairs(
        &self,
        _dim: usize,
        left: u128,
        right: u128,
        _x_i: u128,
        input_bit_length: usize,
        output_bit_length: usize,
    ) -> (Vec<Vec<bool>>, Vec<u128>, Vec<u128>) {
        let mut keys = Vec::new();
        let mut values_0 = Vec::new();
        let mut values_1 = Vec::new();
        let modulus_mask = (1u128 << output_bit_length) - 1;

        // Use HashSet to efficiently collect distinct prefixes
        let mut distinct_prefixes = HashSet::new();

        // Generate all distinct prefixes for values in the range [left, right]
        for value in left..=right {
            for prefix_len in 1..=input_bit_length {
                let prefix = value >> (input_bit_length - prefix_len);
                // Create a compact representation for the prefix with its length
                distinct_prefixes.insert((prefix, prefix_len));
            }
        }

        // Convert distinct prefixes to keys and generate corresponding values
        for (prefix, prefix_len) in distinct_prefixes {
            let prefix_bits = u128_to_bits_msb(prefix, prefix_len);

            println!("Prefix bits: {:?}", prefix_bits);
            
            let value = rand::rng().random::<u128>() & modulus_mask;
            // Generate u128 from Blake3 hash of prefix_bits
            let prefix_bits_bytes: Vec<u8> = prefix_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
            let hash = blake3::hash(&prefix_bits_bytes);
            let hash_bytes = hash.as_bytes();
            let value_mask = u128::from_le_bytes([
                hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
            ]) & modulus_mask;
            
            // For now, use simple secret sharing where both shares are identical
            let share0 = value;
            let share1 = (value + value_mask) & modulus_mask; // share0 XOR share1 = value
            
            keys.push(prefix_bits);
            values_0.push(share0);
            values_1.push(share1);
        }

        (keys, values_0, values_1)
    }
}

/// Strategy for known dictionary with Lp distance
pub struct KnownLpStrategy {
    pub p: u32,
}

impl KeyValuePairStrategy for KnownLpStrategy {
    fn prepare_key_value_pairs(
        &self,
        _dim: usize,
        left: u128,
        right: u128,
        x_i: u128,
        input_bit_length: usize,
        output_bit_length: usize,
    ) -> (Vec<Vec<bool>>, Vec<u128>, Vec<u128>) {
        let mut keys = Vec::new();
        let mut values_0 = Vec::new();
        let mut values_1 = Vec::new();
        let modulus_mask = (1u128 << output_bit_length) - 1;

        for key in left..=right {
            let key_bits = u128_to_bits_msb(key, input_bit_length);
            // Calculate |key - x_i|^p
            let distance = if key >= x_i { key - x_i } else { x_i - key };
            let distance_p = compute_power_static(distance, self.p, output_bit_length) & modulus_mask;
            
            // Generate random value for secret sharing
            let random_value = rand::rng().random::<u128>() & modulus_mask;
            
            // Generate deterministic mask from key bits using Blake3
            let key_bits_bytes: Vec<u8> = key_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
            let hash = blake3::hash(&key_bits_bytes);
            let hash_bytes = hash.as_bytes();
            let value_mask = u128::from_le_bytes([
                hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
            ]) & modulus_mask;
            
            // Secret sharing: share0 gets random_value, share1 gets |key - x_i|^p - random_value + mask
            let share0 = random_value;
            let share1 = (distance_p + modulus_mask + 1 - random_value + value_mask) & modulus_mask;
            
            keys.push(key_bits);
            values_0.push(share0);
            values_1.push(share1);
        }

        (keys, values_0, values_1)
    }
}

/// Strategy for unknown dictionary with Lp distance
pub struct UnknownLpStrategy {
    pub p: u32,
}

impl KeyValuePairStrategy for UnknownLpStrategy {
    fn prepare_key_value_pairs(
        &self,
        _dim: usize,
        left: u128,
        right: u128,
        x_i: u128,
        input_bit_length: usize,
        output_bit_length: usize,
    ) -> (Vec<Vec<bool>>, Vec<u128>, Vec<u128>) {
        let mut keys = Vec::new();
        let mut values_0 = Vec::new();
        let mut values_1 = Vec::new();
        let modulus_mask = (1u128 << output_bit_length) - 1;

        // Use HashSet to efficiently collect distinct prefixes
        let mut distinct_prefixes = HashSet::new();

        // Generate all distinct prefixes for values in the range [left, right]
        for value in left..=right {
            for prefix_len in 1..=input_bit_length {
                let prefix = value >> (input_bit_length - prefix_len);
                // Create a compact representation for the prefix with its length
                distinct_prefixes.insert((prefix, prefix_len));
            }
        }

        // Convert distinct prefixes to keys and generate corresponding values
        for (prefix, prefix_len) in distinct_prefixes {
            let prefix_bits = u128_to_bits_msb(prefix, prefix_len);

            println!("Prefix bits: {:?}, prefix_len: {}", prefix_bits, prefix_len);
            
            // Calculate the distance based on the prefix rules:
            // 1. If prefix is a prefix of x_i, distance = 0
            // 2. If prefix < x_i (lexicographically), closest string is prefix followed by all 1s
            // 3. If prefix > x_i (lexicographically), closest string is prefix followed by all 0s
            
            let distance_p = if is_prefix_of_static(prefix, prefix_len, x_i, input_bit_length) {
                // Case 1: prefix is a prefix of x_i, distance = 0
                0u128
            } else {
                // Get the prefix of x_i with the same length
                let x_i_prefix = x_i >> (input_bit_length - prefix_len);
                
                if prefix < x_i_prefix {
                    // Case 2: prefix < x_i prefix, closest string is prefix + all 1s
                    let closest_string = prefix << (input_bit_length - prefix_len) |
                                       ((1u128 << (input_bit_length - prefix_len)) - 1);
                    let distance = x_i - closest_string; // x_i > closest_string always in this case
                    compute_power_static(distance, self.p, output_bit_length) & modulus_mask
                } else {
                    // Case 3: prefix > x_i prefix, closest string is prefix + all 0s
                    let closest_string = prefix << (input_bit_length - prefix_len);
                    let distance = closest_string - x_i; // closest_string > x_i always in this case
                    compute_power_static(distance, self.p, output_bit_length) & modulus_mask
                }
            };
            
            // Generate random value for secret sharing
            let random_value = rand::rng().random::<u128>() & modulus_mask;
            
            // Generate deterministic mask from prefix bits using Blake3
            let prefix_bits_bytes: Vec<u8> = prefix_bits.iter().map(|&b| if b { 1u8 } else { 0u8 }).collect();
            let hash = blake3::hash(&prefix_bits_bytes);
            let hash_bytes = hash.as_bytes();
            let value_mask = u128::from_le_bytes([
                hash_bytes[0], hash_bytes[1], hash_bytes[2], hash_bytes[3],
                hash_bytes[4], hash_bytes[5], hash_bytes[6], hash_bytes[7],
                hash_bytes[8], hash_bytes[9], hash_bytes[10], hash_bytes[11],
                hash_bytes[12], hash_bytes[13], hash_bytes[14], hash_bytes[15],
            ]) & modulus_mask;
            
            // Secret sharing: share0 gets random_value, share1 gets distance_p - random_value + mask
            let share0 = random_value;
            let share1 = (distance_p + modulus_mask + 1 - random_value + value_mask) & modulus_mask;
            
            keys.push(prefix_bits);
            values_0.push(share0);
            values_1.push(share1);
        }

        (keys, values_0, values_1)
    }
}

/// Compute base^exponent modulo 2^output_bit_length
/// Uses fast exponentiation for efficiency
fn compute_power_static(base: u128, exponent: u32, output_bit_length: usize) -> u128 {
    if exponent == 0 {
        return 1;
    }
    if exponent == 1 {
        return base;
    }
    
    let modulus_mask = (1u128 << output_bit_length) - 1;
    let mut result = 1u128;
    let mut base = base & modulus_mask;
    let mut exp = exponent;
    
    while exp > 0 {
        if exp % 2 == 1 {
            result = (result * base) & modulus_mask;
        }
        base = (base * base) & modulus_mask;
        exp /= 2;
    }
    
    result
}

/// Check if a given prefix is a prefix of the target value
/// Returns true if the first prefix_len bits of target match the prefix
fn is_prefix_of_static(prefix: u128, prefix_len: usize, target: u128, input_bit_length: usize) -> bool {
    if prefix_len == 0 {
        return true; // Empty prefix is always a prefix
    }
    if prefix_len > input_bit_length {
        return false; // Prefix longer than input length
    }
    
    // Extract the first prefix_len bits of target
    let target_prefix = target >> (input_bit_length - prefix_len);
    prefix == target_prefix
}
