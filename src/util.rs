//! Utility functions for the fuzzy heavy hitters protocol
//! 
//! This module contains helper functions for data conversion and communication.

use scuttlebutt::{Channel, AbstractChannel};
use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;

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
    channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
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
    channel: &mut Channel<BufReader<UnixStream>, BufWriter<UnixStream>>,
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
}
