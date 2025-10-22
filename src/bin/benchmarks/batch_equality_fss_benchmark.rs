use mosaic::{
    channel::{connect_to, listen_to},
    fuzzy_match::{
        share_phase::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod, SharePhase},
        check_phase::{CheckPhase, CheckConfig, CheckMethod, CheckProperty},
        protocol::request_dealer_equality,
        dealer::{FssDealer, DpfKeyBatch},
    },
};
use mosaic::configs::property_test_config::BenchmarkConfig;
use scuttlebutt::AbstractChannel;
use std::time::Instant;
use rand::Rng;
use clap::{Arg, App};

fn generate_test_inputs(num_inputs: usize, input_bit_length: usize) -> Vec<Vec<bool>> {
    let mut rng = rand::rng();
    (0..num_inputs)
        .map(|_| {
            (0..input_bit_length)
                .map(|_| rng.random::<bool>())
                .collect()
        })
        .collect()
}

fn run_dealer_benchmark(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchmarkConfig::from_file(config_path)?;

    // Create channels
    let mut signal_server0_channel = listen_to(config.dealer_addr.clone(), config.server0_to_dealer_port.parse::<u16>().unwrap())?;
    let mut check_server0_channel = listen_to(config.dealer_addr.clone(), config.server0_to_dealer_port.parse::<u16>().unwrap() + 1)?;
    let mut signal_server1_channel = listen_to(config.dealer_addr.clone(), config.server1_to_dealer_port.parse::<u16>().unwrap())?;
    let mut check_server1_channel = listen_to(config.dealer_addr.clone(), config.server1_to_dealer_port.parse::<u16>().unwrap() + 1)?;
    let mut dealer = FssDealer::new(
        config.mu,
        0,
        config.h2,
        config.h3,
        config.num_clients,
        config.d,
    );

    println!("Starting dealer benchmark...");

    let signal = dealer.read_dealer_signal(&mut signal_server0_channel)?;
    println!("Received signal from server 0: {:?}", signal);
    let start_time = Instant::now();
    let (server0_keys, server1_keys, random_pairs) = dealer.generate_fss_keys_for_equality().unwrap();
    println!("Time to generate FSS keys: {:?}", start_time.elapsed());

    let start_time = Instant::now();
    let batch_server0 = DpfKeyBatch {
        keys: server0_keys,
        random_values: random_pairs.iter().map(|(r0, _)| r0.clone()).collect(),
    };
    let batch_server1 = DpfKeyBatch {
        keys: server1_keys,
        random_values: random_pairs.iter().map(|(_, r1)| r1.clone()).collect(),
    };

    let (server0_result, server1_result) = rayon::join(
        || dealer.write_equality_key_batch(&mut check_server0_channel.clone(), &batch_server0),
        || dealer.write_equality_key_batch(&mut check_server1_channel.clone(), &batch_server1),
    );
    println!("Time to send key batch: {:?}", start_time.elapsed());
    let (server0_sent, server0_received) = check_server0_channel.get_communication_stats();
    println!("Sent {} bytes to server0", server0_sent);
    println!("Received {} bytes from server0", server0_received);
    let (server1_sent, server1_received) = check_server1_channel.get_communication_stats();
    println!("Sent {} bytes to server1", server1_sent);
    println!("Received {} bytes from server1", server1_received);


    Ok(())
}

fn run_server_benchmark(config_path: &str, server: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = BenchmarkConfig::from_file(config_path)?;
    if server {
        println!("Running as server 1");
    } else {
        println!("Running as server 0");
    }

    // Create channels
    let mut signal_dealer_channel = if server {
        connect_to(config.dealer_addr.clone(), config.server1_to_dealer_port.parse::<u16>().unwrap())?
    } else {
        connect_to(config.dealer_addr.clone(), config.server0_to_dealer_port.parse::<u16>().unwrap())?
    };
    let mut check_dealer_channel = if server {
        connect_to(config.dealer_addr.clone(), config.server1_to_dealer_port.parse::<u16>().unwrap() + 1)?
    } else {
        connect_to(config.dealer_addr.clone(), config.server0_to_dealer_port.parse::<u16>().unwrap() + 1)?
    };
    let mut other_server_channel = if server {
        connect_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    } else {
        listen_to(config.server0_addr.clone(), config.server0_to_server1_port.parse::<u16>().unwrap())?
    };
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
    let check_phase = CheckPhase::new(check_config);

    // Generate test inputs - 1000 Vec<bool> with h2 bits each
    println!("Generating {} test inputs with bit length {}", config.num_clients, config.h2 * config.d);
    let inputs = generate_test_inputs(config.num_clients, config.h2 * config.d);

    println!("Starting server benchmark...");

    let start_time = Instant::now();
    let batch = request_dealer_equality(&mut signal_dealer_channel, &mut check_dealer_channel, 1u128 << config.h3 as u128).unwrap();
    println!("Time to request dealer equality: {:?}", start_time.elapsed());

    if server {
        other_server_channel.write_bytes(&[1u8]).unwrap();
        other_server_channel.flush().unwrap();
    } else {
        let mut ack = [0u8; 1];
        other_server_channel.flush().unwrap();
        other_server_channel.read_bytes(&mut ack).unwrap();
    }

    let keys = batch.keys;
    let random_values = batch.random_values;

    let start_time = Instant::now();
    let _results = check_phase.batch_equality_testing_fss(
        &inputs,
        &keys,
        &random_values,
        &mut other_server_channel,
    ).map_err(|e| format!("CheckPhase error: {:?}", e))?;
    
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
    let matches = App::new("Batch Mu Bounded FSS Benchmark")
        .version("1.0")
        .author("Your Name")
        .about("Benchmarks batch mu bounded FSS testing")
        .arg(Arg::with_name("role")
            .short("r")
            .long("role")
            .value_name("ROLE")
            .help("Role to play: 'server0', 'server1', or 'dealer'")
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
        "dealer" => run_dealer_benchmark(config_path),
        _ => {
            eprintln!("Invalid role '{}'. Must be 'server0', 'server1', or 'dealer'", role);
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
