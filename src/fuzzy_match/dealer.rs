//! FSS Dealer Implementation
//! 
//! This module provides the trusted dealer functionality for generating FSS keys
//! for both the check phase and threshold phase of the fuzzy heavy hitters protocol.
//! 
//! The dealer operates with a coordinator model where only server 0 sends key requests,
//! and the dealer responds by sending matching keys to both servers simultaneously.
//! This ensures both servers always have the same set of FSS keys.

use crate::fss::interval::IntervalFSSKey;
use crate::data_structures::payload::RingVec;
use crate::fuzzy_match::check_phase;
use crate::util::{u128_to_bits, u128_to_bits_msb};
use crate::channel::CommTrackingChannel;
use scuttlebutt::AbstractChannel;
use std::thread;
use std::time::Instant;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::{BufReader, BufWriter};
use std::convert::TryInto;
use rand::Rng;
use rayon::prelude::*;

/// FSS key batch for check phase - contains keys for one server
#[derive(Clone, Debug)]
pub struct FssKeyBatch {
    pub keys: Vec<IntervalFSSKey<1>>,
    pub random_values: Vec<u128>,
}

impl FssKeyBatch {
    pub fn to_bytes(&self, modulus: u128) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.keys.len() as u32).to_le_bytes());
        for k in &self.keys {
            out.extend_from_slice(&k.to_bytes());
        }
        out.extend_from_slice(&(self.random_values.len() as u32).to_le_bytes());
        for v in &self.random_values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(mut bytes: &[u8], modulus: u128) -> Result<(Self, usize), String> {
        if bytes.len() < 4 { return Err("Too short for FssKeyBatch keys len".to_string()); }
        let mut offset = 0;
        let key_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut keys = Vec::with_capacity(key_count);
        for _ in 0..key_count {
            let (k, key_used) = IntervalFSSKey::<1>::from_bytes(&bytes[offset..], modulus);
            keys.push(k);
            offset += key_used;
        }
        if bytes[offset..].len() < 4 { return Err("Too short for FssKeyBatch random_values len".to_string()); }
        let val_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut random_values = Vec::with_capacity(val_count);
        for _ in 0..val_count {
            if bytes[offset..].len() < 16 { return Err("Too short for u128 in FssKeyBatch".to_string()); }
            random_values.push(u128::from_le_bytes(bytes[offset..offset + 16].try_into().unwrap()));
            offset += 16;
        }
        Ok((FssKeyBatch { keys, random_values }, offset))
    }
}

/// FSS key pair batch for check phase - contains keys for both servers
#[derive(Clone, Debug)]
pub struct FssKeyPairBatch {
    pub keys0: Vec<IntervalFSSKey<1>>,
    pub keys1: Vec<IntervalFSSKey<1>>, 
    pub random_pairs: Vec<(u128, u128)>,
}

/// Signal sent from servers to the dealer
#[derive(Clone, Debug)]
pub enum DealerSignal {
    RequestCheckKeys,
    RequestThresholdKeys,
    Shutdown,
}

impl DealerSignal {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            DealerSignal::RequestCheckKeys => vec![0u8],
            DealerSignal::RequestThresholdKeys => vec![1u8],
            DealerSignal::Shutdown => vec![2u8],
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() { return Err("Empty bytes for DealerSignal".to_string()); }
        match bytes[0] {
            0 => Ok(DealerSignal::RequestCheckKeys),
            1 => Ok(DealerSignal::RequestThresholdKeys),
            2 => Ok(DealerSignal::Shutdown),
            _ => Err("Unknown DealerSignal tag".to_string()),
        }
    }
}

/// Networked FSS Dealer that communicates with servers over TCP
pub struct FssDealer {
    distance_threshold: u128,
    count_threshold: u128,
    check_input_bit_length: usize,
    check_output_bit_length: usize,
    n_clients: usize,
}

impl FssDealer {
    /// Create a new networked FSS dealer
    pub fn new(
        distance_threshold: u128,
        count_threshold: u128,  
        check_input_bit_length: usize,
        check_output_bit_length: usize,
        n_clients: usize,
    ) -> Self {
        FssDealer {
            distance_threshold,
            count_threshold,
            check_input_bit_length,
            check_output_bit_length,
            n_clients,
        }
    }

    /// Run the dealer - listen for connections from both servers and handle key requests
    pub fn run(
        &self,  
        channel_server0: &mut CommTrackingChannel,
        channel_server1: &mut CommTrackingChannel,
    ) -> Result<(), String> {
        println!("FSS Dealer starting...");

        // Generate initial FSS keys
        let mut current_check_keys = self.generate_fss_keys_for_check()?;

        // Generate keys for threshold phase
        let mut current_threshold_keys = self.generate_fss_keys_for_threshold()?;


        println!("Generated initial FSS keys, dealer ready");

        // Main dealer loop - handle key requests
        loop {
            // Wait for key request from server 0 (coordinator)
            let signal_bytes = match self.read_dealer_signal(channel_server0) {
                Ok(signal) => signal,
                Err(e) => {
                    println!("Dealer: Error reading from server 0: {}, terminating", e);
                    break;
                }
            };

            match signal_bytes {
                DealerSignal::RequestCheckKeys => {
                    // Send keys to both servers
                    let batch_server0 = FssKeyBatch {
                        keys: current_check_keys.0.clone(),
                        random_values: current_check_keys.2.iter().map(|(r0, _)| *r0).collect(),
                    };

                    let batch_server1 = FssKeyBatch {
                        keys: current_check_keys.1.clone(),
                        random_values: current_check_keys.2.iter().map(|(_, r1)| *r1).collect(),
                    };

                    // Send to server 0
                    self.write_fss_key_batch(channel_server0, &batch_server0)
                        .map_err(|e| format!("Failed to send keys to server 0: {}", e))?;

                    // Send to server 1
                    self.write_fss_key_batch(channel_server1, &batch_server1)
                        .map_err(|e| format!("Failed to send keys to server 1: {}", e))?;

                    // Generate new keys for next request
                    current_check_keys = self.generate_fss_keys_for_check()?;
                }
                DealerSignal::RequestThresholdKeys => {
                    // Create batches
                    let batch_server0 = FssKeyBatch {
                        keys: current_threshold_keys.0.clone(),
                        random_values: current_threshold_keys.2.iter().map(|(r0, _)| *r0).collect(),
                    };

                    let batch_server1 = FssKeyBatch {
                        keys: current_threshold_keys.1.clone(),
                        random_values: current_threshold_keys.2.iter().map(|(_, r1)| *r1).collect(),
                    };

                    // Send to server 0
                    self.write_fss_key_batch(channel_server0, &batch_server0)
                        .map_err(|e| format!("Failed to send threshold keys to server 0: {}", e))?;

                    // Send to server 1
                    self.write_fss_key_batch(channel_server1, &batch_server1)
                        .map_err(|e| format!("Failed to send threshold keys to server 1: {}", e))?;

                    // Generate new keys for next request
                    current_threshold_keys = self.generate_fss_keys_for_threshold()?;
                }
                DealerSignal::Shutdown => {
                    println!("Dealer: Received shutdown signal, terminating");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Read a dealer signal from the channel
    fn read_dealer_signal(&self, channel: &mut CommTrackingChannel) -> Result<DealerSignal, String> {
        // Read the length first
        let mut len_bytes = [0u8; 8];
        channel.read_bytes(&mut len_bytes)
            .map_err(|e| format!("Failed to read length: {}", e))?;
        let len = u64::from_le_bytes(len_bytes) as usize;
        // Read the data
        let mut data = vec![0u8; len];
        channel.read_bytes(&mut data)
            .map_err(|e| format!("Failed to read data: {}", e))?;
        DealerSignal::from_bytes(&data)
    }

    /// Write an FSS key batch to the channel
    fn write_fss_key_batch(&self, channel: &mut CommTrackingChannel, batch: &FssKeyBatch) -> Result<(), String> {
        let modulus = 1u128 << self.check_output_bit_length;
        let data = batch.to_bytes(modulus);
        let len_bytes = (data.len() as u64).to_le_bytes();
        channel.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to write length: {}", e))?;
        channel.write_bytes(&data)
            .map_err(|e| format!("Failed to write data: {}", e))?;
        channel.flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(())
    }

    /// Generate FSS keys for check phase comparison (Lp distance with IntervalFSS)
    /// This simulates a trusted dealer generating FSS keys for distance threshold comparison
    fn generate_fss_keys_for_check(&self) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
        let in_modulus = 1u128 << self.check_input_bit_length;
        let out_modulus = 1u128 << self.check_output_bit_length;
    
        let mut random_pairs = Vec::new();
    
        for _ in 0..self.n_clients {
            // Generate random pair (r0, r1) for this check using standard rand
            let mut std_rng = rand::thread_rng();
            let r0 = std_rng.gen_range(0..in_modulus);
            let r1 = std_rng.gen_range(0..in_modulus);
            random_pairs.push((r0, r1));
        }
    
        // Use parallel processing for FSS key generation
        let key_pairs: Vec<(IntervalFSSKey<1>, IntervalFSSKey<1>)> = random_pairs
            .par_iter()
            .map(|&(r0, r1)| {
                // Check if distance_threshold + r0 + r1 would wrap around
                let sum = self.distance_threshold + (r0 + r1) % in_modulus;
                let wraps_around = sum >= in_modulus;
                let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
                    // Wrap-around case: interval [distance_threshold+r0+r1 mod modulus, r0+r1]
                    // Return 0 in the middle, 1 on left and right
                    let interval_start = (sum + 1) % in_modulus;
                    let interval_end = (r0 + r1 - 1) % in_modulus;

                    let alpha_bits = u128_to_bits_msb(interval_start, self.check_input_bit_length);
                    let beta_bits = u128_to_bits_msb(interval_end, self.check_input_bit_length);
                
                    // For wrap-around: left=1, middle=0, right=1
                    let a = RingVec::<1>::new([1], out_modulus); // left
                    let b = RingVec::<1>::new([0], out_modulus); // middle
                    let c = RingVec::<1>::new([1], out_modulus); // right

                    (alpha_bits, beta_bits, a, b, c)
                } else {
                    // No wrap-around case: interval [r0+r1, distance_threshold+r0+r1]
                    // Return 1 inside interval (distance <= threshold), 0 outside
                    let interval_start = (r0 + r1) % in_modulus;
                    let interval_end = sum;

                    let alpha_bits = u128_to_bits_msb(interval_start, self.check_input_bit_length);
                    let beta_bits = u128_to_bits_msb(interval_end, self.check_input_bit_length);

                    // For no wrap-around: left=0, middle=1, right=0
                    let a = RingVec::<1>::new([0], out_modulus); // left
                    let b = RingVec::<1>::new([1], out_modulus); // middle
                    let c = RingVec::<1>::new([0], out_modulus); // right

                    (alpha_bits, beta_bits, a, b, c)
                };
            
                let (key0, key1) = IntervalFSSKey::gen_IntervalFSSKey(
                    &alpha_bits,
                    &beta_bits,
                    &a,
                    &b,
                    &c,
                    out_modulus,
                );
            
                (key0, key1)
            })
            .collect();
    
        // Split the parallel results into separate vectors
        let mut keys_server0 = Vec::with_capacity(self.n_clients);
        let mut keys_server1 = Vec::with_capacity(self.n_clients);
        
        for (key0, key1) in key_pairs {
            keys_server0.push(key0);
            keys_server1.push(key1);
        }
    
        Ok((keys_server0, keys_server1, random_pairs))
    }

    /// Generate FSS keys for interval FSS threshold comparison
    /// This simulates a trusted dealer generating FSS keys
    fn generate_fss_keys_for_threshold(&self) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
        let modulus = 1u128 << self.check_output_bit_length;
        let mut keys_server0 = Vec::new();
        let mut keys_server1 = Vec::new();
        let mut random_pairs = Vec::new();
    
        for _ in 0..1 {
            // Generate random pair (r0, r1) for this query using standard rand
            let mut std_rng = rand::thread_rng();
            let r0 = std_rng.gen_range(0..modulus);
            let r1 = std_rng.gen_range(0..modulus);
            random_pairs.push((r0, r1));

            // Check if count_threshold + r0 + r1 would wrap around
            let sum = self.count_threshold + (r0 + r1) % modulus;
            let (alpha_bits, beta_bits, a, b, c) = if sum >= modulus {
                // Wrap-around case: interval [threshold+r0+r1 mod modulus, r0+r1]
                // Return 1 in the middle, 0 on left and right
                let interval_start = sum % modulus;
                let interval_end = (r0 + r1) % modulus;
            
                let alpha_bits = u128_to_bits_msb(interval_start, self.check_output_bit_length);
                let beta_bits = u128_to_bits_msb(interval_end, self.check_output_bit_length);
            
                // For wrap-around: left=0, middle=1, right=0
                let a = RingVec::<1>::new([0], 2); // left
                let b = RingVec::<1>::new([1], 2); // middle
                let c = RingVec::<1>::new([0], 2); // right
            
                (alpha_bits, beta_bits, a, b, c)
            } else {
                // No wrap-around case: interval [r0+r1, threshold+r0+r1]
                // Return 1 on left and right, 0 in the middle
                let interval_start = (r0 + r1) % modulus;
                let interval_end = (sum + modulus - 1) % modulus;
            
                let alpha_bits = u128_to_bits_msb(interval_start, self.check_output_bit_length);
                let beta_bits = u128_to_bits_msb(interval_end, self.check_output_bit_length);
            
                // For no wrap-around: left=1, middle=0, right=1
                let a = RingVec::<1>::new([1], 2); // left
                let b = RingVec::<1>::new([0], 2); // middle
                let c = RingVec::<1>::new([1], 2); // right
            
                (alpha_bits, beta_bits, a, b, c)
            };
        
            let (key0, key1) = IntervalFSSKey::gen_IntervalFSSKey(
                &alpha_bits,
                &beta_bits,
                &a,
                &b,
                &c,
                2,
            );
        
            keys_server0.push(key0);
            keys_server1.push(key1);
        }
    
        Ok((keys_server0, keys_server1, random_pairs))
    }

}
