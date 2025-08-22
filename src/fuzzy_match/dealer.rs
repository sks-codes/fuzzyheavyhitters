//! FSS Dealer Implementation
//! 
//! This module provides the trusted dealer functionality for generating FSS keys
//! for both the check phase and threshold phase of the fuzzy heavy hitters protocol.
//! 
//! The dealer operates with a coordinator model where only server 0 sends key requests,
//! and the dealer responds by sending matching keys to both servers simultaneously.
//! This ensures both servers always have the same set of FSS keys.

use crate::fss::{
    ldcf::LdcfKey,
    rdcf::RdcfKey,
    dpf::DpfKey,
};
use crate::data_structures::payload::RingVec;
use crate::util::{u128_to_bits_msb, bits_to_u8s, u8s_to_bits};
use crate::channel::CommTrackingChannel;
use scuttlebutt::AbstractChannel;
use std::convert::TryInto;
use rand::Rng;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;

/// FSS key batch for check phase - contains keys for one server
#[derive(Clone, Debug)]
pub struct FssKeyBatch {
    pub keys: Vec<(LdcfKey<1>, RdcfKey<1>)>,
    pub random_values: Vec<u128>,
}

impl FssKeyBatch {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.keys.len() as u32).to_le_bytes());
        for (k0, k1) in &self.keys {
            out.extend_from_slice(&k0.to_bytes());
            out.extend_from_slice(&k1.to_bytes());
        }
        out.extend_from_slice(&(self.random_values.len() as u32).to_le_bytes());
        for v in &self.random_values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize), String> {
        if bytes.len() < 4 { return Err("Too short for FssKeyBatch keys len".to_string()); }
        let mut offset = 0;
        let key_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut keys = Vec::with_capacity(key_count);
        for _ in 0..key_count {
            let (k0, key_used) = LdcfKey::<1>::from_bytes(&bytes[offset..], modulus);
            offset += key_used;
            let (k1, key_used) = RdcfKey::<1>::from_bytes(&bytes[offset..], modulus);
            offset += key_used;
            keys.push((k0, k1));
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

pub struct DpfKeyBatch {
    pub keys: Vec<DpfKey<1>>,
    pub random_values: Vec<Vec<bool>>,
}

impl DpfKeyBatch {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.keys.len() as u32).to_le_bytes());
        for k in &self.keys {
            out.extend_from_slice(&k.to_bytes());
        }
        out.extend_from_slice(&(self.random_values.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.random_values[0].len() as u32).to_le_bytes());
        for v in &self.random_values {
            out.extend_from_slice(&bits_to_u8s(v));
        }
        out
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize), String> {
        if bytes.len() < 4 { return Err("Too short for DpfKeyBatch keys len".to_string()); }
        let mut offset = 0;
        let key_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut keys = Vec::with_capacity(key_count);
        for _ in 0..key_count {
            let (k, key_used) = DpfKey::<1>::from_bytes(&bytes[offset..], modulus);
            offset += key_used;
            keys.push(k);
        }
        if bytes[offset..].len() < 4 { return Err("Too short for DpfKeyBatch random_values len".to_string()); }
        let val_count = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let val_length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;    
        offset += 4;
        let val_bytes_length = (val_length + 7) / 8;
        let mut random_values = Vec::with_capacity(val_count);
        for _ in 0..val_count {
            random_values.push(u8s_to_bits(bytes[offset..offset + val_bytes_length].try_into().unwrap(), val_length));
            offset += val_bytes_length;
        }
        Ok((DpfKeyBatch { keys, random_values }, offset))
    }
}

/// Signal sent from servers to the dealer
#[derive(Clone, Debug, PartialEq)]
pub enum DealerSignal {
    RequestEqualityKeys,
    RequestCheckKeys,
    RequestThresholdKeys,
    Shutdown,
}

impl DealerSignal {
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            DealerSignal::RequestEqualityKeys => vec![0u8],
            DealerSignal::RequestCheckKeys => vec![1u8],
            DealerSignal::RequestThresholdKeys => vec![2u8],
            DealerSignal::Shutdown => vec![3u8],
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() { return Err("Empty bytes for DealerSignal".to_string()); }
        match bytes[0] {
            0 => Ok(DealerSignal::RequestEqualityKeys),
            1 => Ok(DealerSignal::RequestCheckKeys),
            2 => Ok(DealerSignal::RequestThresholdKeys),
            3 => Ok(DealerSignal::Shutdown),
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
    dimensions: usize,
}

impl FssDealer {
    /// Create a new networked FSS dealer
    pub fn new(
        distance_threshold: u128,
        count_threshold: u128,  
        check_input_bit_length: usize,
        check_output_bit_length: usize,
        n_clients: usize,
        dimensions: usize,
    ) -> Self {
        FssDealer {
            distance_threshold,
            count_threshold,
            check_input_bit_length,
            check_output_bit_length,
            n_clients,
            dimensions,
        }
    }

    /// Run the dealer with multiple channels - handle key requests from multiple parallel channels
    pub fn run_parallel(
        &self,  
        channels_server0: &[Arc<Mutex<CommTrackingChannel>>],
        channels_server1: &[Arc<Mutex<CommTrackingChannel>>],
    ) -> Result<(), String> {
        println!("FSS Dealer starting with {} channels per server...", channels_server0.len());

        if channels_server0.len() != channels_server1.len() {
            return Err(format!("Mismatch in channel count: server0 has {}, server1 has {}", 
                             channels_server0.len(), channels_server1.len()));
        }

        println!("Generated initial FSS keys, dealer ready");

        // Process each channel pair in parallel - each gets its own persistent thread
        channels_server0
            .par_iter()
            .zip(channels_server1.par_iter())
            .enumerate()
            .try_for_each(|(channel_idx, (server0_channel, server1_channel))| {
                let mut equality_keys = self.generate_fss_keys_for_equality().expect("Failed to generate equality keys");
                let mut check_keys = self.generate_fss_keys_for_check().expect("Failed to generate check keys");
                let mut threshold_keys = self.generate_fss_keys_for_threshold().expect("Failed to generate threshold keys");

                // Each thread has its own local shutdown flag
                let mut local_shutdown = false;
                
                // Lock the channels once at the beginning since this thread owns them
                let mut server0_channel_guard = server0_channel.lock()
                    .map_err(|e| format!("Failed to lock server0 channel {}: {}", channel_idx, e))?;
                let mut server1_channel_guard = server1_channel.lock()
                    .map_err(|e| format!("Failed to lock server1 channel {}: {}", channel_idx, e))?;
                
                // Each channel pair runs in its own persistent loop
                loop {
                    // Check if local shutdown was triggered
                    if local_shutdown {
                        println!("Channel {} terminating due to shutdown", channel_idx);
                        break;
                    }

                    let server0_signal = self.read_dealer_signal(&mut *server0_channel_guard);
                    let server1_signal = self.read_dealer_signal(&mut *server1_channel_guard);
                    assert_eq!(server0_signal.clone().unwrap(), server1_signal.unwrap(),
                        "Signals from both servers should match");

                    // Check if there's a signal waiting on the server0 channel
                    match server0_signal {
                        Ok(signal) => {
                            match signal {
                                DealerSignal::RequestEqualityKeys => {
                                    let (keys0, keys1, random_pairs) = {
                                        let current_keys = equality_keys.clone();
                                        current_keys
                                    };

                                    let batch_server0 = DpfKeyBatch {
                                        keys: keys0,
                                        random_values: random_pairs.iter().map(|(r0, _)| r0.clone()).collect(),
                                    };

                                    let batch_server1 = DpfKeyBatch {
                                        keys: keys1,
                                        random_values: random_pairs.iter().map(|(_, r1)| r1.clone()).collect(),
                                    };

                                    self.write_equality_key_batch(&mut *server0_channel_guard, &batch_server0)
                                        .map_err(|e| format!("Failed to send keys to server 0 on channel {}: {}", channel_idx, e))?;

                                    self.write_equality_key_batch(&mut *server1_channel_guard, &batch_server1)
                                        .map_err(|e| format!("Failed to send keys to server 1 on channel {}: {}", channel_idx, e))?;

                                    equality_keys = self.generate_fss_keys_for_equality().map_err(|e| format!("Failed to generate equality keys: {}", e))?;
                                }
                                DealerSignal::RequestCheckKeys => {
                                    // Get current keys and generate new ones
                                    let (keys0, keys1, random_pairs) = {
                                        let current_keys = check_keys.clone();
                                        current_keys
                                    };

                                    // Send keys to both servers on this channel
                                    let batch_server0 = FssKeyBatch {
                                        keys: keys0,
                                        random_values: random_pairs.iter().map(|(r0, _)| *r0).collect(),
                                    };

                                    let batch_server1 = FssKeyBatch {
                                        keys: keys1,
                                        random_values: random_pairs.iter().map(|(_, r1)| *r1).collect(),
                                    };

                                    // Send to server 0 on this channel
                                    self.write_check_key_batch(&mut *server0_channel_guard, &batch_server0)
                                        .map_err(|e| format!("Failed to send keys to server 0 on channel {}: {}", channel_idx, e))?;

                                    // Send to server 1 on this channel
                                    self.write_check_key_batch(&mut *server1_channel_guard, &batch_server1)
                                        .map_err(|e| format!("Failed to send keys to server 1 on channel {}: {}", channel_idx, e))?;

                                    check_keys = self.generate_fss_keys_for_check().map_err(|e| format!("Failed to generate check keys: {}", e))?;
                                }
                                DealerSignal::RequestThresholdKeys => {
                                    // Get current keys
                                    let (keys0, keys1, random_pairs) = {
                                        let current_keys = threshold_keys.clone();
                                        current_keys
                                    };
                                    
                                    // Create batches
                                    let batch_server0 = FssKeyBatch {
                                        keys: keys0,
                                        random_values: random_pairs.iter().map(|(r0, _)| *r0).collect(),
                                    };

                                    let batch_server1 = FssKeyBatch {
                                        keys: keys1,
                                        random_values: random_pairs.iter().map(|(_, r1)| *r1).collect(),
                                    };

                                    // Send to server 0 on this channel
                                    self.write_threshold_key_batch(&mut *server0_channel_guard, &batch_server0)
                                        .map_err(|e| format!("Failed to send threshold keys to server 0 on channel {}: {}", channel_idx, e))?;

                                    // Send to server 1 on this channel
                                    self.write_threshold_key_batch(&mut *server1_channel_guard, &batch_server1)
                                        .map_err(|e| format!("Failed to send threshold keys to server 1 on channel {}: {}", channel_idx, e))?;

                                    threshold_keys = self.generate_fss_keys_for_threshold().map_err(|e| format!("Failed to generate threshold keys: {}", e))?;
                                }
                                DealerSignal::Shutdown => {
                                    println!("Dealer: Received shutdown signal on channel {}, terminating this thread", channel_idx);
                                    local_shutdown = true;
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            println!("Dealer: Error reading from server 0 channel {}: {}, terminating thread", channel_idx, e);
                            break;
                        }
                    }
                    
                    // No need for sleep since read_dealer_signal is blocking
                }

                Ok::<(), String>(())
            })
            .map_err(|e| format!("Parallel dealer processing failed: {}", e))?;

        println!("All dealer channels terminated");
        Ok(())
    }

    /// Read a dealer signal from the channel
    pub fn read_dealer_signal(&self, channel: &mut CommTrackingChannel) -> Result<DealerSignal, String> {
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

    pub fn write_equality_key_batch(&self, channel: &mut CommTrackingChannel, batch: &DpfKeyBatch) -> Result<(), String> {
        let modulus = 1u128 << self.check_output_bit_length;
        let data = batch.to_bytes();
        let len_bytes = (data.len() as u64).to_le_bytes();
        println!("Writing DPF key batch of size {} bytes to channel", data.len());
        channel.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to write length: {}", e))?;
        channel.write_bytes(&data)
            .map_err(|e| format!("Failed to write data: {}", e))?;
        channel.flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(())
    }

    /// Write an FSS key batch to the channel
    pub fn write_check_key_batch(&self, channel: &mut CommTrackingChannel, batch: &FssKeyBatch) -> Result<(), String> {
        let data = batch.to_bytes();
        let len_bytes = (data.len() as u64).to_le_bytes();
        println!("Writing FSS key batch of size {} bytes to channel", data.len());
        channel.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to write length: {}", e))?;
        channel.write_bytes(&data)
            .map_err(|e| format!("Failed to write data: {}", e))?;
        channel.flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(())
    }

    pub fn write_threshold_key_batch(&self, channel: &mut CommTrackingChannel, batch: &FssKeyBatch) -> Result<(), String> {
        let data = batch.to_bytes();
        let len_bytes = (data.len() as u64).to_le_bytes();
        println!("Writing Threshold key batch of size {} bytes to channel", data.len());
        channel.write_bytes(&len_bytes)
            .map_err(|e| format!("Failed to write length: {}", e))?;
        channel.write_bytes(&data)
            .map_err(|e| format!("Failed to write data: {}", e))?;
        channel.flush()
            .map_err(|e| format!("Failed to flush: {}", e))?;
        Ok(())
    }

    pub fn generate_fss_keys_for_equality(&self) -> Result<(Vec<DpfKey<1>>, Vec<DpfKey<1>>, Vec<(Vec<bool>, Vec<bool>)>), String> {
        let out_modulus = 1u128 << self.check_output_bit_length;

        let mut random_pairs = Vec::new();

        for _ in 0..self.n_clients {
            // Generate random pair (r0, r1) for this check using standard rand
            let r0 = (0..self.check_input_bit_length * self.dimensions).map(|_| rand::rng().random::<bool>()).collect::<Vec<_>>();
            let r1 = (0..self.check_input_bit_length * self.dimensions).map(|_| rand::rng().random::<bool>()).collect::<Vec<_>>();
            random_pairs.push((r0, r1));
        }

        let key_pairs: Vec<(DpfKey<1>, DpfKey<1>)> = random_pairs
            .par_iter()
            .map(|(r0, r1)| {
                let sum_bits = r0.iter().zip(r1.iter())
                    .map(|(&b0, &b1)| b0 ^ b1)
                    .collect::<Vec<_>>();
                let (key0, key1) = DpfKey::<1>::gen_DpfKey(
                    &sum_bits,
                    &RingVec::<1>::new([1], out_modulus),
                    &RingVec::<1>::new([0], out_modulus),
                    out_modulus
                ); // 1 if equal, 0 if not
                (key0, key1)
            })
            .collect();

        let (key0s, key1s) = key_pairs.into_iter().unzip();
        Ok((key0s, key1s, random_pairs))
    }

    /// Generate FSS keys for check phase comparison (Lp distance with IntervalFSS)
    /// This simulates a trusted dealer generating FSS keys for distance threshold comparison
    pub fn generate_fss_keys_for_check(&self) -> Result<(Vec<(LdcfKey<1>, RdcfKey<1>)>, Vec<(LdcfKey<1>, RdcfKey<1>)>, Vec<(u128, u128)>), String> {
        let in_modulus = 1u128 << self.check_input_bit_length;
        let out_modulus = 1u128 << self.check_output_bit_length;
    
        let mut random_pairs = Vec::new();
    
        for _ in 0..self.n_clients {
            // Generate random pair (r0, r1) for this check using standard rand
            let mut std_rng = rand::rng();
            let r0 = std_rng.random_range(0..in_modulus);
            let r1 = std_rng.random_range(0..in_modulus);
            random_pairs.push((r0, r1));
        }
    
        // Use parallel processing for FSS key generation
        let key_pairs: Vec<((LdcfKey<1>, RdcfKey<1>), (LdcfKey<1>, RdcfKey<1>))> = random_pairs
            .par_iter()
            .map(|&(r0, r1)| {
                // Check if distance_threshold + r0 + r1 would wrap around
                let sum = self.distance_threshold + (r0 + r1) % in_modulus;
                let wraps_around = sum >= in_modulus;
                let zero_payload = RingVec::<1>::new([0], out_modulus);
                let one_payload = RingVec::<1>::new([1], out_modulus);
                if wraps_around {
                    // Wrap-around case: interval [distance_threshold+r0+r1 mod modulus, r0+r1]
                    // Return 0 in the middle, 1 on left and right
                    let interval_start = (sum + 1) % in_modulus;
                    let interval_end = (r0 + r1 - 1) % in_modulus;

                    let alpha_bits = u128_to_bits_msb(interval_start, self.check_input_bit_length);
                    let beta_bits = u128_to_bits_msb(interval_end, self.check_input_bit_length);

                    let (key00, key10) = LdcfKey::<1>::gen_LdcfKey(
                        &alpha_bits,
                        &one_payload,
                        &zero_payload,
                        out_modulus
                    );

                    let (key01, key11) = RdcfKey::<1>::gen_RdcfKey(
                        &beta_bits,
                        &zero_payload,
                        &one_payload,
                        out_modulus
                    );

                    ((key00, key01), (key10, key11))
                } else {
                    // No wrap-around case: interval [r0+r1, distance_threshold+r0+r1]
                    // Return 1 inside interval (distance <= threshold), 0 outside
                    let interval_start = (r0 + r1) % in_modulus;
                    let interval_end = sum;

                    let alpha_bits = u128_to_bits_msb(interval_start, self.check_input_bit_length);
                    let beta_bits = u128_to_bits_msb(interval_end, self.check_input_bit_length);

                    let (key00, key10) = LdcfKey::<1>::gen_LdcfKey(
                        &alpha_bits,
                        &zero_payload,
                        &one_payload,
                        out_modulus
                    );

                    let (key01, key11) = RdcfKey::<1>::gen_RdcfKey(
                        &beta_bits,
                        &zero_payload,
                        &(zero_payload - one_payload),
                        out_modulus
                    );

                    ((key00, key01), (key10, key11))
                }
            })
            .collect();

        let (key0s, key1s) = key_pairs.into_iter().unzip();
        Ok((key0s, key1s, random_pairs))
    }

    /// Generate FSS keys for interval FSS threshold comparison
    /// This simulates a trusted dealer generating FSS keys
    fn generate_fss_keys_for_threshold(&self) -> Result<(Vec<(LdcfKey<1>, RdcfKey<1>)>, Vec<(LdcfKey<1>, RdcfKey<1>)>, Vec<(u128, u128)>), String> {
        let modulus = 1u128 << self.check_output_bit_length;
        let mut random_pairs = Vec::new();
    
        // Generate random pair (r0, r1) for this query using standard rand
        let mut std_rng = rand::rng();
        let r0 = std_rng.random_range(0..modulus);
        let r1 = std_rng.random_range(0..modulus);
        random_pairs.push((r0, r1));

        let zero_payload = RingVec::<1>::new([0], modulus); 
        let one_payload = RingVec::<1>::new([1], modulus);

        // Check if count_threshold + r0 + r1 would wrap around
        let sum = self.count_threshold + (r0 + r1) % modulus;
        if sum >= modulus {
            // Wrap-around case: interval [threshold+r0+r1 mod modulus, r0+r1]
            // Return 1 in the middle, 0 on left and right
            let interval_start = sum % modulus;
            let interval_end = (r0 + r1) % modulus;
            
            let alpha_bits = u128_to_bits_msb(interval_start, self.check_output_bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, self.check_output_bit_length);

            let (key00, key10) = LdcfKey::<1>::gen_LdcfKey(
                &alpha_bits,
                &zero_payload,
                &one_payload,
                2,
            );

            let (key01, key11) = RdcfKey::<1>::gen_RdcfKey(
                &beta_bits,
                &zero_payload,
                &(zero_payload - one_payload),
                2,
            );

            Ok((vec![(key00, key01)], vec![(key10, key11)], random_pairs))
        } else {
            // No wrap-around case: interval [r0+r1, threshold+r0+r1]
            // Return 1 on left and right, 0 in the middle
            let interval_start = (r0 + r1) % modulus;
            let interval_end = (sum + modulus - 1) % modulus;
            
            let alpha_bits = u128_to_bits_msb(interval_start, self.check_output_bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, self.check_output_bit_length);
            
            let (key00, key10) = LdcfKey::<1>::gen_LdcfKey(
                &alpha_bits,
                &one_payload,
                &zero_payload,
                2,
            );

            let (key01, key11) = RdcfKey::<1>::gen_RdcfKey(
                &beta_bits,
                &zero_payload,
                &one_payload,
                2,
            );

            Ok((vec![(key00, key01)], vec![(key10, key11)], random_pairs))
        }
    }

}
