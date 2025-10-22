use mosaic::{
    channel::{CommTrackingChannel, connect_to, listen_to},
    configs::property_test_config::BenchmarkConfig,
    fuzzy_match::threshold_phase::{self, ThresholdPhase, ThresholdConfig, ThresholdMethod},
    data_structures::modint::ModInt,
};
use scuttlebutt::{AesRng, Channel, AbstractChannel};
use std::net::{TcpListener, TcpStream};
use std::io::{BufReader, BufWriter};
use std::thread;
use std::time::{Duration, Instant};
use rand::Rng;
use clap::{Arg, App};
use rayon::prelude::*;
use crossbeam;

fn generate_test_inputs(num_inputs: usize, modulus: u128) -> Vec<ModInt> {
    let mut rng = rand::thread_rng();
    (0..num_inputs)
        .map(|_| {
            ModInt::new(rng.random::<u128>(), modulus)
        })
        .collect()
}

fn run_server_benchmark(config_path: &str, server: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchmarkConfig::from_file(config_path)?;
    if server {
        println!("Running as server 1");
    } else {
        println!("Running as server 0");
    }

    // Create channels
    let mut other_server_channel = if server {
        connect_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    } else {
        listen_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    };

    let threshold_config = ThresholdConfig {
        h3: config.h3,
        is_garbler_side: server,
        method: ThresholdMethod::GC,
    };

    let threshold_phase = ThresholdPhase::new(threshold_config);

    // Generate test inputs - 1000 Vec<bool> with h2 bits each
    println!("Generating {} test inputs with bit length {}", config.num_clients, config.h3);
    let inputs = generate_test_inputs(config.num_clients, 1u128 << config.h3 as u128);

    println!("Starting server benchmark...");
    if server {
        other_server_channel.write_bytes(&[1u8]).unwrap();
        other_server_channel.flush().unwrap();
    } else {
        let mut ack = [0u8; 1];
        other_server_channel.flush().unwrap();
        other_server_channel.read_bytes(&mut ack).unwrap();
    }

    let start_time = Instant::now();
    let mut rng = AesRng::new();
    let threshold = ModInt::new(config.threshold, 1u128 << config.h3);
    let _results = threshold_phase.compare_with_threshold_gc(
        &inputs,
        threshold,
        &mut other_server_channel,
        &mut rng,
    ).map_err(|e| format!("ThresholdPhase error: {:?}", e))?;
    
    let elapsed = start_time.elapsed();
    
    // Print results
    println!("\n=== Server Benchmark Results ===");
    println!("Server time: {:?}", elapsed);
    let (sent, received) = other_server_channel.get_communication_stats();
    println!("Communication sent: {} bytes", sent);
    println!("Communication received: {} bytes", received);
    Ok(())
}


fn main() {
    let matches = App::new("Batch Equality GC Benchmark")
        .version("1.0")
        .author("Your Name")
        .about("Benchmarks batch equality GC testing")
        .arg(Arg::with_name("role")
            .short("r")
            .long("role")
            .value_name("ROLE")
            .help("Role to play: 'server0' or 'server1'")
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("config")
            .short("c")
            .long("config")
            .value_name("CONFIG_PATH")
            .help("Path to the configuration file")
            .required(true)
            .takes_value(true))
        .get_matches();

    let role = matches.value_of("role").unwrap();
    let config_path = matches.value_of("config").unwrap();

    let result = match role.to_lowercase().as_str() {
        "server0" => run_server_benchmark(config_path, false),
        "server1" => run_server_benchmark(config_path, true),
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
