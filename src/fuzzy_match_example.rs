//! Example usage of the FuzzyMatch protocol
//!
//! This example demonstrates how to use the complete FuzzyMatch protocol
//! with share, check, and threshold phases.

use std::io::{BufReader, BufWriter};
use std::os::unix::net::UnixStream;
use scuttlebutt::{AesRng, Channel};

use crate::fuzzy_match::{FuzzyMatch, FuzzyMatchConfig};
use crate::share_phase::{ShareConfig, ShareMethod, ShareData};
use crate::check_phase::CheckConfig;
use crate::threshold_phase::ThresholdConfig;
use crate::data_structures::modint::ModInt;

/// Example of running the complete FuzzyMatch protocol
pub fn run_fuzzy_match_example() -> Result<(), Box<dyn std::error::Error>> {
    // Configuration setup
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        input_bit_length: 8,
        output_bit_length: 8,
        dimension: 2,
        data: ShareData::OKVS {
            r1: [1u8; 16],
            r2: [2u8; 16],
        },
    };

    let check_config = CheckConfig {
        input_bit_length: 8,
        output_bit_length: 8,
        num_dimensions: 2,
        is_garbler_side: true,
    };

    let threshold_config = ThresholdConfig {
        modulus: 256, // Power of 2
        is_garbler_side: true,
    };

    let fuzzy_config = FuzzyMatchConfig {
        share_config,
        check_config,
        threshold_config,
        l2_threshold: 100.0,
    };

    // Create protocol instances
    let fuzzy_match_server1 = FuzzyMatch::new(fuzzy_config.clone());
    
    let mut server2_config = fuzzy_config.clone();
    server2_config.check_config.is_garbler_side = false;
    server2_config.threshold_config.is_garbler_side = false;
    let fuzzy_match_server2 = FuzzyMatch::new(server2_config);

    // Example: 3 clients share their points
    let clients_data = vec![
        vec![10u128, 20u128],  // Client 1's 2D point
        vec![15u128, 25u128],  // Client 2's 2D point  
        vec![30u128, 40u128],  // Client 3's 2D point
    ];
    
    let delta = 5u128; // Tolerance for matching

    // Phase 1: Share phase - clients share their points
    let mut client_shares_server1 = Vec::new();
    let mut client_shares_server2 = Vec::new();

    for client_point in &clients_data {
        let (share1, share2) = fuzzy_match_server1.share_point(client_point, delta)?;
        client_shares_server1.push(share1);
        client_shares_server2.push(share2);
    }

    // Query point that servers want to check against
    let query_point = vec![12u128, 22u128];
    
    // Threshold: how many clients must match
    let threshold = 2u128; // At least 2 clients must match

    // Create communication channel (in real implementation, this would be network sockets)
    let (socket1, socket2) = UnixStream::pair()?;
    let mut rng = AesRng::new();
    
    // Server 1 setup
    let reader1 = BufReader::new(socket1.try_clone()?);
    let writer1 = BufWriter::new(socket1);
    let mut channel1 = Channel::new(reader1, writer1);
    
    // In a real scenario, servers would run in parallel
    // Here we simulate the protocol execution
    println!("Running FuzzyMatch protocol...");
    println!("Clients data: {:?}", clients_data);
    println!("Query point: {:?}", query_point);
    println!("Threshold: {} clients must match", threshold);
    
    // This would typically be run in separate threads/processes for each server
    // For demonstration, we show the API calls each server would make
    
    // Server 1 would call:
    // let result1 = fuzzy_match_server1.run_complete_protocol(
    //     &client_shares_server1,
    //     &query_point, 
    //     threshold.clone(),
    //     &mut channel1,
    //     &mut rng
    // )?;
    
    println!("Protocol setup complete!");
    println!("In a real implementation:");
    println!("- Server 1 would process {} client shares", client_shares_server1.len());
    println!("- Server 2 would process {} client shares", client_shares_server2.len());
    println!("- Both would run check phase for query point {:?}", query_point);
    println!("- Final result would indicate if >= {} clients match", threshold);

    Ok(())
}

/// Example demonstrating individual phases
pub fn demonstrate_individual_phases() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Individual Phase Demonstration ===");
    
    // Setup (same as before)
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        input_bit_length: 8,
        output_bit_length: 8,
        dimension: 2,
        data: ShareData::OKVS {
            r1: [1u8; 16],
            r2: [2u8; 16],
        },
    };

    let check_config = CheckConfig {
        input_bit_length: 8,
        output_bit_length: 8,
        num_dimensions: 2,
        is_garbler_side: true,
    };

    let threshold_config = ThresholdConfig {
        modulus: 256,
        is_garbler_side: true,
    };

    let fuzzy_config = FuzzyMatchConfig {
        share_config,
        check_config,
        threshold_config,
        l2_threshold: 100.0,
    };

    let fuzzy_match = FuzzyMatch::new(fuzzy_config);
    
    // Phase 1: Share Phase
    println!("\n1. Share Phase:");
    let client_point = vec![15u128, 25u128];
    let delta = 3u128;
    let (share1, share2) = fuzzy_match.share_point(&client_point, delta)?;
    println!("   Client shared point {:?} with tolerance {}", client_point, delta);
    println!("   Generated 2 shares (one for each server)");
    
    // Phase 2: Check Phase (would require actual network communication)
    println!("\n2. Check Phase:");
    let query_point = vec![16u128, 24u128];
    println!("   Servers would check if query {:?} matches client's shared point", query_point);
    println!("   Process:");
    println!("   - For each dimension i, evaluate query[i] with OKVS[i]");
    println!("   - Concatenate all bit vectors");
    println!("   - Run equality test via garbled circuits");
    println!("   - Convert boolean result to ring share via OT");
    
    // Phase 3: Threshold Phase
    println!("\n3. Threshold Phase:");
    println!("   Servers would:");
    println!("   - Aggregate ring shares from all clients");
    println!("   - Compare sum with threshold using garbled circuits");
    println!("   - Output: does the number of matches exceed threshold?");
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_example_setup() {
        // Test that we can set up the protocol without errors
        let result = demonstrate_individual_phases();
        assert!(result.is_ok(), "Example setup should succeed");
    }
}
