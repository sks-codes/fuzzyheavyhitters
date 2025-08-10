//! CLI for Fuzzy Heavy Hitters Protocol
//! 
//! This CLI allows running the complete fuzzy heavy hitters protocol locally
//! with both servers in the same process for testing purposes.

use counttree::{
    fuzzy_match::{
        protocol::FuzzyHeavyHittersProtocol,
        dealer::FssDealer,
        threshold_phase::ThresholdData,
        check_phase::{CheckData, CheckMethod},
        client::Client,
        share_phase::SharedRange,
    },
    cli_config::CliConfig,
    channel::CommTrackingChannel,
    util::{calculate_distance, get_distance_threshold},
};
use scuttlebutt::AbstractChannel;
use clap::{App, Arg, SubCommand};
use tarpc::server;
use std::process;
use std::thread;
use std::thread::sleep;
use std::time::Duration;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::{BufReader, BufWriter};
use std::fs;
use serde_json;
use bincode;
use std::time::Instant;
use std::sync::{Arc, Mutex};
use std::thread::available_parallelism;

/// Setup multiple TCP channels for parallel garbled circuit communication
/// Returns a vector of channels that can be used for parallel computation
fn setup_parallel_channels(
    is_connector: bool, // true if this side initiates connections
    num_channels: usize,
    target_addr: &str,
    base_port: u16,
) -> Result<Vec<Arc<Mutex<CommTrackingChannel>>>, String> {
    let mut channels = Vec::with_capacity(num_channels);

    for i in 0..num_channels {
        let port = base_port + i as u16;
        
        let stream = if is_connector {
            // Connect to the target
            println!("Connecting to port {}", port);
            thread::sleep(Duration::from_millis(100));
            let addr = format!("{}:{}", target_addr, port);
            TcpStream::connect(&addr)
                .map_err(|e| format!("Failed to connect to {}: {}", addr, e))?
        } else {
            // Listen and accept connection
            println!("Waiting on port {}", port);
            let listener = TcpListener::bind(format!("{}:{}", target_addr, port))
                .map_err(|e| format!("Failed to bind to port {}: {}", port, e))?;
            let (stream, _) = listener.accept()
                .map_err(|e| format!("Failed to accept connection on port {}: {}", port, e))?;
            stream
        };

        stream.set_nodelay(true)
            .map_err(|e| format!("Failed to set nodelay: {}", e))?;

        let reader = BufReader::new(stream.try_clone()
            .map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(stream);
        
        let channel = CommTrackingChannel::new(reader, writer);
        channels.push(Arc::new(Mutex::new(channel)));
    }

    Ok(channels)
}

/// Load client points directly from JSON file
fn load_client_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read client points file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse client points file {}: {}", file_path, e))
}

/// Load query points from JSON file
fn load_query_points(file_path: &str) -> Result<Vec<Vec<u128>>, String> {
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read query file {}: {}", file_path, e))?;
    
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse query file {}: {}", file_path, e))
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
    if total_space_size > 100_000 {
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
        data_file: "data/synthetic/client_points.json".to_string(),
        query_file: "data/synthetic/server_points.json".to_string(),
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
            num_clients: 100, // Number of clients participating in the protocol
            okvs: Some(counttree::cli_config::OkvsConfig {
                r1: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                r2: [16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            }),
        },
        network: counttree::cli_config::NetworkConfig {
            server0_addr: "127.0.0.1".to_string(),
            server1_addr: "127.0.0.1".to_string(),
            server0_to_server1_port: 8000,
            dealer_to_server0_port: 9000,
            dealer_to_server1_port: 9001,
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
fn run_dealer(config_path: &str, num_threads: usize) -> Result<(), String> {
    let start_time = Instant::now();
    println!("Starting FSS Dealer...");
    let cli_config = CliConfig::from_file(config_path)?;

    let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);

    let dealer = FssDealer::new(
        distance_threshold,
        cli_config.protocol.threshold,
        cli_config.protocol.output_bit_length,
        cli_config.protocol.check_output_bit_length,
        cli_config.protocol.num_clients,
    );

    // Determine number of parallel channels (use specified num_threads or system parallelism)
    let num_channels = num_threads;
    
    println!("Setting up {} parallel dealer channels to each server...", num_channels);

    // Set up parallel channels to server 0
    let channels_server0 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers)
        num_channels,
        &cli_config.network.server0_addr,
        cli_config.network.dealer_to_server0_port,
    )?;

    // Set up parallel channels to server 1
    let channels_server1 = setup_parallel_channels(
        true, // is_connector (dealer connects to servers) 
        num_channels,
        &cli_config.network.server1_addr,
        cli_config.network.dealer_to_server1_port,
    )?;

    println!("Connected to both servers with {} channels each", num_channels);

    let dealer_start = Instant::now();
    dealer.run_parallel(&channels_server0, &channels_server1)
        .map_err(|e| format!("Failed to run parallel dealer protocol: {}", e))?;
    let dealer_time = dealer_start.elapsed();
    
    let total_time = start_time.elapsed();
    
    // Calculate communication metrics from all channels
    let mut total_bytes_sent_0 = 0;
    let mut total_bytes_received_0 = 0;
    let mut total_bytes_sent_1 = 0;
    let mut total_bytes_received_1 = 0;
    
    for channel in &channels_server0 {
        let channel_guard = channel.lock().unwrap();
        let (sent, received) = channel_guard.get_communication_stats();
        total_bytes_sent_0 += sent;
        total_bytes_received_0 += received;
    }
    
    for channel in &channels_server1 {
        let channel_guard = channel.lock().unwrap();
        let (sent, received) = channel_guard.get_communication_stats();
        total_bytes_sent_1 += sent;
        total_bytes_received_1 += received;
    }
    
    let total_bytes = total_bytes_sent_0 + total_bytes_received_0 + total_bytes_sent_1 + total_bytes_received_1;
    
    println!("\n=== Dealer Performance Summary ===");
    println!("📊 FSS key generation and distribution time: {:.2?}", dealer_time);
    println!("📊 Total dealer time: {:.2?}", total_time);
    println!("📡 Communication with server 0 ({} channels):", num_channels);
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_bytes_sent_0, total_bytes_sent_0 as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_bytes_received_0, total_bytes_received_0 as f64 / 1024.0);
    println!("📡 Communication with server 1 ({} channels):", num_channels);
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_bytes_sent_1, total_bytes_sent_1 as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_bytes_received_1, total_bytes_received_1 as f64 / 1024.0);
    println!("📡 Total communication: {} bytes ({:.2} KB)", total_bytes, total_bytes as f64 / 1024.0);

    Ok(())
}

/// Run as client - generates shares and sends them to servers
fn run_client(config_path: &str) -> Result<(), String> {
    let start_time = Instant::now();
    println!("Starting Client...");
    let cli_config = CliConfig::from_file(config_path)?;
    
    // Load client data directly from client_points.json
    println!("Loading client data from {}", cli_config.data_file);
    let client_points = load_client_points(&cli_config.data_file)?;
    
    println!("Loaded {} client points", client_points.len());
    
    // Generate client shares
    let share_start = Instant::now();
    let share_config = cli_config.to_share_config()?; // Client uses share config
    let client = Client::new(share_config);
    let (shares_server0, shares_server1) = client.generate_client_shares(&client_points, cli_config.protocol.delta)?;
    let share_time = share_start.elapsed();

    println!("Connecting to servers...");
    let server0_stream = TcpStream::connect((cli_config.network.server0_addr.as_str(), cli_config.network.client_to_server0_port))
        .map_err(|e| format!("Failed to connect to server 0: {}", e))?;
    
    let server1_stream = TcpStream::connect((cli_config.network.server1_addr.as_str(), cli_config.network.client_to_server1_port))
        .map_err(|e| format!("Failed to connect to server 1: {}", e))?;
    
    // Create communication channels for both servers
    let mut channel_server0 = {
        server0_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(server0_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(server0_stream);
        CommTrackingChannel::new(reader, writer)
    };
    let mut channel_server1 = {
        server1_stream.set_nodelay(true).map_err(|e| format!("Failed to set nodelay: {}", e))?;
        let reader = BufReader::new(server1_stream.try_clone().map_err(|e| format!("Failed to clone stream: {}", e))?);
        let writer = BufWriter::new(server1_stream);
        CommTrackingChannel::new(reader, writer)
    };
    
    // Send shares to both servers
    println!("Sending shares to servers...");
    let send_start = Instant::now();
    client.send_client_shares(
        shares_server0, 
        shares_server1, 
        &mut channel_server0, 
        &mut channel_server1)?;
    let send_time = send_start.elapsed();
    
    let total_time = start_time.elapsed();
    
    // Calculate communication metrics
    let (bytes_sent_0, _) = channel_server0.get_communication_stats();
    let (bytes_sent_1, _) = channel_server1.get_communication_stats();
    let total_bytes_sent = bytes_sent_0 + bytes_sent_1;
    
    println!("Client shares sent successfully");
    
    println!("\n=== Client Performance Summary ===");
    println!("📊 Share generation time: {:.2?}", share_time);
    println!("📊 Share transmission time: {:.2?}", send_time);
    println!("📊 Total client time: {:.2?}", total_time);
    println!("📡 Bytes sent to server 0: {} bytes", bytes_sent_0);
    println!("📡 Bytes sent to server 1: {} bytes", bytes_sent_1);
    println!("📡 Total bytes sent: {} bytes ({:.2} KB)", total_bytes_sent, total_bytes_sent as f64 / 1024.0);
    
    Ok(())
}

/// Run server for both known and unknown dictionary cases
fn run_server(config_path: &str, is_server1: bool, num_threads: usize) -> Result<(), String> {
    let start_time = Instant::now();
    let cli_config = CliConfig::from_file(config_path)?;
    let server_id = if is_server1 { 1 } else { 0 };
    let is_known_dictionary = cli_config.protocol.dictionary_type == "Known";
    
    println!("Server {} starting {} dictionary protocol with {} parallel threads...", 
             server_id, 
             if is_known_dictionary { "known" } else { "unknown" },
             num_threads);

    let server_addr = if is_server1 {
        cli_config.network.server1_addr.clone()
    } else {
        cli_config.network.server0_addr.clone()
    };
    
    // Create protocol configuration
    let protocol_config = cli_config.to_protocol_config(is_server1)?;
    let protocol = FuzzyHeavyHittersProtocol::new(protocol_config, is_server1);
    
    let client_to_server_port = if is_server1 { 
        cli_config.network.client_to_server1_port 
    } else { 
        cli_config.network.client_to_server0_port 
    };

    // Listen for client connection
    println!("Server {}: Waiting for client connection on port {}", server_id, client_to_server_port);
    let client_listener = TcpListener::bind((server_addr.clone(), client_to_server_port))
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
    let shares = protocol.receive_client_shares(&mut client_channel)
        .map_err(|e| format!("Failed to receive client shares: {}", e))?;
    println!("Server {}: Received {} shares from client", server_id, shares.len());

    // Determine number of parallel channels (use same as num_threads or system parallelism)
    let num_dealer_channels = num_threads;

    // Set up multiple dealer channels
    println!("Server {}: Setting up {} dealer channels...", server_id, num_dealer_channels);
    let dealer_to_server_port = if is_server1 {
        cli_config.network.dealer_to_server1_port
    } else {
        cli_config.network.dealer_to_server0_port
    };
    
    let dealer_channels = setup_parallel_channels(
        false, // is_connector (server listens for dealer connections)
        num_dealer_channels,
        &server_addr, // listen on all interfaces
        dealer_to_server_port,
    )?;
    
    println!("Server {}: Successfully established {} dealer channels", server_id, dealer_channels.len());

    // Set up parallel server-to-server communication channels
    let server0_addr = &cli_config.network.server0_addr;
    let server0_to_server1_port = cli_config.network.server0_to_server1_port;
    
    let other_server_channels = {
        println!("Server {}: Setting up {} inter-server channels...", server_id, num_threads);
        
        let channels = if is_server1 {
            // Server 1 connects to Server 0's channels
            sleep(Duration::from_secs(1)); // Give time for server 0 to start
            setup_parallel_channels(
                true, // is_connector
                num_threads,
                server0_addr,
                server0_to_server1_port,
            )?
        } else {
            // Server 0 listens for Server 1's connections
            setup_parallel_channels(
                false, // is_connector
                num_threads,
                server0_addr, // listen on all interfaces
                server0_to_server1_port,
            )?
        };
        
        println!("Server {}: Successfully established {} inter-server channels", server_id, channels.len());
        channels
    };

    let protocol_time = if is_known_dictionary {
        // Load query points for known dictionary
        println!("Server {}: Loading query points from {}", server_id, cli_config.query_file);
        let query_points = load_query_points(&cli_config.query_file)?;
        
        // Run the protocol for known dictionary
        let protocol_start = Instant::now();
        
        println!("Server {}: Using {} dealer channels and {} server channels for parallel processing", 
                 server_id, dealer_channels.len(), other_server_channels.len());
        
        let results = protocol.run_server_known_dictionary_parallel(
            &shares,
            &query_points,
            &dealer_channels,
            &other_server_channels,
        )?;
        let protocol_time = protocol_start.elapsed();
        
        println!("Server {}: Protocol execution completed", server_id);
        println!("Results: {:?}", results);

        start_time.elapsed();
    } else {
        println!("Server {}: Running unknown dictionary protocol...", server_id);
        
        // Run the protocol for unknown dictionary
        let protocol_start = Instant::now();
        let heavy_hitters = protocol.run_server_unknown_dictionary_parallel(
            &shares,
            is_server1,
            &dealer_channels,
            &other_server_channels,
        )?;
        let protocol_time = protocol_start.elapsed();
        
        println!("Server {}: Protocol execution completed", server_id);
        println!("Found {} heavy hitters: {:?}", heavy_hitters.len(), heavy_hitters);
        
        start_time.elapsed();
    };
        
    // Calculate communication metrics from all channels
    let mut total_other_server_bytes_sent = 0;
    let mut total_other_server_bytes_received = 0;
    let mut total_dealer_bytes_sent = 0;
    let mut total_dealer_bytes_received = 0;
        
    for channel in &other_server_channels {
        let channel_guard = channel.lock().unwrap();
        let (sent, received) = channel_guard.get_communication_stats();
        total_other_server_bytes_sent += sent;
        total_other_server_bytes_received += received;
    }
        
    for channel in &dealer_channels {
        let channel_guard = channel.lock().unwrap();
        let (sent, received) = channel_guard.get_communication_stats();
        total_dealer_bytes_sent += sent;
        total_dealer_bytes_received += received;
    }
        
    println!("\n=== Server {} Performance Summary ===", server_id);
    println!("📊 Protocol execution time: {:.2?}", protocol_time);
    println!("📊 Total server time: {:.2?}", start_time.elapsed());
    println!("📡 Communication with other server ({} channels):", other_server_channels.len());
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_other_server_bytes_sent, total_other_server_bytes_sent as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_other_server_bytes_received, total_other_server_bytes_received as f64 / 1024.0);
    println!("📡 Communication with dealer ({} channels):", dealer_channels.len());
    println!("   Bytes sent: {} bytes ({:.2} KB)", total_dealer_bytes_sent, total_dealer_bytes_sent as f64 / 1024.0);
    println!("   Bytes received: {} bytes ({:.2} KB)", total_dealer_bytes_received, total_dealer_bytes_received as f64 / 1024.0);
    println!("📡 Total communication: {} bytes ({:.2} KB)", 
                total_other_server_bytes_sent + total_other_server_bytes_received + total_dealer_bytes_sent + total_dealer_bytes_received,
                (total_other_server_bytes_sent + total_other_server_bytes_received + total_dealer_bytes_sent + total_dealer_bytes_received) as f64 / 1024.0);
        
    
    Ok(())
}

/// Helper function to run as server 0
fn run_server0(config_path: &str, num_threads: usize) -> Result<(), String> {
    run_server(config_path, false, num_threads)
}

/// Helper function to run as server 1  
fn run_server1(config_path: &str, num_threads: usize) -> Result<(), String> {
    run_server(config_path, true, num_threads)
}

/// Run ground truth (non-secure plaintext) protocol for verification
fn run_ground_truth(config_path: &str) -> Result<(), String> {
    let start_time = Instant::now();
    println!("Running Ground Truth (Plaintext) Protocol...");
    
    let cli_config = CliConfig::from_file(config_path)?;
    let is_known_dictionary = cli_config.protocol.dictionary_type == "Known";
    
    // Load client data
    println!("Loading client data from {}", cli_config.data_file);
    let client_points = load_client_points(&cli_config.data_file)?;
    println!("Loaded {} client points", client_points.len());
    
    if is_known_dictionary {
        println!("\n=== Running Known Dictionary Ground Truth ===");
        
        // Load query points
        println!("Loading query points from {}", cli_config.query_file);
        let query_points = load_query_points(&cli_config.query_file)?;
        println!("Loaded {} query points", query_points.len());
        
        // Calculate ground truth results for each query point
        let mut ground_truth_results = Vec::new();
        let distance_threshold = get_distance_threshold(cli_config.protocol.delta, &cli_config.protocol.distance_metric);
        println!("Distance threshold for delta {}: {}", cli_config.protocol.delta, distance_threshold);
        
        println!("\nCalculating ground truth for each query point...");
        for (i, query_point) in query_points.iter().enumerate() {
            let mut count = 0;
            
            // Count client points within delta distance of this query point
            for client_point in &client_points {
                let distance = calculate_distance(query_point, client_point, &cli_config.protocol.distance_metric);
                
                if distance <= distance_threshold {
                    count += 1;
                }
            }
            
            let is_heavy_hitter = count >= cli_config.protocol.threshold as usize;
            ground_truth_results.push(is_heavy_hitter);
            
            if cli_config.output.verbose {
                let status = if is_heavy_hitter { "HEAVY HITTER" } else { "not heavy hitter" };
                println!("Query {}: {:?} -> {} (count: {})", 
                    i + 1, query_point, status, count);
            }
        }
        
        // Display summary
        let total_heavy_hitters = ground_truth_results.iter().filter(|&&x| x).count();
        println!("\n=== Ground Truth Results Summary ===");
        println!("Protocol: Known Dictionary");
        println!("Distance metric: {}", cli_config.protocol.distance_metric);
        println!("Delta (distance threshold): {}", cli_config.protocol.delta);
        println!("Threshold (minimum count): {}", cli_config.protocol.threshold);
        println!("Total query points: {}", query_points.len());
        println!("Heavy hitters found: {} ({:.1}%)", 
            total_heavy_hitters, 
            (total_heavy_hitters as f64 / query_points.len() as f64) * 100.0);
        
        // Save results if output file specified
        if let Some(output_file) = &cli_config.output.output_file {
            let results_data = serde_json::json!({
                "protocol_type": "known_dictionary_ground_truth",
                "config": {
                    "distance_metric": cli_config.protocol.distance_metric,
                    "delta": cli_config.protocol.delta.to_string(),
                    "threshold": cli_config.protocol.threshold.to_string(),
                    "dimensions": cli_config.protocol.dimensions.to_string(),
                },
                "results": query_points.iter().zip(ground_truth_results.iter()).enumerate().map(|(i, (query, result))| {
                    serde_json::json!({
                        "query_id": (i + 1).to_string(),
                        "query_point": query.iter().map(|&x| x.to_string()).collect::<Vec<_>>(),
                        "is_heavy_hitter": result
                    })
                }).collect::<Vec<_>>(),
                "summary": {
                    "total_queries": query_points.len().to_string(),
                    "heavy_hitters": total_heavy_hitters.to_string(),
                    "percentage": (total_heavy_hitters as f64 / query_points.len() as f64 * 100.0).to_string()
                }
            });
            
            fs::write(output_file, serde_json::to_string_pretty(&results_data)
                .map_err(|e| format!("Failed to serialize results: {}", e))?)
                .map_err(|e| format!("Failed to write results to {}: {}", output_file, e))?;
            println!("Results saved to {}", output_file);
        }
        
    } else {
        println!("\n=== Running Unknown Dictionary Ground Truth ===");
        
        // Find all fuzzy heavy hitters using brute-force search
        let ground_truth_heavy_hitters = find_fuzzy_heavy_hitters_bruteforce(
            &client_points,
            cli_config.protocol.delta,
            cli_config.protocol.threshold as usize,
            cli_config.protocol.input_bit_length,
            &cli_config.protocol.distance_metric,
        );
        
        // Display results
        println!("\n=== Ground Truth Results Summary ===");
        println!("Protocol: Unknown Dictionary");
        println!("Distance metric: {}", cli_config.protocol.distance_metric);
        println!("Delta (distance threshold): {}", cli_config.protocol.delta);
        println!("Threshold (minimum count): {}", cli_config.protocol.threshold);
        println!("Input bit length: {}", cli_config.protocol.input_bit_length);
        println!("Dimensions: {}", cli_config.protocol.dimensions);
        println!("Total client points: {}", client_points.len());
        println!("Heavy hitters found: {}", ground_truth_heavy_hitters.len());
        
        if cli_config.output.verbose {
            println!("\nHeavy hitter values:");
            for (i, heavy_hitter) in ground_truth_heavy_hitters.iter().enumerate() {
                println!("  {}: {:?}", i + 1, heavy_hitter);
            }
        }
        
        // Save results if output file specified
        if let Some(output_file) = &cli_config.output.output_file {
            let results_data = serde_json::json!({
                "protocol_type": "unknown_dictionary_ground_truth",
                "config": {
                    "distance_metric": cli_config.protocol.distance_metric,
                    "delta": cli_config.protocol.delta.to_string(),
                    "threshold": cli_config.protocol.threshold.to_string(),
                    "dimensions": cli_config.protocol.dimensions.to_string(),
                    "input_bit_length": cli_config.protocol.input_bit_length.to_string()
                },
                "heavy_hitters": ground_truth_heavy_hitters.iter().map(|hh| {
                    hh.iter().map(|&x| x.to_string()).collect::<Vec<_>>()
                }).collect::<Vec<_>>(),
                "summary": {
                    "total_client_points": client_points.len().to_string(),
                    "heavy_hitters_count": ground_truth_heavy_hitters.len().to_string(),
                }
            });
            
            fs::write(output_file, serde_json::to_string_pretty(&results_data)
                .map_err(|e| format!("Failed to serialize results: {}", e))?)
                .map_err(|e| format!("Failed to write results to {}: {}", output_file, e))?;
            println!("Results saved to {}", output_file);
        }
    }
    
    let total_time = start_time.elapsed();
    println!("\n=== Ground Truth Performance Summary ===");
    println!("📊 Total computation time: {:.2?}", total_time);
    
    Ok(())
}

fn main() {
    let matches = App::new("Fuzzy Heavy Hitters CLI")
        .version("1.0")
        .about("CLI for running the fuzzy heavy hitters protocol in distributed or local mode")
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
                .arg(
                    Arg::with_name("threads")
                        .short("t")
                        .long("threads")
                        .value_name("NUMBER")
                        .help("Number of parallel threads/channels to use")
                        .default_value("1")
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
                .about("Run as server 0 (evaluator)")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
                .arg(
                    Arg::with_name("threads")
                        .short("t")
                        .long("threads")
                        .value_name("NUMBER")
                        .help("Number of parallel threads/channels to use")
                        .default_value("1")
                )
        )
        .subcommand(
            SubCommand::with_name("server1")
                .about("Run as server 1 (garbler)")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
                .arg(
                    Arg::with_name("threads")
                        .short("t")
                        .long("threads")
                        .value_name("NUMBER")
                        .help("Number of parallel threads/channels to use")
                        .default_value("1")
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
        .subcommand(
            SubCommand::with_name("ground-truth")
                .about("Run ground truth (non-secure plaintext) protocol for verification")
                .arg(
                    Arg::with_name("config")
                        .short("c")
                        .long("config")
                        .value_name("FILE")
                        .help("Configuration file path")
                        .required(true)
                )
        )
        .get_matches();

    let result = match matches.subcommand() {
        ("dealer", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            let num_threads = sub_matches.value_of("threads").unwrap()
                .parse::<usize>()
                .map_err(|_| "Invalid number of threads").expect("Failed to parse number of threads");
            run_dealer(config_path, num_threads)
        },
        ("client", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_client(config_path)
        },
        ("server0", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            let num_threads = sub_matches.value_of("threads").unwrap()
                .parse::<usize>()
                .map_err(|_| "Invalid number of threads").expect("Failed to parse number of threads");
            run_server0(config_path, num_threads)
        },
        ("server1", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            let num_threads = sub_matches.value_of("threads").unwrap()
                .parse::<usize>()
                .map_err(|_| "Invalid number of threads").expect("Failed to parse number of threads");
            run_server1(config_path, num_threads)
        },
        ("generate-config", Some(sub_matches)) => {
            let output_path = sub_matches.value_of("output").unwrap();
            generate_config(output_path)
        },
        ("ground-truth", Some(sub_matches)) => {
            let config_path = sub_matches.value_of("config").unwrap();
            run_ground_truth(config_path)
        },
        _ => {
            eprintln!("No subcommand specified. Use --help for usage information.");
            eprintln!("Available commands:");
            eprintln!("  dealer        - Run as FSS dealer");
            eprintln!("  client        - Run as client");
            eprintln!("  server0       - Run as server 0");
            eprintln!("  server1       - Run as server 1");
            eprintln!("  generate-config - Generate sample configuration");
            eprintln!("  ground-truth  - Run ground truth (non-secure) protocol");
            process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
