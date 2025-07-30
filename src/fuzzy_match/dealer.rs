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
use crate::util::{u128_to_bits, u128_to_bits_msb};
use std::sync::mpsc;
use std::thread;
use std::time::Instant;
use rand::Rng;

/// FSS key batch for check phase - contains keys for one server
#[derive(Clone, Debug)]
pub struct FssKeyBatch {
    pub keys: Vec<IntervalFSSKey<1>>,
    pub random_values: Vec<u128>,
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
    RequestKeys,
    Shutdown,
}

/// Dealer that generates FSS key pairs on-demand based on server signals
/// Only server 0 should send RequestKeys signals - the dealer will send keys to both servers
pub struct FssDealer {
    receiver_server0: mpsc::Receiver<FssKeyBatch>,
    receiver_server1: mpsc::Receiver<FssKeyBatch>,
    signal_sender_server0: mpsc::Sender<DealerSignal>,
    signal_sender_server1: mpsc::Sender<DealerSignal>,
}

impl FssDealer {
    /// Create a new FSS dealer that generates key pairs on-demand based on server signals
    /// Server 0 acts as the coordinator - when it sends RequestKeys, dealer sends keys to both servers
    /// Returns the dealer with separate channels for both servers
    pub fn new(
        n_clients: usize,
        distance_threshold: u128,
        output_bit_length: usize,
        check_output_bit_length: usize,
    ) -> Result<Self, String> {
        let (batch_sender_server0, batch_receiver_server0) = mpsc::channel::<FssKeyBatch>();
        let (batch_sender_server1, batch_receiver_server1) = mpsc::channel::<FssKeyBatch>();
        let (signal_sender_server0, signal_receiver_server0) = mpsc::channel::<DealerSignal>();
        let (signal_sender_server1, signal_receiver_server1) = mpsc::channel::<DealerSignal>();
        
        // Spawn the dealer thread
        thread::spawn(move || {
            // Generate initial set of keys
            let mut current_keys = match generate_fss_keys_for_check(
                distance_threshold,
                output_bit_length,
                check_output_bit_length,
                n_clients,
            ) {
                Ok(keys) => {
                    Some(keys)
                }
                Err(e) => {
                    eprintln!("FSS dealer: failed to generate initial keys: {}", e);
                    None
                }
            };
            
            loop {
                // Only listen for signals from server 0 - it will coordinate key requests for both servers
                let signal_server0 = signal_receiver_server0.try_recv();
                let signal_server1 = signal_receiver_server1.try_recv();
                
                // Handle server 0 signal (primary coordinator)
                if let Ok(signal) = signal_server0 {
                    match signal {
                        DealerSignal::RequestKeys => {
                            if let Some((keys0, keys1, random_pairs)) = &current_keys {
                                // Send keys to BOTH servers
                                let batch_server0 = FssKeyBatch {
                                    keys: keys0.clone(),
                                    random_values: random_pairs.iter().map(|(r0, _)| *r0).collect(),
                                };
                                
                                let batch_server1 = FssKeyBatch {
                                    keys: keys1.clone(),
                                    random_values: random_pairs.iter().map(|(_, r1)| *r1).collect(),
                                };
                                
                                // Send to server 0
                                if batch_sender_server0.send(batch_server0).is_err() {
                                    println!("FSS dealer: server 0 disconnected, shutting down");
                                    break;
                                }
                                
                                // Send to server 1
                                if batch_sender_server1.send(batch_server1).is_err() {
                                    println!("FSS dealer: server 1 disconnected, shutting down");
                                    break;
                                }
                                
                                // Generate new keys for next request AFTER sending to both servers
                                match generate_fss_keys_for_check(
                                    distance_threshold,
                                    output_bit_length,
                                    check_output_bit_length,
                                    n_clients,
                                ) {
                                    Ok(new_keys) => {
                                        current_keys = Some(new_keys);
                                    }
                                    Err(e) => {
                                        eprintln!("FSS dealer: failed to generate new keys: {}", e);
                                    }
                                }
                            }
                        }
                        DealerSignal::Shutdown => {
                            println!("FSS dealer: received shutdown signal from server 0");
                            break;
                        }
                    }
                }
                
                // Handle shutdown signal from server 1 (but not key requests)
                if let Ok(signal) = signal_server1 {
                    match signal {
                        DealerSignal::RequestKeys => {
                            // Ignore key requests from server 1 - only server 0 coordinates
                            println!("FSS dealer: ignoring key request from server 1 (only server 0 coordinates)");
                        }
                        DealerSignal::Shutdown => {
                            println!("FSS dealer: received shutdown signal from server 1");
                            break;
                        }
                    }
                }
            }
        });
        
        Ok(FssDealer {
            receiver_server0: batch_receiver_server0,
            receiver_server1: batch_receiver_server1,
            signal_sender_server0,
            signal_sender_server1,
        })
    }
    
    /// Extract the receivers and signal senders for use by the protocol
    pub fn into_channels(self) -> (
        mpsc::Receiver<FssKeyBatch>, 
        mpsc::Receiver<FssKeyBatch>, 
        mpsc::Sender<DealerSignal>,
        mpsc::Sender<DealerSignal>
    ) {
        (self.receiver_server0, self.receiver_server1, self.signal_sender_server0, self.signal_sender_server1)
    }
}

/// Generate FSS keys for interval FSS threshold comparison
/// This simulates a trusted dealer generating FSS keys
pub fn generate_fss_keys_for_threshold(
    threshold: u128,
    bit_length: usize,
    num_queries: usize,
) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
    let modulus = 1u128 << bit_length;
    
    let mut keys_server0 = Vec::new();
    let mut keys_server1 = Vec::new();
    let mut random_pairs = Vec::new();
    
    for _ in 0..num_queries {
        // Generate random pair (r0, r1) for this query using standard rand
        let mut std_rng = rand::thread_rng();
        let r0 = std_rng.gen_range(0..modulus);
        let r1 = std_rng.gen_range(0..modulus);
        random_pairs.push((r0, r1));
        
        // Check if threshold + r0 + r1 would wrap around
        let sum = threshold + r0 + r1;
        let wraps_around = sum >= modulus;
        let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
            // Wrap-around case: interval [threshold+r0+r1 mod modulus, r0+r1]
            // Return 1 in the middle, 0 on left and right
            let interval_start = sum % modulus;
            let interval_end = (r0 + r1) % modulus;
            
            let alpha_bits = u128_to_bits_msb(interval_start, bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, bit_length);
            
            // For wrap-around: left=0, middle=1, right=0
            let a = RingVec::<1>::new([0], 2); // left
            let b = RingVec::<1>::new([1], 2); // middle
            let c = RingVec::<1>::new([0], 2); // right
            
            (alpha_bits, beta_bits, a, b, c)
        } else {
            // No wrap-around case: interval [r0+r1, threshold+r0+r1]
            // Return 1 on left and right, 0 in the middle
            let interval_start = (r0 + r1) % modulus;
            let interval_end = sum;
            
            let alpha_bits = u128_to_bits_msb(interval_start, bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, bit_length);
            
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

/// Generate FSS keys for check phase comparison (Lp distance with IntervalFSS)
/// This simulates a trusted dealer generating FSS keys for distance threshold comparison
pub fn generate_fss_keys_for_check(
    distance_threshold: u128,
    input_bit_length: usize,
    output_bit_length: usize,
    num_checks: usize,
) -> Result<(Vec<IntervalFSSKey<1>>, Vec<IntervalFSSKey<1>>, Vec<(u128, u128)>), String> {
    let in_modulus = 1u128 << input_bit_length;
    let out_modulus = 1u128 << output_bit_length;
    
    let mut keys_server0 = Vec::new();
    let mut keys_server1 = Vec::new();
    let mut random_pairs = Vec::new();
    
    for _ in 0..num_checks {
        // Generate random pair (r0, r1) for this check using standard rand
        let mut std_rng = rand::thread_rng();
        let r0 = std_rng.gen_range(0..in_modulus);
        let r1 = std_rng.gen_range(0..in_modulus);
        random_pairs.push((r0, r1));
    }
    
    for &(r0, r1) in &random_pairs {
        // Check if distance_threshold + r0 + r1 would wrap around
        let sum = distance_threshold + (r0 + r1) % in_modulus;
        let wraps_around = sum >= in_modulus;
        let (alpha_bits, beta_bits, a, b, c) = if wraps_around {
            // Wrap-around case: interval [distance_threshold+r0+r1 mod modulus, r0+r1]
            // Return 0 in the middle, 1 on left and right
            let interval_start = sum % in_modulus;
            let interval_end = (r0 + r1) % in_modulus;
            
            let alpha_bits = u128_to_bits_msb(interval_start, input_bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, input_bit_length);
            
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

            let alpha_bits = u128_to_bits_msb(interval_start, input_bit_length);
            let beta_bits = u128_to_bits_msb(interval_end, input_bit_length);

            
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
        
        keys_server0.push(key0);
        keys_server1.push(key1);
    }
    
    Ok((keys_server0, keys_server1, random_pairs))
}
