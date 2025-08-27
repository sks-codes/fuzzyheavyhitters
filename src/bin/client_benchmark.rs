use counttree::fuzzy_match::client::Client;
use counttree::fuzzy_match::{
    share_phase::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod, SharePhase},
};
use std::time::Instant;
use rand::Rng;

fn main() {
    let share_config = ShareConfig {
        method: ShareMethod::OKVS,
        metric: DistanceMetric::LInfinity,
        dictionary_type: DictionaryType::Unknown,
        h1: 20,
        h2: 20,
        d: 4,
    };
    
    // let share_phase = SharePhase::new(share_config);
    let client = Client::new(share_config);

    let num_clients = 1usize << 20;
    let random_points = (0..num_clients).map(|_| {
        let mut rng = rand::thread_rng();
        (0..4).map(|_| {
            rng.random::<u128>()
        }).collect::<Vec<u128>>()
    }).collect::<Vec<Vec<u128>>>();

    let start = Instant::now();
    let delta = 100u128;
    let results = client.generate_client_shares(&random_points, delta).unwrap();
    println!("Time to generate shares for {} clients: {:?}", num_clients, start.elapsed());

    let mut bytes = Vec::new();
    for result in results.0.iter() {
        bytes.extend_from_slice(&result.to_bytes());
    }

    println!("Total bytes generated: {}", bytes.len());
}