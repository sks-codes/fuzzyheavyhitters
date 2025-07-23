#!/usr/bin/env rust
//! Synthetic Data Generator Binary
//! 
//! This binary generates synthetic clustered location data for testing
//! the fuzzy heavy hitters protocol. It reads configuration from a JSON
//! file and exports the generated data to specified output files.

use counttree::synthetic_data::{SyntheticDataGenerator, SyntheticDataConfig};
use std::env;
use std::fs;
use std::path::Path;
use serde_json;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() != 2 {
        eprintln!("Usage: {} <config_file.json>", args[0]);
        eprintln!("Example: {} data/synthetic_config.json", args[0]);
        std::process::exit(1);
    }
    
    let config_path = &args[1];
    
    // Read and parse configuration file
    let config = match read_config(config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error reading config file '{}': {}", config_path, e);
            std::process::exit(1);
        }
    };
    
    println!("=== Synthetic Data Generator ===");
    println!("Config file: {}", config_path);
    println!("Configuration: {:#?}", config);
    println!();
    
    // Generate synthetic dataset
    let generator = SyntheticDataGenerator::new(config);
    let dataset = generator.generate();
    
    // Print summary
    dataset.print_summary();
    
    // Export data to files
    let data_dir = "data/synthetic";
    if let Err(e) = export_dataset(&dataset, data_dir) {
        eprintln!("Error exporting dataset: {}", e);
        std::process::exit(1);
    }
    
    println!("✅ Synthetic data generation completed successfully!");
    println!("📁 Data exported to: {}/", data_dir);
}

fn read_config(config_path: &str) -> Result<SyntheticDataConfig, Box<dyn std::error::Error>> {
    let config_content = fs::read_to_string(config_path)?;
    let config: SyntheticDataConfig = serde_json::from_str(&config_content)?;
    Ok(config)
}

fn export_dataset(dataset: &counttree::synthetic_data::SyntheticDataset, output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Create output directory if it doesn't exist
    fs::create_dir_all(output_dir)?;
    
    // Export client points
    let client_points_path = Path::new(output_dir).join("client_points.json");
    let client_points_json = serde_json::to_string_pretty(&dataset.get_client_points())?;
    fs::write(&client_points_path, client_points_json)?;
    println!("📄 Client points exported to: {}", client_points_path.display());
    
    // Export server query points (cluster centers)
    let server_points_path = Path::new(output_dir).join("server_query_points.json");
    let server_points_json = serde_json::to_string_pretty(&dataset.get_server_query_points())?;
    fs::write(&server_points_path, server_points_json)?;
    println!("📄 Server query points exported to: {}", server_points_path.display());
    
    // Export cluster information
    let clusters_path = Path::new(output_dir).join("clusters.json");
    let clusters_json = serde_json::to_string_pretty(&dataset.get_clusters())?;
    fs::write(&clusters_path, clusters_json)?;
    println!("📄 Cluster information exported to: {}", clusters_path.display());
    
    // Export complete dataset
    let full_dataset_path = Path::new(output_dir).join("full_dataset.json");
    let full_dataset_json = serde_json::to_string_pretty(dataset)?;
    fs::write(&full_dataset_path, full_dataset_json)?;
    println!("📄 Full dataset exported to: {}", full_dataset_path.display());
    
    // Export summary statistics
    let stats_path = Path::new(output_dir).join("dataset_stats.txt");
    let stats_content = format!(
        "Synthetic Dataset Statistics\n\
         ============================\n\
         Total clusters: {}\n\
         Total client points: {}\n\
         Server query points: {}\n\
         Coordinate bounds: {:?}\n\
         Max cluster radius: {}\n\
         Dimensions: {}\n\n\
         Cluster Details:\n",
        dataset.clusters.len(),
        dataset.total_client_points,
        dataset.server_query_points.len(),
        dataset.config.coordinate_bounds,
        dataset.config.max_cluster_radius,
        dataset.config.dimensions
    );
    
    let mut cluster_details = String::new();
    for (i, cluster) in dataset.clusters.iter().enumerate() {
        cluster_details.push_str(&format!(
            "Cluster {}: Center={:?}, Size={}\n",
            i + 1, cluster.center, cluster.size
        ));
    }
    
    fs::write(&stats_path, stats_content + &cluster_details)?;
    println!("📄 Dataset statistics exported to: {}", stats_path.display());
    
    Ok(())
}
