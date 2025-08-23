//! Utility functions for the fuzzy heavy hitters protocol
//! 
//! This module contains helper functions for data conversion and communication.

use scuttlebutt::AbstractChannel;

/// Pack a vector of booleans into a vector of bytes
/// Each byte contains up to 8 bits, with remaining bits set to 0 if the length is not a multiple of 8
pub fn pack_bits_to_bytes(bits: &[bool]) -> Vec<u8> {
    let mut bytes = Vec::new();
    
    for chunk in bits.chunks(8) {
        let mut byte = 0u8;
        for (i, &bit) in chunk.iter().enumerate() {
            if bit {
                byte |= 1 << i;
            }
        }
        bytes.push(byte);
    }
    
    bytes
}

/// Unpack a vector of bytes into a vector of booleans
/// The `expected_length` parameter specifies how many bits to extract
pub fn unpack_bytes_to_bits(bytes: &[u8], expected_length: usize) -> Vec<bool> {
    let mut bits = Vec::new();
    
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for bit_idx in 0..8 {
            if bits.len() >= expected_length {
                break;
            }
            
            let bit = (byte >> bit_idx) & 1 == 1;
            bits.push(bit);
        }
        
        if bits.len() >= expected_length {
            break;
        }
    }
    
    // Trim to exact length
    bits.truncate(expected_length);
    bits
}

/// Send a vector of booleans over a channel by packing them into bytes
pub fn send_bool_vec(
    channel: &mut crate::channel::CommTrackingChannel,
    bits: &[bool],
) -> Result<(), String> {
    // First send the length
    let length = bits.len() as u32;
    let length_bytes = length.to_le_bytes();
    channel.write_bytes(&length_bytes)
        .map_err(|e| format!("Failed to send length: {:?}", e))?;
    
    // Then pack and send the bits
    let packed_bytes = pack_bits_to_bytes(bits);
    let packed_length = packed_bytes.len() as u32;
    let packed_length_bytes = packed_length.to_le_bytes();
    channel.write_bytes(&packed_length_bytes)
        .map_err(|e| format!("Failed to send packed length: {:?}", e))?;
    
    channel.write_bytes(&packed_bytes)
        .map_err(|e| format!("Failed to send packed bits: {:?}", e))?;
    
    channel.flush()
        .map_err(|e| format!("Failed to flush channel: {:?}", e))?;
    
    Ok(())
}

/// Receive a vector of booleans from a channel by unpacking them from bytes
pub fn receive_bool_vec(
    channel: &mut crate::channel::CommTrackingChannel,
) -> Result<Vec<bool>, String> {
    // First receive the length
    let mut length_bytes = [0u8; 4];
    channel.read_bytes(&mut length_bytes)
        .map_err(|e| format!("Failed to receive length: {:?}", e))?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    
    // Then receive the packed length
    let mut packed_length_bytes = [0u8; 4];
    channel.read_bytes(&mut packed_length_bytes)
        .map_err(|e| format!("Failed to receive packed length: {:?}", e))?;
    let packed_length = u32::from_le_bytes(packed_length_bytes) as usize;
    
    // Receive the packed bytes
    let mut packed_bytes = vec![0u8; packed_length];
    channel.read_bytes(&mut packed_bytes)
        .map_err(|e| format!("Failed to receive packed bits: {:?}", e))?;
    
    // Unpack to get the original bits
    let bits = unpack_bytes_to_bits(&packed_bytes, length);
    
    Ok(bits)
}

/// Convert a u128 value to a vector of bits with specified bit length
/// Bits are returned in LSB-first order (bit 0 is the least significant bit)
pub fn u128_to_bits(value: u128, bit_length: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bit_length);
    for i in 0..bit_length {
        bits.push((value >> i) & 1 == 1);
    }
    bits
}

/// Convert a vector of bits to a u128 value
/// Bits are expected to be in LSB-first order (bit 0 is the least significant bit)
pub fn bits_to_u128(bits: &[bool]) -> u128 {
    let mut value = 0u128;
    for (i, &bit) in bits.iter().enumerate() {
        if bit {
            value |= 1u128 << i;
        }
    }
    value
}

/// Convert a u128 value to a vector of bits with specified bit length
/// Bits are returned in MSB-first order (bit 0 is the most significant bit)
pub fn u128_to_bits_msb(value: u128, bit_length: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(bit_length);
    for i in (0..bit_length).rev() {
        bits.push((value >> i) & 1 == 1);
    }
    bits
}

/// Convert a vector of bits to a u128 value
/// Bits are expected to be in MSB-first order (bit 0 is the most significant bit)
pub fn bits_to_u128_msb(bits: &[bool]) -> u128 {
    let mut value = 0u128;
    for &bit in bits {
        value = (value << 1) | (if bit { 1 } else { 0 });
    }
    value
}

/// Convert a query point (vector of bit vectors) to a vector of u128 values for debugging
/// Each inner Vec<bool> represents the bits for one dimension
pub fn bool_vec_to_u128s(query_point: &[Vec<bool>]) -> Vec<u128> {
    query_point.iter().map(|dim_bits| bits_to_u128(dim_bits)).collect()
}

pub fn bits_to_u8s(bits: &[bool]) -> Vec<u8> {
    bits.chunks(8).map(|chunk| {
        chunk.iter().enumerate().fold(0u8, |acc, (i, &bit)| {
            acc | ((bit as u8) << i)
        })
    }).collect()
}

pub fn u8s_to_bits(bytes: &[u8], len: usize) -> Vec<bool> {
    let mut bits = Vec::new();
    for &byte in bytes {
        for i in 0..8 {
            bits.push((byte >> i) & 1 == 1);
        }
    }
    bits.truncate(len);
    bits
}

/// Calculate distance between two points based on the distance metric
pub fn calculate_distance(point1: &[u128], point2: &[u128], distance_metric: &str) -> u128 {
    match distance_metric {
        "Linf" => {
            // L-infinity distance (max coordinate difference)
            let mut l_inf_distance = 0;
            for dim in 0..point1.len() {
                let diff = if point1[dim] > point2[dim] {
                    point1[dim] - point2[dim]
                } else {
                    point2[dim] - point1[dim]
                };
                l_inf_distance = l_inf_distance.max(diff);
            }
            l_inf_distance
        },
        "L1" => {
            // L1 distance (Manhattan distance)
            let mut l1_distance = 0;
            for dim in 0..point1.len() {
                let diff = if point1[dim] > point2[dim] {
                    point1[dim] - point2[dim]
                } else {
                    point2[dim] - point1[dim]
                };
                l1_distance += diff;
            }
            l1_distance
        },
        "L2" => {
            // L2 distance squared (to avoid sqrt)
            let mut sum_of_squares = 0u128;
            for dim in 0..point1.len() {
                let diff = if point1[dim] > point2[dim] {
                    point1[dim] - point2[dim]
                } else {
                    point2[dim] - point1[dim]
                };
                sum_of_squares += diff * diff;
            }
            sum_of_squares
        },
        "L3" => {
            // L3 distance (sum of cubes)^(1/3), but we return cubes for efficiency
            let mut sum_of_cubes = 0u128;
            for dim in 0..point1.len() {
                let diff = if point1[dim] > point2[dim] {
                    point1[dim] - point2[dim]
                } else {
                    point2[dim] - point1[dim]
                };
                sum_of_cubes += diff * diff * diff;
            }
            sum_of_cubes
        },
        _ => {
            // Default to L-infinity for unknown methods
            let mut l_inf_distance = 0;
            for dim in 0..point1.len() {
                let diff = if point1[dim] > point2[dim] {
                    point1[dim] - point2[dim]
                } else {
                    point2[dim] - point1[dim]
                };
                l_inf_distance = l_inf_distance.max(diff);
            }
            l_inf_distance
        }
    }
}

pub fn calculate_optimistic_distance(point_max: &[u128], point_min: &[u128], point2: &[u128], distance_metric: &str) -> u128 {
    let point_best = point_max.iter().zip(point_min.iter()).zip(point2.iter()).map(|((max, min), p2)| {
        if max < p2 {
            *max
        } else if min <= p2 {
            *p2
        } else {
            *min
        }
    }).collect::<Vec<u128>>();
    calculate_distance(&point_best, point2, distance_metric)
}

/// Get the distance threshold for comparison based on the distance metric
pub fn get_distance_threshold(delta: u128, distance_metric: &str) -> u128 {
    match distance_metric {
        "Linf" | "L1" => delta,
        "L2" => {
            // For L2, we use squared distance, so threshold is delta^2
            delta * delta
        },
        "L3" => {
            // For L3, we use cubed distance, so threshold is delta^3
            delta * delta * delta
        },
        _ => delta, // Default to delta for unknown methods
    }
}

#[inline]
pub fn xor_u8_16(a: &[u8; 16], b: &[u8; 16]) -> [u8; 16] {
    [
        a[0] ^ b[0],  a[1] ^ b[1],  a[2] ^ b[2],  a[3] ^ b[3],
        a[4] ^ b[4],  a[5] ^ b[5],  a[6] ^ b[6],  a[7] ^ b[7],
        a[8] ^ b[8],  a[9] ^ b[9],  a[10] ^ b[10],  a[11] ^ b[11],
        a[12] ^ b[12],  a[13] ^ b[13],  a[14] ^ b[14],  a[15] ^ b[15],
    ]
}

pub fn xor<const N: usize>(a: &[u8; N], b: &[u8; N]) -> [u8; N] {
    let mut result = [0u8; N];
    for i in 0..N {
        result[i] = a[i] ^ b[i];
    }
    result
}

pub fn and_bit<const N: usize>(a: [u8; N], b: bool) -> [u8; N] {
    if b {
        a
    } else {
        [0u8; N]
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_full_bytes() {
        let bits = vec![true, false, true, true, false, false, true, false];
        let packed = pack_bits_to_bytes(&bits);
        let unpacked = unpack_bytes_to_bits(&packed, bits.len());
        assert_eq!(bits, unpacked);
    }

    #[test]
    fn test_pack_unpack_partial_bytes() {
        let bits = vec![true, false, true, false, true];
        let packed = pack_bits_to_bytes(&bits);
        let unpacked = unpack_bytes_to_bits(&packed, bits.len());
        assert_eq!(bits, unpacked);
    }

    #[test]
    fn test_pack_unpack_empty() {
        let bits = vec![];
        let packed = pack_bits_to_bytes(&bits);
        let unpacked = unpack_bytes_to_bits(&packed, bits.len());
        assert_eq!(bits, unpacked);
    }

    #[test]
    fn test_pack_unpack_single_bit() {
        let bits = vec![true];
        let packed = pack_bits_to_bytes(&bits);
        let unpacked = unpack_bytes_to_bits(&packed, bits.len());
        assert_eq!(bits, unpacked);
    }

    #[test]
    fn test_u128_to_bits() {
        // Test basic conversion
        let bits = u128_to_bits(5, 4); // 5 = 0101 in binary (LSB first)
        assert_eq!(bits, vec![true, false, true, false]); // LSB first: [1, 0, 1, 0]
        
        // Test with zero
        let bits = u128_to_bits(0, 4);
        assert_eq!(bits, vec![false, false, false, false]);
        
        // Test with all ones
        let bits = u128_to_bits(15, 4); // 15 = 1111 in binary
        assert_eq!(bits, vec![true, true, true, true]);
        
        // Test larger number
        let bits = u128_to_bits(170, 8); // 170 = 10101010 in binary
        assert_eq!(bits, vec![false, true, false, true, false, true, false, true]);
    }

    #[test]
    fn test_bits_to_u128() {
        // Test basic conversion (reverse of u128_to_bits)
        let bits = vec![true, false, true, false]; // LSB first for 5
        assert_eq!(bits_to_u128(&bits), 5);
        
        // Test with zero
        let bits = vec![false, false, false, false];
        assert_eq!(bits_to_u128(&bits), 0);
        
        // Test with all ones
        let bits = vec![true, true, true, true];
        assert_eq!(bits_to_u128(&bits), 15);
        
        // Test larger number
        let bits = vec![false, true, false, true, false, true, false, true];
        assert_eq!(bits_to_u128(&bits), 170);
        
        // Test round-trip conversion
        let original = 12345u128;
        let bits = u128_to_bits(original, 16);
        let converted_back = bits_to_u128(&bits);
        assert_eq!(original, converted_back);
    }

    #[test]
    fn test_u128_to_bits_msb() {
        // Test basic conversion
        let bits = u128_to_bits_msb(5, 4); // 5 = 0101 in binary (MSB first)
        assert_eq!(bits, vec![false, true, false, true]); // MSB first: [0, 1, 0, 1]
        
        // Test with zero
        let bits = u128_to_bits_msb(0, 4);
        assert_eq!(bits, vec![false, false, false, false]);
        
        // Test with all ones
        let bits = u128_to_bits_msb(15, 4); // 15 = 1111 in binary
        assert_eq!(bits, vec![true, true, true, true]);
        
        // Test larger number
        let bits = u128_to_bits_msb(170, 8); // 170 = 10101010 in binary
        assert_eq!(bits, vec![true, false, true, false, true, false, true, false]);
    }

    #[test]
    fn test_bits_to_u128_msb() {
        // Test basic conversion (reverse of u128_to_bits_msb)
        let bits = vec![false, true, false, true]; // MSB first for 5
        assert_eq!(bits_to_u128_msb(&bits), 5);
        
        // Test with zero
        let bits = vec![false, false, false, false];
        assert_eq!(bits_to_u128_msb(&bits), 0);
        
        // Test with all ones
        let bits = vec![true, true, true, true];
        assert_eq!(bits_to_u128_msb(&bits), 15);
        
        // Test larger number
        let bits = vec![true, false, true, false, true, false, true, false];
        assert_eq!(bits_to_u128_msb(&bits), 170);
        
        // Test round-trip conversion
        let original = 12345u128;
        let bits = u128_to_bits_msb(original, 16);
        let converted_back = bits_to_u128_msb(&bits);
        assert_eq!(original, converted_back);
    }

    #[test]
    fn test_query_point_to_u128s() {
        let query_point = vec![
            vec![true, false, true, false], // 5
            vec![false, true, true, false], // 6
            vec![true, true, true, true],   // 15
        ];
        let result = bool_vec_to_u128s(&query_point);
        assert_eq!(result, vec![5, 6, 15]);
    }

    #[test]
    fn test_u8_to_bits() {
        let original_bits = [true, false, true];
        let u8_value = bits_to_u8s(&original_bits);
        let recovered_bits = u8s_to_bits(&u8_value, original_bits.len());
        assert_eq!(recovered_bits, original_bits);

        let original_bits = [false, true, false, true];
        let u8_value = bits_to_u8s(&original_bits);
        let recovered_bits = u8s_to_bits(&u8_value, original_bits.len());
        assert_eq!(recovered_bits, original_bits);

        let original_bits = [true, true, true, true];
        let u8_value = bits_to_u8s(&original_bits);
        let recovered_bits = u8s_to_bits(&u8_value, original_bits.len());
        assert_eq!(recovered_bits, original_bits);

        let original_bits = [false, true, false, true];
        let u8_value = bits_to_u8s(&original_bits);
        let recovered_bits = u8s_to_bits(&u8_value, original_bits.len());
        assert_eq!(recovered_bits, original_bits);

        let original_bits = [false, false, false, false, false];
        let u8_value = bits_to_u8s(&original_bits);
        let recovered_bits = u8s_to_bits(&u8_value, original_bits.len());
        assert_eq!(recovered_bits, original_bits);
    }
}
