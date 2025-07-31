//! CLI for Fuzzy Heavy Hitters Protocol
//! 
//! This CLI allows running the complete fuzzy heavy hitters protocol locally
//! with both servers in the same process for testing purposes.

use counttree::{
    fuzzy_match::{
        protocol::FuzzyHeavyHittersProtocol,
        dealer::{FssDealer, generate_fss_keys_for_threshold, generate_fss_keys_for_check},
        threshold_phase::ThresholdData,
        check_phase::{CheckData, CheckMethod},
    },
    cli_config::CliConfig,
    channel::CommTrackingChannel,
};
use scuttlebutt::AbstractChannel;
use clap::{App, Arg, SubCommand};
use std::process;
use std::thread;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::{BufReader, BufWriter};
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
    println!("List of query points: {:?}", query_points);
    
    // Create initial protocol configurations for both servers
    let protocol_config_server0 = cli_config.to_protocol_config(false)?;
    let protocol_config_server1 = cli_config.to_protocol_config(true)?;

    let protocol_server0 = FuzzyHeavyHittersProtocol::new(protocol_config_server0);
    let protocol_server1 = FuzzyHeavyHittersProtocol::new(protocol_config_server1);

    let estimated_num_check = query_points.len() * client_points.len();
    let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
    let (check_data_list_server0, check_data_list_server1) = if cli_config.protocol.check_method == "Linf" {
        let check_data_list_0 = vec![CheckData::Linf; estimated_num_check];
        let check_data_list_1 = vec![CheckData::Linf; estimated_num_check];
        (check_data_list_0, check_data_list_1)
    } else if cli_config.protocol.check_method == "LpGarbledCircuits" {
        let check_data_list_0 = vec![CheckData::LpGarbledCircuits {threshold: distance_threshold}; estimated_num_check];
        let check_data_list_1 = vec![CheckData::LpGarbledCircuits {threshold: distance_threshold}; estimated_num_check];
        (check_data_list_0, check_data_list_1)
    } else if cli_config.protocol.check_method == "LpIntervalFSS" {
        let (keys0, keys1, random_pairs) = generate_fss_keys_for_check(
            distance_threshold,
            cli_config.protocol.output_bit_length,
            cli_config.protocol.check_output_bit_length,
            estimated_num_check,
        )?;
        println!("Generated {} check FSS key pairs for LpIntervalFSS", keys0.len());
        
        // Create check data for each check using corresponding random pairs
        let mut check_data_list_0 = Vec::new();
        let mut check_data_list_1 = Vec::new();
        
        for ((fss_key_0, fss_key_1), (r0, r1)) in keys0.iter().zip(keys1.iter()).zip(random_pairs.iter()) {
            check_data_list_0.push(CheckData::LpIntervalFSS {
                threshold: distance_threshold,
                fss_key: fss_key_0.clone(),
                random_value: *r0,
            });
            check_data_list_1.push(CheckData::LpIntervalFSS {
                threshold: distance_threshold,
                fss_key: fss_key_1.clone(),
                random_value: *r1,
            });
        }
        
        (check_data_list_0, check_data_list_1)
    } else {
        return Err(format!("Unsupported check method: {}", cli_config.protocol.check_method));
    };

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

    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol_server0.generate_client_shares(&client_points)?;
    
    println!("Starting protocol execution...");
    println!("Using {} threshold method", cli_config.protocol.threshold_method);
    println!("Threshold: {}, Delta: {}", cli_config.protocol.threshold, cli_config.protocol.delta);
    println!();
    
    // Create TCP socket pair for communication (using localhost)
    let port = 9000u16; // Choose a port for local communication
    let server_addr = SocketAddr::from(([127, 0, 0, 1], port));
    
    let listener = TcpListener::bind(server_addr)
        .map_err(|e| format!("Failed to bind TCP listener: {}", e))?;
    
    let stream2 = TcpStream::connect(server_addr)
        .map_err(|e| format!("Failed to connect to TCP server: {}", e))?;
    
    let (stream1, _) = listener.accept()
        .map_err(|e| format!("Failed to accept TCP connection: {}", e))?;
    
    // Convert to CommTrackingChannel
    let channel1 = {
        stream1.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(stream1.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(stream1);
        CommTrackingChannel::new(reader, writer)
    };
    
    let channel2 = {
        stream2.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(stream2.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(stream2);
        CommTrackingChannel::new(reader, writer)
    };
    
    // Run server 1 in a separate thread
    let protocol_server1_clone = protocol_server1.clone();
    let shares_server1_clone = shares_server1.clone();
    let query_points_clone = query_points.clone();
    let threshold_data_list_server1_clone = threshold_data_list_server1.clone();
    let check_data_list_server1_clone = check_data_list_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1_clone.run_server_known_dictionary(
            1,
            &shares_server1_clone,
            &query_points_clone,
            &threshold_data_list_server1_clone,
            &check_data_list_server1_clone,
            channel1,
        )
    });
    
    // Run server 0 in main thread
    let final_results = protocol_server0.run_server_known_dictionary(
        0,
        &shares_server0,
        &query_points,
        &threshold_data_list_server0,
        &check_data_list_server0,
        channel2,
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
    
    // Initialize FSS dealer for check phase if using LpIntervalFSS
    let (fss_dealer_receiver_server0, fss_dealer_receiver_server1, fss_dealer_signal_sender_server0, fss_dealer_signal_sender_server1) = if cli_config.protocol.check_method == "LpIntervalFSS" {
        println!("Starting FSS dealer for check phase...");
        let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
        let dealer = FssDealer::new(
            client_points.len(),
            distance_threshold,
            cli_config.protocol.output_bit_length,
            cli_config.protocol.check_output_bit_length,
        )?;
        
        println!("FSS dealer started and ready");
        
        let (receiver_server0, receiver_server1, signal_sender_server0, signal_sender_server1) = dealer.into_channels();
        (Some(receiver_server0), Some(receiver_server1), Some(signal_sender_server0), Some(signal_sender_server1))
    } else {
        (None, None, None, None)
    };
    
    println!("Starting unknown dictionary search...");
    println!("Using {} threshold method", cli_config.protocol.threshold_method);
    println!("Threshold: {}, Delta: {}", cli_config.protocol.threshold, cli_config.protocol.delta);
    println!();
    
    // Create TCP socket pair for communication (using localhost)
    let port2 = 9001u16; // Choose a different port for second communication
    let server_addr2 = SocketAddr::from(([127, 0, 0, 1], port2));
    
    let listener2 = TcpListener::bind(server_addr2)
        .map_err(|e| format!("Failed to bind TCP listener: {}", e))?;
    
    let stream_b = TcpStream::connect(server_addr2)
        .map_err(|e| format!("Failed to connect to TCP server: {}", e))?;
    
    let (stream_a, _) = listener2.accept()
        .map_err(|e| format!("Failed to accept TCP connection: {}", e))?;
    
    // Convert to CommTrackingChannel
    let channel_a = {
        stream_a.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(stream_a.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(stream_a);
        CommTrackingChannel::new(reader, writer)
    };
    
    let channel_b = {
        stream_b.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(stream_b.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(stream_b);
        CommTrackingChannel::new(reader, writer)
    };
    
    // Run server 1 in a separate thread
    let shares_server1_clone = shares_server1.clone();
    let threshold_data_list_server1_clone = threshold_data_list_server1.clone();
    
    let server1_handle = thread::spawn(move || {
        protocol_server1.run_server_unknown_dictionary(
            &shares_server1_clone,
            &threshold_data_list_server1_clone,
            channel_a,
            true, // is_server1 = true
            fss_dealer_receiver_server1, // Server 1 gets its own FSS dealer receiver
            fss_dealer_signal_sender_server1, // Server 1 can signal the dealer
        )
    });
    
    // Run server 0 in main thread
    let heavy_hitter_values = protocol_server0.run_server_unknown_dictionary(
        &shares_server0,
        &threshold_data_list_server0,
        channel_b,
        false, // is_server1 = false
        fss_dealer_receiver_server0, // Server 0 gets its own FSS dealer receiver
        fss_dealer_signal_sender_server0, // Server 0 can signal the dealer
    )?;
    
    // Wait for server 1 to complete (both servers should return the same results)
    let _result_server1 = server1_handle.join()
        .map_err(|e| format!("Server 1 thread failed: {:?}", e))??;
    
    // Display results
    display_unknown_dictionary_results(&cli_config, &client_points, &heavy_hitter_values)?;
    
    Ok(())
}

/// Calculate distance between two points based on the distance metric
fn calculate_distance(point1: &[u128], point2: &[u128], distance_metric: &str) -> u128 {
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

/// Get the distance threshold for comparison based on the distance metric
fn get_distance_threshold(delta: u128, distance_metric: &str) -> u128 {
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
    println!("Using distance metric: {}", cli_config.protocol.distance_metric);
    let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
    let mut expected_heavy_hitters = 0;
    
    for (i, query_point) in query_points.iter().enumerate() {
        let mut count = 0;
        
        // Count client points within delta distance of this query point
        for client_point in client_points {
            let distance = calculate_distance(query_point, client_point, &cli_config.protocol.distance_metric);
            
            if distance <= distance_threshold {
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
        let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
        let count = client_points.iter().filter(|client_point| {
            let distance = calculate_distance(&query_points[*i], client_point, &cli_config.protocol.distance_metric);
            distance <= distance_threshold
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
    heavy_hitter_values: &[Vec<u128>],
) -> Result<(), String> {
    println!("=== Unknown Dictionary Search Results ===");
    println!("Found {} heavy hitter values", heavy_hitter_values.len());
    
    // Display the heavy hitter values
    for (i, value_set) in heavy_hitter_values.iter().enumerate() {
        println!("\nHeavy Hitter Value {}: {:?}", i + 1, value_set);
    }
    
    // Manual verification: run brute-force search on plaintext to find all fuzzy heavy hitters
    println!("\n=== Manual Verification (Brute-Force Ground Truth) ===");
    
    // Find all actual fuzzy heavy hitters by brute-force search
    let actual_heavy_hitters = find_fuzzy_heavy_hitters_bruteforce(
        client_points, 
        cli_config.protocol.delta as u128, 
        cli_config.protocol.threshold as usize,
        cli_config.protocol.input_bit_length,
        &cli_config.protocol.distance_metric,
    );
    
    println!("Brute-force found {} actual fuzzy heavy hitters:", actual_heavy_hitters.len());
    // for (i, heavy_hitter) in actual_heavy_hitters.iter().enumerate() {
    //     println!("  {}: {:?}", i + 1, heavy_hitter);
    // }
    
    // Compare protocol results with ground truth
    println!("\n=== Protocol vs Ground Truth Comparison ===");
    
    let mut protocol_correct = 0;
    let mut protocol_false_positives = 0;
    
    // Check each protocol result
    for heavy_hitter in heavy_hitter_values {
        let is_actual_heavy_hitter = actual_heavy_hitters.iter().any(|actual| {
            // Check if this protocol result matches any actual heavy hitter
            actual.len() == heavy_hitter.len() && 
            actual.iter().zip(heavy_hitter.iter()).all(|(a, b)| a == b)
        });
        
        if is_actual_heavy_hitter {
            protocol_correct += 1;
            println!("Protocol result {:?}: ✓ (correctly identified)", heavy_hitter);
        } else {
            protocol_false_positives += 1;
            println!("Protocol result {:?}: ✗ (false positive)", heavy_hitter);
        }
    }
    
    // Check for missed heavy hitters
    let mut missed_heavy_hitters = 0;
    for actual_hh in &actual_heavy_hitters {
        let found_by_protocol = heavy_hitter_values.iter().any(|protocol_hh| {
            actual_hh.len() == protocol_hh.len() && 
            actual_hh.iter().zip(protocol_hh.iter()).all(|(a, b)| a == b)
        });
        
        if !found_by_protocol {
            missed_heavy_hitters += 1;
            // println!("Missed heavy hitter: {:?} (false negative)", actual_hh);
        }
    }
    
    println!("\n=== Final Verification Summary ===");
    println!("Protocol found: {} heavy hitters", heavy_hitter_values.len());
    println!("Ground truth: {} heavy hitters", actual_heavy_hitters.len());
    println!("Correctly identified: {} / {}", protocol_correct, actual_heavy_hitters.len());
    println!("False positives: {}", protocol_false_positives);
    println!("False negatives: {}", missed_heavy_hitters);
    
    let precision = if heavy_hitter_values.is_empty() { 
        1.0 
    } else { 
        protocol_correct as f64 / heavy_hitter_values.len() as f64 
    };
    let recall = if actual_heavy_hitters.is_empty() { 
        1.0 
    } else { 
        protocol_correct as f64 / actual_heavy_hitters.len() as f64 
    };
    
    println!("Precision: {:.1}%", precision * 100.0);
    println!("Recall: {:.1}%", recall * 100.0);

    Ok(())
}

/// Brute-force search to find all fuzzy heavy hitters in plaintext
/// This serves as ground truth for verification
fn find_fuzzy_heavy_hitters_bruteforce(
    client_points: &[Vec<u128>],
    delta: u128,
    threshold: usize,
    input_bit_length: usize,
    distance_metric: &str,
) -> Vec<Vec<u128>> {
    if client_points.is_empty() {
        return Vec::new();
    }
    
    let dimensions = client_points[0].len();
    let max_value = (1u128 << input_bit_length) - 1;
    let total_space_size = (1u128 << input_bit_length).pow(dimensions as u32);
    
    println!("Running brute-force search over {}^{} = {} possible points...", 
        1u128 << input_bit_length, dimensions, total_space_size);
    
    // Use prefix search if space is too large (> 1M points), otherwise brute force
    if total_space_size > 1_000_000_000 {
        println!("Space too large, using prefix-based search for efficiency...");
        find_heavy_hitters_prefix_search(client_points, delta, threshold, input_bit_length, distance_metric)
    } else {
        println!("Using full brute-force search...");
        let brute_force_result = find_heavy_hitters_full_search(client_points, delta, threshold, max_value, dimensions, distance_metric);
        
        println!("Also running prefix search for comparison...");
        let prefix_result = find_heavy_hitters_prefix_search(client_points, delta, threshold, input_bit_length, distance_metric);
            
        println!("Brute force found {} heavy hitters", brute_force_result.len());
        println!("Prefix search found {} heavy hitters", prefix_result.len());
            
        // Check if results match
        let mut matches = 0;
        for bf_hh in &brute_force_result {
            if prefix_result.iter().any(|pr_hh| bf_hh == pr_hh) {
                matches += 1;
            }
        }
        println!("Methods agree on {} out of {} heavy hitters", matches, brute_force_result.len().max(prefix_result.len()));
            
        if brute_force_result.len() != prefix_result.len() || matches != brute_force_result.len() {
            println!("WARNING: Brute force and prefix search results differ!");
        }
        
        brute_force_result
    }
}

/// Full brute-force search over all possible points
fn find_heavy_hitters_full_search(
    client_points: &[Vec<u128>],
    delta: u128,
    threshold: usize,
    max_value: u128,
    dimensions: usize,
    distance_metric: &str,
) -> Vec<Vec<u128>> {
    let mut heavy_hitters = Vec::new();
    let mut current_point = vec![0u128; dimensions];
    let mut points_checked = 0;
    
    loop {
        // Count how many client points are within delta distance of current_point
        let mut count = 0;
        let distance_threshold = get_distance_threshold(delta, distance_metric);
        for client_point in client_points {
            let distance = calculate_distance(&current_point, client_point, distance_metric);
            
            if distance <= distance_threshold {
                count += 1;
            }
        }
        
        // If count meets threshold, this is a heavy hitter
        if count >= threshold {
            heavy_hitters.push(current_point.clone());
        }
        
        points_checked += 1;
        if points_checked % 100_000 == 0 {
            println!("  Checked {} points, found {} heavy hitters so far...", points_checked, heavy_hitters.len());
        }
        
        // Generate next point (increment like a counter in base (max_value + 1))
        let mut carry = 1;
        for dim in 0..dimensions {
            current_point[dim] += carry;
            if current_point[dim] > max_value {
                current_point[dim] = 0;
                carry = 1;
            } else {
                carry = 0;
                break;
            }
        }
        
        // If we've wrapped around all dimensions, we're done
        if carry == 1 {
            break;
        }
    }
    
    println!("Full search completed. Checked {} points total.", points_checked);
    heavy_hitters
}

/// Prefix-based search for large input spaces
/// Uses a branch-and-bound approach: if a prefix can't possibly be a heavy hitter,
/// don't explore its extensions
fn find_heavy_hitters_prefix_search(
    client_points: &[Vec<u128>],
    delta: u128,
    threshold: usize,
    input_bit_length: usize,
    distance_metric: &str,
) -> Vec<Vec<u128>> {
    let dimensions = client_points[0].len();
    let mut heavy_hitters = Vec::new();
    let mut prefixes_to_explore = vec![vec![vec![]; dimensions]]; // Start with empty prefixes
    let mut prefixes_checked = 0;
    
    while !prefixes_to_explore.is_empty() {
        let mut next_prefixes = Vec::new();
        
        for prefix_set in prefixes_to_explore {
            prefixes_checked += 1;
            if prefixes_checked % 10_000 == 0 {
                println!("  Checked {} prefixes, found {} heavy hitters so far...", 
                    prefixes_checked, heavy_hitters.len());
            }
            
            // Check if this prefix set represents complete points
            let is_complete = prefix_set.iter().all(|prefix| prefix.len() == input_bit_length);
            
            if is_complete {
                // Convert prefix to actual point and check if it's a heavy hitter
                let point: Vec<u128> = prefix_set.iter().map(|prefix| {
                    // Convert from bit vector to u128 (MSB first, like brute-force search)
                    prefix.iter().enumerate().fold(0u128, |acc, (i, &bit)| {
                        if bit { acc | (1u128 << (input_bit_length - 1 - i)) } else { acc }
                    })
                }).collect();
                
                let count = count_nearby_points(client_points, &point, delta);
                if count >= threshold {
                    heavy_hitters.push(point);
                }
            } else {
                // Get upper bound estimate for this prefix
                let upper_bound = estimate_max_nearby_count_for_prefix(client_points, &prefix_set, delta, input_bit_length);
                
                if upper_bound >= threshold {
                    // This prefix might lead to heavy hitters, so extend it
                    // Find the first dimension that can be extended
                    for dim in 0..dimensions {
                        if prefix_set[dim].len() < input_bit_length {
                            // Try extending with both 0 and 1
                            for bit_value in [false, true] {
                                let mut extended_prefix = prefix_set.clone();
                                extended_prefix[dim].push(bit_value);
                                next_prefixes.push(extended_prefix);
                            }
                            break; // Only extend one dimension at a time
                        }
                    }
                }
                // If upper_bound < threshold, we prune this branch
            }
        }
        
        prefixes_to_explore = next_prefixes;
    }
    
    println!("Prefix search completed. Checked {} prefixes total.", prefixes_checked);
    heavy_hitters
}

/// Count how many client points are within delta distance of a given point
fn count_nearby_points(client_points: &[Vec<u128>], point: &[u128], delta: u128) -> usize {
    client_points.iter().filter(|client_point| {
        let mut l_inf_distance = 0;
        for dim in 0..point.len() {
            let diff = if point[dim] > client_point[dim] {
                point[dim] - client_point[dim]
            } else {
                client_point[dim] - point[dim]
            };
            l_inf_distance = l_inf_distance.max(diff);
        }
        l_inf_distance <= delta
    }).count()
}

/// Estimate the maximum number of nearby points for any complete point that extends this prefix
/// This provides an upper bound for pruning
fn estimate_max_nearby_count_for_prefix(
    client_points: &[Vec<u128>],
    prefix_set: &[Vec<bool>],
    delta: u128,
    input_bit_length: usize,
) -> usize {
    // For each client point, check if it could possibly be within delta of some extension of this prefix
    client_points.iter().filter(|client_point| {
        // For each dimension, check if the client point coordinate could be within delta
        // of some value that extends the current prefix
        prefix_set.iter().enumerate().all(|(dim, prefix)| {
            if dim >= client_point.len() {
                return true;
            }
            
            // Convert current prefix to min and max possible values (MSB first)
            let prefix_value = prefix.iter().enumerate().fold(0u128, |acc, (i, &bit)| {
                if bit { acc | (1u128 << (input_bit_length - 1 - i)) } else { acc }
            });
            
            let remaining_bits = input_bit_length - prefix.len();
            let min_possible = prefix_value;
            let max_possible = prefix_value | ((1u128 << remaining_bits) - 1);
            
            // Check if client point could be within delta of the range [min_possible, max_possible]
            let client_coord = client_point[dim];
            
            // Check if there's any overlap between [client_coord - delta, client_coord + delta] 
            // and [min_possible, max_possible]
            let client_min = if client_coord >= delta {
                client_coord - delta
            } else {
                0 // Can't go below 0
            };
            let client_max = if client_coord + delta <= (1u128 << input_bit_length) - 1 {
                client_coord + delta
            } else {
                (1u128 << input_bit_length) - 1 // Can't exceed max value
            };

            !(client_max < min_possible || client_min > max_possible)
        })
    }).count()
}

/// Generate a sample configuration file
fn generate_config(output_path: &str) -> Result<(), String> {
    let sample_config = CliConfig {
        data_file: "data/synthetic/clusters.json".to_string(),
        query_file: "data/synthetic/server_query_points.json".to_string(),
        protocol: counttree::cli_config::ProtocolParameters {
            delta: 5,
            threshold: 3,
            input_bit_length: 10,
            output_bit_length: 16,
            check_output_bit_length: 20, // output_bit_length + 4 for aggregation
            dimensions: 2,
            share_method: "OKVS".to_string(), // Can also be "IntervalFSS"
            dictionary_type: "Known".to_string(), // Can also be "Unknown"
            threshold_method: "GarbledCircuits".to_string(), // Can also be "IntervalFSS"
            check_method: "Linf".to_string(), // Can also be "LpGarbledCircuits" or "LpIntervalFSS"
            distance_metric: "Linf".to_string(), // Can also be "L1", "L2", "L3"
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
            dealer_addr: "127.0.0.1".to_string(),
            dealer_to_server0_port: 9000,
            dealer_to_server1_port: 9001,
            client_addr: "127.0.0.1".to_string(),
            client_to_server0_port: 7000,
            client_to_server1_port: 7001,
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

/// Run as dealer - generates and distributes FSS keys to servers
fn run_dealer(config_path: &str) -> Result<(), String> {
    println!("Starting FSS Dealer...");
    let cli_config = CliConfig::from_file(config_path)?;
    
    use counttree::fuzzy_match::dealer::NetworkedFssDealer;
    
    // For dealer, we need to estimate the number of clients
    // This would typically be provided as a parameter or read from data
    let estimated_clients = 1000; // Default estimate, could be made configurable
    
    let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
    
    let dealer = NetworkedFssDealer::new(
        cli_config.network.server0_addr,
        cli_config.network.dealer_to_server0_port,
        cli_config.network.server1_addr,
        cli_config.network.dealer_to_server1_port,
        distance_threshold,
        cli_config.protocol.output_bit_length,
        cli_config.protocol.check_output_bit_length,
    );
    
    dealer.run(estimated_clients)
}

/// Run as client - generates shares and sends them to servers
fn run_client(config_path: &str) -> Result<(), String> {
    println!("Starting Client...");
    let cli_config = CliConfig::from_file(config_path)?;
    
    // Load client data
    println!("Loading client data from {}", cli_config.data_file);
    let clusters = load_clusters(&cli_config.data_file)?;
    
    // Extract client points from clusters
    let mut client_points = Vec::new();
    for cluster in &clusters {
        for point in &cluster.points {
            client_points.push(point.clone());
        }
    }
    
    println!("Loaded {} client points from {} clusters", client_points.len(), clusters.len());
    
    // Create protocol configuration
    let protocol_config = cli_config.to_protocol_config(false)?; // Client uses server0 config
    let protocol = FuzzyHeavyHittersProtocol::new(protocol_config);
    
    // Generate client shares
    println!("Generating client shares...");
    let (shares_server0, shares_server1) = protocol.generate_client_shares(&client_points)?;
    
    // Connect to both servers and send shares
    use std::net::TcpStream;
    use std::io::{BufReader, BufWriter};
    
    println!("Connecting to servers...");
    let server0_stream = TcpStream::connect((cli_config.network.server0_addr.as_str(), cli_config.network.client_to_server0_port))
        .map_err(|e| format!("Failed to connect to server 0: {}", e))?;
    
    let server1_stream = TcpStream::connect((cli_config.network.server1_addr.as_str(), cli_config.network.client_to_server1_port))
        .map_err(|e| format!("Failed to connect to server 1: {}", e))?;
    
    // Send shares to server 0
    let mut channel_server0 = {
        server0_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(server0_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(server0_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    // Send shares to server 1
    let mut channel_server1 = {
        server1_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(server1_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(server1_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    println!("Sending shares to servers...");
    // Send shares using binary serialization
    let shares0_data = bincode::serialize(&shares_server0)
        .map_err(|e| format!("Failed to serialize shares for server 0: {}", e))?;
    let shares1_data = bincode::serialize(&shares_server1)
        .map_err(|e| format!("Failed to serialize shares for server 1: {}", e))?;
    
    // Send to server 0
    let len_bytes = (shares0_data.len() as u64).to_le_bytes();
    channel_server0.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to send length to server 0: {}", e))?;
    channel_server0.write_bytes(&shares0_data)
        .map_err(|e| format!("Failed to send shares to server 0: {}", e))?;
    channel_server0.flush()
        .map_err(|e| format!("Failed to flush to server 0: {}", e))?;
    
    // Send to server 1
    let len_bytes = (shares1_data.len() as u64).to_le_bytes();
    channel_server1.write_bytes(&len_bytes)
        .map_err(|e| format!("Failed to send length to server 1: {}", e))?;
    channel_server1.write_bytes(&shares1_data)
        .map_err(|e| format!("Failed to send shares to server 1: {}", e))?;
    channel_server1.flush()
        .map_err(|e| format!("Failed to flush to server 1: {}", e))?;
    
    println!("Client shares sent successfully");
    Ok(())
}

/// Run as server 0
fn run_server0(config_path: &str) -> Result<(), String> {
    println!("Starting Server 0...");
    let cli_config = CliConfig::from_file(config_path)?;
    
    run_server(config_path, false)
}

/// Run as server 1  
fn run_server1(config_path: &str) -> Result<(), String> {
    println!("Starting Server 1...");
    let cli_config = CliConfig::from_file(config_path)?;
    
    run_server(config_path, true)
}

/// Common server implementation
fn run_server(config_path: &str, is_server1: bool) -> Result<(), String> {
    let cli_config = CliConfig::from_file(config_path)?;
    let server_id = if is_server1 { 1 } else { 0 };
    
    println!("Server {} starting...", server_id);
    
    // Create protocol configuration
    let protocol_config = cli_config.to_protocol_config(is_server1)?;
    let protocol = FuzzyHeavyHittersProtocol::new(protocol_config);
    
    use std::net::TcpListener;
    use std::io::{BufReader, BufWriter};
    
    // Set up listeners for client and dealer connections
    let client_port = if is_server1 { 
        cli_config.network.client_to_server1_port 
    } else { 
        cli_config.network.client_to_server0_port 
    };
    
    let dealer_port = if is_server1 {
        cli_config.network.dealer_to_server1_port
    } else {
        cli_config.network.dealer_to_server0_port
    };
    
    let other_server_port = if is_server1 {
        cli_config.network.server0_port
    } else {
        cli_config.network.server1_port
    };
    
    let other_server_addr = if is_server1 {
        &cli_config.network.server0_addr
    } else {
        &cli_config.network.server1_addr
    };
    
    // Listen for client connection
    println!("Server {}: Waiting for client connection on port {}", server_id, client_port);
    let client_listener = TcpListener::bind(("0.0.0.0", client_port))
        .map_err(|e| format!("Failed to bind client listener: {}", e))?;
    
    let (client_stream, _) = client_listener.accept()
        .map_err(|e| format!("Failed to accept client connection: {}", e))?;
    
    println!("Server {}: Client connected", server_id);
    
    // Receive shares from client
    let mut client_channel = {
        client_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(client_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(client_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    println!("Server {}: Receiving shares from client...", server_id);
    // Receive shares using binary deserialization
    let mut len_bytes = [0u8; 8];
    client_channel.read_bytes(&mut len_bytes)
        .map_err(|e| format!("Failed to read length from client: {}", e))?;
    let len = u64::from_le_bytes(len_bytes) as usize;
    
    let mut shares_data = vec![0u8; len];
    client_channel.read_bytes(&mut shares_data)
        .map_err(|e| format!("Failed to receive shares from client: {}", e))?;
    
    let shares: Vec<Vec<u128>> = bincode::deserialize(&shares_data)
        .map_err(|e| format!("Failed to deserialize shares: {}", e))?;
    
    println!("Server {}: Received {} shares from client", server_id, shares.len());
    
    // Listen for dealer connection
    println!("Server {}: Waiting for dealer connection on port {}", server_id, dealer_port);
    let dealer_listener = TcpListener::bind(("0.0.0.0", dealer_port))
        .map_err(|e| format!("Failed to bind dealer listener: {}", e))?;
    
    let (dealer_stream, _) = dealer_listener.accept()
        .map_err(|e| format!("Failed to accept dealer connection: {}", e))?;
    
    println!("Server {}: Dealer connected", server_id);
    
    // Set up dealer channel
    let mut dealer_channel = {
        dealer_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(dealer_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(dealer_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    // Connect to other server
    println!("Server {}: Connecting to other server at {}:{}", server_id, other_server_addr, other_server_port);
    let other_server_stream = std::net::TcpStream::connect((other_server_addr.as_str(), other_server_port))
        .map_err(|e| format!("Failed to connect to other server: {}", e))?;
    
    let mut other_server_channel = {
        other_server_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(other_server_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(other_server_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    println!("Server {}: Connected to other server", server_id);
    
    // TODO: Implement the actual protocol execution
    // This would depend on whether it's known or unknown dictionary protocol
    // and would use the shares, dealer_channel, and other_server_channel
    
    match cli_config.protocol.dictionary_type.as_str() {
        "Known" => {
            // Load query points for known dictionary
            println!("Server {}: Loading query points from {}", server_id, cli_config.query_file);
            let query_points = load_query_points(&cli_config.query_file)?;
            
            // TODO: Generate threshold and check data based on configuration
            // This is a simplified placeholder - actual implementation would need
            // to coordinate with dealer for FSS keys if using IntervalFSS methods
            
            println!("Server {}: Running known dictionary protocol...", server_id);
            // The actual protocol call would go here
        }
        "Unknown" => {
            println!("Server {}: Running unknown dictionary protocol...", server_id);
            // The actual protocol call would go here
        }
        other => return Err(format!("Unsupported dictionary type: {}", other)),
    }
    
    println!("Server {}: Protocol execution completed", server_id);
    Ok(())
}

fn main() {
    let matches = App::new("Fuzzy Heavy Hitters CLI")
        .version("1.0")
        .about("CLI for running the fuzzy heavy hitters protocol in distributed or local mode")
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
            SubCommand::with_name("dealer")
                .about("Run as FSS dealer (generates and distributes keys)")
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
            SubCommand::with_name("client")
                .about("Run as client (generates and sends shares)")
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
            SubCommand::with_name("server0")
                .about("Run as server 0")
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
            SubCommand::with_name("server1")
                .about("Run as server 1")
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
        ("dealer", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_dealer(config_path)
        },
        ("client", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_client(config_path)
        },
        ("server0", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_server0(config_path)
        },
        ("server1", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_server1(config_path)
        },
        ("generate-config", Some(sub_matches)) => {
            let output_path = sub_matches.value_of("output").unwrap();
            generate_config(output_path)
        },
        _ => {
            eprintln!("No subcommand specified. Use --help for usage information.");
            eprintln!("Available commands:");
            eprintln!("  run-local     - Run both servers locally for testing");
            eprintln!("  dealer        - Run as FSS dealer");
            eprintln!("  client        - Run as client");
            eprintln!("  server0       - Run as server 0");
            eprintln!("  server1       - Run as server 1");
            eprintln!("  generate-config - Generate sample configuration");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
