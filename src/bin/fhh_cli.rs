//! CLI for Fuzzy Heavy Hitters Protocol
//! 
//! This CLI allows running the complete fuzzy heavy hitters protocol locally
//! with both servers in the same process for testing purposes.

use counttree::{
    protocol::{FuzzyHeavyHittersProtocol, generate_fss_keys_for_threshold},
    fuzzy_match::threshold_phase::ThresholdData,
    cli_config::CliConfig,
};
use clap::{App, Arg, SubCommand};
use std::process;
use std::thread;
use std::os::unix::net::UnixStream;
use std::fs;
use serde_json;

/// Data structure for synthetic data
#[derive(serde::Deserialize)]
struct Cluster {
    center: Vec<u128>,
    points: Vec<Vec<u128>>,
}

/// Load cluster data from JSON file
fn load_clusters(file_path: &str) -> Result<Vec<Cluster>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read data file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse data file {}: {}", file_path, e))
}

/// Load query points from JSON file
fn load_query_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read query file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse query file {}: {}", file_path, e))
}

/// Run the protocol with both servers in the same process (for testing)
fn run_local_protocol(config_path: &str) -> Result<(), String> {
    println!("Loading configuration from {}", config_path);
    let cli_config = CliConfig::from_file(config_path)?;
    
    // Check dictionary type and route to appropriate handler
    match cli_config.protocol.dictionary_type.as_str() {
        "Known" => run_known_dictionary_protocol(cli_config),
        "Unknown" => run_unknown_dictionary_protocol(cli_config),
        other => Err(format!("Unsupported dictionary type: {}", other)),
    }
}

/// Run the protocol for known dictionary case
fn run_known_dictionary_protocol(cli_config: CliConfig) -> Result<(), String> {
    println!("Loading synthetic data from {}", cli_config.data_file);
    let clusters = load_clusters(&cli_config.data_file)?;
    
    println!("Loading query points from {}", cli_config.query_file);
    let query_points = load_query_points(&cli_config.query_file)?;
    
    // Extract client points from clusters
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    println!("Loaded {} client points from {} clusters", client_points.len(), clusters.len());
    println!("Testing {} query points", query_points.len());
    
    // Create initial protocol configurations for both servers
    let protocol_config_server0 = cli_config.to_protocol_config(false)?;
    let protocol_config_server1 = cli_config.to_protocol_config(true)?;

    // Generate FSS keys and create threshold data if using IntervalFSS
    let (threshold_data_list_server0, threshold_data_list_server1) = if cli_config.protocol.threshold_method == "IntervalFSS" {
        println!("Generating FSS keys for IntervalFSS...");
        let (keys0, keys1, random_pairs) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            cli_config.protocol.check_output_bit_length,
            query_points.len(),
        )?;
        
        println!("Generated {} random pairs for FSS keys", random_pairs.len());
        
        // Create threshold data for each query using corresponding random pairs
        let mut threshold_data_list_0 = Vec::new();
        let mut threshold_data_list_1 = Vec::new();
        
        for ((fss_key_0, fss_key_1), (r0, r1)) in keys0.iter().zip(keys1.iter()).zip(random_pairs.iter()) {
            threshold_data_list_0.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_0.clone(),
                random_value: *r0,
            });
            threshold_data_list_1.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_1.clone(),
                random_value: *r1,
            });
        }
        
        (threshold_data_list_0, threshold_data_list_1)
    } else {
        let threshold_data = ThresholdData::GarbledCircuits;
        let threshold_data_list_0 = vec![threshold_data.clone(); query_points.len()];
        let threshold_data_list_1 = vec![threshold_data; query_points.len()];
        (threshold_data_list_0, threshold_data_list_1)
    };

    let protocol_server0 = FuzzyHeavyHittersProtocol::new(protocol_config_server0);
    let protocol_server1 = FuzzyHeavyHittersProtocol::new(protocol_config_server1);

    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol_server0.generate_client_shares(&client_points)?;
    
    println!("Starting protocol execution...");
    println!("Using {} threshold method", cli_config.protocol.threshold_method);
    println!("Threshold: {}, Delta: {}", cli_config.protocol.threshold, cli_config.protocol.delta);
    println!();
    
    // Create Unix socket pair for communication
    let (stream1, stream2) = UnixStream::pair()
        .map_err(|e| format!("Failed to create Unix socket pair: {}", e))?;
    
    // Run server 1 in a separate thread
    let shares_server1_clone = shares_server1.clone();
    let query_points_clone = query_points.clone();
    let threshold_data_list_server1_clone = threshold_data_list_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1.run_server1(
            &shares_server1_clone,
            &query_points_clone,
            &threshold_data_list_server1_clone,
            stream1,
        )
    });
    
    // Run server 0 in main thread
    let final_results = protocol_server0.run_server0(
        &shares_server0,
        &query_points,
        &threshold_data_list_server0,
        stream2,
    )?;
    
    // Wait for server 1 to complete (both servers should return the same final results)
    let _result_server1 = server1_handle.join()
        .map_err(|e| format!("Server 1 thread failed: {:?}", e))??;
    
    // Display results and comparison with expected
    display_known_dictionary_results(&cli_config, &query_points, &client_points, &final_results)?;
    
    Ok(())
}

/// Run the protocol for unknown dictionary case
fn run_unknown_dictionary_protocol(cli_config: CliConfig) -> Result<(), String> {
    println!("Loading synthetic data from {}", cli_config.data_file);
    let clusters = load_clusters(&cli_config.data_file)?;
    
    // Extract client points from clusters
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    println!("Loaded {} client points from {} clusters", client_points.len(), clusters.len());
    println!("Running unknown dictionary search...");
    
    // Create initial protocol configurations for both servers
    let protocol_config_server0 = cli_config.to_protocol_config(false)?;
    let protocol_config_server1 = cli_config.to_protocol_config(true)?;

    // For unknown dictionary, we need multiple threshold data instances for the search
    // Generate enough for a reasonable search depth
    let max_search_iterations = 100; // Adjust based on expected search complexity
    
    let (threshold_data_list_server0, threshold_data_list_server1) = if cli_config.protocol.threshold_method == "IntervalFSS" {
        println!("Generating FSS keys for IntervalFSS...");
        let (keys0, keys1, random_pairs) = generate_fss_keys_for_threshold(
            cli_config.protocol.threshold,
            cli_config.protocol.check_output_bit_length,
            max_search_iterations,
        )?;
        
        println!("Generated {} random pairs for FSS keys", random_pairs.len());
        
        // Create threshold data for search using corresponding random pairs
        let mut threshold_data_list_0 = Vec::new();
        let mut threshold_data_list_1 = Vec::new();
        
        for ((fss_key_0, fss_key_1), (r0, r1)) in keys0.iter().zip(keys1.iter()).zip(random_pairs.iter()) {
            threshold_data_list_0.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_0.clone(),
                random_value: *r0,
            });
            threshold_data_list_1.push(ThresholdData::IntervalFSS {
                fss_key: fss_key_1.clone(),
                random_value: *r1,
            });
        }
        
        (threshold_data_list_0, threshold_data_list_1)
    } else {
        let threshold_data = ThresholdData::GarbledCircuits;
        let threshold_data_list_0 = vec![threshold_data.clone(); max_search_iterations];
        let threshold_data_list_1 = vec![threshold_data; max_search_iterations];
        (threshold_data_list_0, threshold_data_list_1)
    };

    let protocol_server0 = FuzzyHeavyHittersProtocol::new(protocol_config_server0);
    let protocol_server1 = FuzzyHeavyHittersProtocol::new(protocol_config_server1);

    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol_server0.generate_client_shares(&client_points)?;
    
    println!("Starting unknown dictionary search...");
    println!("Using {} threshold method", cli_config.protocol.threshold_method);
    println!("Threshold: {}, Delta: {}", cli_config.protocol.threshold, cli_config.protocol.delta);
    println!();
    
    // Create Unix socket pair for communication
    let (stream1, stream2) = UnixStream::pair()
        .map_err(|e| format!("Failed to create Unix socket pair: {}", e))?;
    
    // Run server 1 in a separate thread
    let shares_server0_clone = shares_server0.clone();
    let shares_server1_clone = shares_server1.clone();
    let threshold_data_list_server0_clone = threshold_data_list_server0.clone();
    let threshold_data_list_server1_clone = threshold_data_list_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1.run_unknown_dictionary_search(
            &shares_server0_clone,
            &shares_server1_clone,
            &threshold_data_list_server0_clone,
            &threshold_data_list_server1_clone,
            stream1,
            true, // is_server1 = true
        )
    });
    
    // Run server 0 in main thread
    let heavy_hitter_prefixes = protocol_server0.run_unknown_dictionary_search(
        &shares_server0,
        &shares_server1,
        &threshold_data_list_server0,
        &threshold_data_list_server1,
        stream2,
        false, // is_server1 = false
    )?;
    
    // Wait for server 1 to complete (both servers should return the same results)
    let _result_server1 = server1_handle.join()
        .map_err(|e| format!("Server 1 thread failed: {:?}", e))??;
    
    // Display results
    display_unknown_dictionary_results(&cli_config, &client_points, &heavy_hitter_prefixes)?;
    
    Ok(())
}

/// Display results for known dictionary protocol
fn display_known_dictionary_results(
    cli_config: &CliConfig,
    query_points: &[Vec<u128>],
    client_points: &[Vec<u128>],
    final_results: &[bool],
) -> Result<(), String> {
    // Display results (final_results already contains the XOR of both servers' bits)
    println!("=== Protocol Results ===");
    let mut total_heavy_hitters = 0;
    
    for (i, query_point) in query_points.iter().enumerate() {
        let final_result = final_results[i];
        
        if final_result {
            total_heavy_hitters += 1;
        }
        
        let status = if final_result { "HEAVY HITTER" } else { "not heavy hitter" };
        
        if cli_config.output.verbose {
            println!("Query {}: {:?} -> {}", 
                i + 1, query_point, status);
        } else {
            println!("Query {}: {:?} -> {}", i + 1, query_point, status);
        }
    }
    
    println!();
    println!("Summary: {}/{} query points are heavy hitters", total_heavy_hitters, query_points.len());
    
    // Calculate expected results manually for comparison
    println!("\n=== Expected Results (Manual Calculation) ===");
    let mut expected_heavy_hitters = 0;
    
    for (i, query_point) in query_points.iter().enumerate() {
        let mut count = 0;
        
        // Count client points within delta distance of this query point
        for client_point in client_points {
            // Calculate L-infinity distance (max coordinate difference)
            let mut l_inf_distance = 0;
            for dim in 0..query_point.len() {
                let diff = if query_point[dim] > client_point[dim] {
                    query_point[dim] - client_point[dim]
                } else {
                    client_point[dim] - query_point[dim]
                };
                l_inf_distance = l_inf_distance.max(diff);
            }
            
            if l_inf_distance <= cli_config.protocol.delta as u128 {
                count += 1;
            }
        }
        
        let is_expected_heavy_hitter = count >= cli_config.protocol.threshold as usize;
        if is_expected_heavy_hitter {
            expected_heavy_hitters += 1;
        }
        
        let status = if is_expected_heavy_hitter { "HEAVY HITTER" } else { "not heavy hitter" };
        let protocol_result = final_results[i];
        let match_str = if is_expected_heavy_hitter == protocol_result { "✓" } else { "✗" };
        
        if cli_config.output.verbose {
            println!("Query {}: {:?} -> {} (count: {}) {}", 
                i + 1, query_point, status, count, match_str);
        } else {
            println!("Query {}: {:?} -> {} (count: {}) {}", 
                i + 1, query_point, status, count, match_str);
        }
    }
    
    println!();
    println!("Expected Summary: {}/{} query points are heavy hitters", expected_heavy_hitters, query_points.len());
    
    let matches = query_points.iter().enumerate().filter(|(i, _)| {
        let count = client_points.iter().filter(|client_point| {
            let mut l_inf_distance = 0;
            for dim in 0..query_points[*i].len() {
                let diff = if query_points[*i][dim] > client_point[dim] {
                    query_points[*i][dim] - client_point[dim]
                } else {
                    client_point[dim] - query_points[*i][dim]
                };
                l_inf_distance = l_inf_distance.max(diff);
            }
            l_inf_distance <= cli_config.protocol.delta as u128
        }).count();
        let expected = count >= cli_config.protocol.threshold as usize;
        expected == final_results[*i]
    }).count();
    
    println!("Protocol Accuracy: {}/{} results match expected ({:.1}%)", 
        matches, query_points.len(), (matches as f64 / query_points.len() as f64) * 100.0);
    
    Ok(())
}

/// Display results for unknown dictionary protocol
fn display_unknown_dictionary_results(
    cli_config: &CliConfig,
    client_points: &[Vec<u128>],
    heavy_hitter_prefixes: &[Vec<Vec<bool>>],
) -> Result<(), String> {
    println!("=== Unknown Dictionary Search Results ===");
    println!("Found {} heavy hitter prefixes", heavy_hitter_prefixes.len());
    
    if heavy_hitter_prefixes.is_empty() {
        println!("No heavy hitters found in the search space.");
        return Ok(());
    }
    
    // Convert prefixes back to readable format and display
    for (i, prefix_set) in heavy_hitter_prefixes.iter().enumerate() {
        println!("\nHeavy Hitter Prefix {}: ", i + 1);
        for (dim, prefix) in prefix_set.iter().enumerate() {
            let prefix_str: String = prefix.iter().map(|&b| if b { '1' } else { '0' }).collect();
            println!("  Dimension {}: {} (length: {})", dim, prefix_str, prefix.len());
        }
        
        // Estimate how many points this prefix could represent
        let total_bits = cli_config.protocol.input_bit_length;
        let mut remaining_space = 1u128;
        for prefix in prefix_set {
            let remaining_bits = total_bits - prefix.len();
            remaining_space *= 1u128 << remaining_bits;
        }
        println!("  Represents up to {} possible points", remaining_space);
    }
    
    // Calculate some statistics about the discovered prefixes
    let mut total_prefix_bits = 0;
    let mut min_prefix_length = usize::MAX;
    let mut max_prefix_length = 0;
    
    for prefix_set in heavy_hitter_prefixes {
        for prefix in prefix_set {
            total_prefix_bits += prefix.len();
            min_prefix_length = min_prefix_length.min(prefix.len());
            max_prefix_length = max_prefix_length.max(prefix.len());
        }
    }
    
    if !heavy_hitter_prefixes.is_empty() {
        let avg_prefix_length = total_prefix_bits as f64 / (heavy_hitter_prefixes.len() * cli_config.protocol.dimensions) as f64;
        println!("\n=== Prefix Statistics ===");
        println!("Average prefix length per dimension: {:.1} bits", avg_prefix_length);
        println!("Min prefix length: {} bits", min_prefix_length);
        println!("Max prefix length: {} bits", max_prefix_length);
        println!("Total space explored: {:.1}%", 
            (total_prefix_bits as f64 / (heavy_hitter_prefixes.len() * cli_config.protocol.dimensions * cli_config.protocol.input_bit_length) as f64) * 100.0);
    }
    
    // Manual verification: check how many actual client points fall under these prefixes
    println!("\n=== Manual Verification ===");
    let mut verified_count = 0;
    
    for prefix_set in heavy_hitter_prefixes {
        let mut count_for_this_prefix = 0;
        
        // Check each client point to see if it matches this prefix
        for client_point in client_points {
            let mut matches_prefix = true;
            
            for (dim, prefix) in prefix_set.iter().enumerate() {
                if dim >= client_point.len() {
                    continue;
                }
                
                // Convert client point coordinate to binary and check prefix match
                let point_bits = format!("{:0width$b}", client_point[dim], width = cli_config.protocol.input_bit_length);
                let point_binary: Vec<bool> = point_bits.chars().rev().map(|c| c == '1').collect();
                
                // Check if the prefix matches
                for (bit_idx, &prefix_bit) in prefix.iter().enumerate() {
                    if bit_idx >= point_binary.len() || point_binary[bit_idx] != prefix_bit {
                        matches_prefix = false;
                        break;
                    }
                }
                
                if !matches_prefix {
                    break;
                }
            }
            
            if matches_prefix {
                count_for_this_prefix += 1;
            }
        }
        
        if count_for_this_prefix >= cli_config.protocol.threshold as usize {
            verified_count += 1;
            println!("Prefix matches {} client points (≥ threshold of {}): ✓", 
                count_for_this_prefix, cli_config.protocol.threshold);
        } else {
            println!("Prefix matches {} client points (< threshold of {}): ✗", 
                count_for_this_prefix, cli_config.protocol.threshold);
        }
    }
    
    println!("Verification: {}/{} prefixes correctly exceed threshold", 
        verified_count, heavy_hitter_prefixes.len());

    Ok(())
}

/// Generate a sample configuration file
fn generate_config(output_path: &str) -> Result<(), String> {
    let sample_config = CliConfig {
        data_file: "data/synthetic/clusters.json".to_string(),
        query_file: "data/synthetic/server_query_points.json".to_string(),
        protocol: counttree::cli_config::ProtocolParameters {
            delta: 5,
            threshold: 3,
            input_bit_length: 8,
            output_bit_length: 16,
            check_output_bit_length: 20, // output_bit_length + 4 for aggregation
            dimensions: 2,
            share_method: "OKVS".to_string(), // Can also be "IntervalFSS"
            dictionary_type: "Known".to_string(), // Can also be "Unknown"
            threshold_method: "GarbledCircuits".to_string(), // Can also be "IntervalFSS"
            okvs: Some(counttree::cli_config::OkvsConfig {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            }),
        },
        network: counttree::cli_config::NetworkConfig {
            server0_addr: "127.0.0.1".to_string(),
            server1_addr: "127.0.0.1".to_string(),
            server0_port: 8000,
            server1_port: 8001,
        },
        output: counttree::cli_config::OutputConfig {
            verbose: true,
            show_intermediate: false,
            output_file: Some("results.json".to_string()),
        },
    };
    
    sample_config.to_file(output_path)?;
    println!("Sample configuration written to {}", output_path);
    Ok(())
}

fn main() {
    let matches = App::new("Fuzzy Heavy Hitters CLI")
        .version("1.0")
        .about("CLI for running the fuzzy heavy hitters protocol locally")
        .subcommand(
            SubCommand::with_name("run-local")
                .about("Run both servers locally for testing")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
        )
        .subcommand(
            SubCommand::with_name("generate-config")
                .about("Generate a sample configuration file")
                .arg(
                    Arg::with_name("output")
                        .short("o")
                        .long("output")
                        .value_name("FILE")
                        .help("Output configuration file path")
                        .default_value("fhh_config.json")
                )
        )
        .get_matches();

    let result = match matches.subcommand() {
        ("run-local", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_local_protocol(config_path)
        },
        ("generate-config", Some(sub_matches)) => {
            let output_path = sub_matches.value_of("output").unwrap();
            generate_config(output_path)
        },
        _ => {
            eprintln!("No subcommand specified. Use --help for usage information.");
            eprintln!("Available commands: run-local, generate-config");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
