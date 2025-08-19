use counttree::channel::{CommTrackingChannel, connect_to, listen_to};
use counttree::configs::property_test_config::PropertyTestConfig;
use counttree::fuzzy_match::check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckProperty};
use counttree::fuzzy_match::share_phase::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod, SharePhase};
use scuttlebutt::{AesRng, Channel, AbstractChannel};
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;
use clap::{Arg, App};
use rayon::prelude::*;
use crossbeam;

fn generate_test_inputs(num_inputs: usize, input_bit_length: usize) -> Vec<Vec<bool>> {
    let mut rng = rand::thread_rng();
    (0..num_inputs)
        .map(|_| {
            (0..input_bit_length)
                .map(|_| rng.gen::<bool>())
                .collect()
        })
        .collect()
}

fn run_server_benchmark(config_path: &str, server: bool, num_threads: usize) -> Result<(), Box<dyn std::error::Error>> {
    let config = PropertyTestConfig::from_file(config_path)?;
    if server {
        println!("Running as server 1");
    } else {
        println!("Running as server 0");
    }

    // Create multiple channels for parallel runs
    let base_port: u16 = config.server0_to_server1_port.parse()?;
    let mut channels: Vec<CommTrackingChannel> = (0..num_threads)
        .map(|i| -> Result<_, Box<dyn std::error::Error>> {
            let port = base_port + i as u16;
            if server {
                connect_to(config.server0_addr.clone(), port)
            } else {
                listen_to(config.server0_addr.clone(), port)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Create CheckConfig for garbler
    let check_config = CheckConfig {
        h2: config.h2,
        h3: config.h3,
        d: config.d,
        is_garbler_side: server,
        property: CheckProperty::Equality,
        method: CheckMethod::FSS,
    };
    
    // Create SharePhase (dummy configuration since we're not using it for generation)
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        metric: DistanceMetric::LInfinity,
        dictionary_type: DictionaryType::Known,
        h1: config.h1,
        h2: config.h2,
        d: config.d,
    };
    
    let share_phase = SharePhase::new(share_config);
    let check_phase = CheckPhase::new(check_config, share_phase);

    for i in 0..30 {
        // Generate test inputs - 1000 Vec<bool> with h2 bits each
        println!("Generating {} test inputs with bit length {}", num_threads * config.num_clients, config.h2 * config.d);
        let inputs = generate_test_inputs(num_threads * config.num_clients, config.h2 * config.d);

        println!("Starting server benchmark with {} threads...", num_threads);

        // Handshake on all channels to synchronize
        if server {
            for ch in channels.iter_mut() {
                ch.write_bytes(&[1u8]).unwrap();
                ch.flush().unwrap();
            }
        } else {
            for ch in channels.iter_mut() {
                let mut ack = [0u8; 1];
                ch.flush().unwrap();
                ch.read_bytes(&mut ack).unwrap();
            }
        }

        // Split inputs into chunks for each thread
        let chunk_size = (inputs.len() + num_threads - 1) / num_threads;
        let input_chunks: Vec<&[Vec<bool>]> = inputs.chunks(chunk_size).collect();

        let start_time = Instant::now();

        // Use crossbeam scope for better thread isolation (like tree_crawl)
        let thread_results = crossbeam::scope(|s| {
            let mut handles = vec![];
            
            for (thread_idx, (ch, in_chunk)) in channels.iter_mut().zip(input_chunks.iter()).enumerate() {
                if in_chunk.is_empty() { continue; }
                
                let chunk = in_chunk.to_vec(); // Copy chunk for thread ownership
                let local_cp = check_phase.clone();
                
                handles.push(s.spawn(move |_| {
                    let chunk_start = Instant::now();
                    let mut rng = AesRng::new();
                    let mut channel = ch.clone(); // Each thread gets its own channel clone
                    
                    let result = local_cp
                        .batch_equality_testing_gc(&chunk, &mut channel, &mut rng)
                        .map_err(|e| format!("CheckPhase error: {:?}", e));
                    
                    let chunk_elapsed = chunk_start.elapsed();
                    println!("Crossbeam Thread {} (chunk size {}): {:?}", thread_idx, chunk.len(), chunk_elapsed);
                    
                    (thread_idx, result)
                }));
            }
            
            // Collect results
            let mut results = vec![];
            for handle in handles {
                results.push(handle.join().unwrap());
            }
            results
        }).unwrap();

        // Check for any errors
        for (thread_idx, result) in thread_results {
            result.map_err(|e| format!("Thread {} failed: {}", thread_idx, e))?;
        }

        let elapsed = start_time.elapsed();
        println!("Server time (parallel): {:?}", elapsed);
    }
    
    // Print results
    println!("\n=== Server Benchmark Results ===");
    let (total_sent, total_received) = channels.iter().fold((0usize, 0usize), |(s, r), ch| {
        let (cs, cr) = ch.get_communication_stats();
        (s + cs, r + cr)
    });
    println!("Communication sent (all channels): {} bytes", total_sent);
    println!("Communication received (all channels): {} bytes", total_received);
    Ok(())
}

fn main() {
    let matches = App::new("Batch Equality GC Benchmark")
        .version("1.0")
        .author("Your Name")
        .about("Benchmarks batch equality testing using garbled circuits")
        .arg(Arg::with_name("role")
            .short("r")
            .long("role")
            .value_name("ROLE")
            .help("Role to play: 'garbler' or 'evaluator'")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("CONFIG_PATH")
            .help("Path to the configuration file")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("threads")
            .short("t")
            .long("threads")
            .value_name("N")
            .help("Number of parallel channels (threads)")
            .required(false)
            .takes_value(true))
        .get_matches();

    let role = matches.value_of("role").unwrap();
    let config_path = matches.value_of("config").unwrap();

    let num_threads: usize = matches.value_of("threads").unwrap_or("1").parse().unwrap_or(1);
    
    // Set Rayon global thread pool to match requested parallelism
    rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global()
        .unwrap_or_else(|_| println!("Warning: Rayon thread pool already initialized"));

    let result = match role.to_lowercase().as_str() {
        "server0" => run_server_benchmark(config_path, false, num_threads),
        "server1" => run_server_benchmark(config_path, true, num_threads),
        _ => {
            eprintln!("Invalid role '{}'. Must be 'server0' or 'server1'", role);
            std::process::exit(1);
        }
    };

    match result {
        Ok(()) => {
            println!("Benchmark completed successfully!");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Benchmark failed: {}", e);
            std::process::exit(1);
        }
    }
}
