use clap::Parser;
use csv::Reader;
use mosaic::sample_driving_data::geo_to_grid;
use rand::prelude::*;
use serde_json;
use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

fn read_csv_and_convert<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u128>>, Box<dyn Error>> {
    let mut rdr = Reader::from_path(path)?;

    rdr.records()
        .map(|record| {
            let record = record?;
            let start_lon = record[15].parse::<f64>()?;
            let start_lat = record[16].parse::<f64>()?;
            // let end_lat = record[6].parse::<f64>()?;
            // let end_lon = record[7].parse::<f64>()?;

            // Convert to grid coordinates (same as csv_to_bitvecs function)
            let (start_lat_grid, start_lon_grid) = geo_to_grid(start_lat, start_lon);
            // let (end_lat_grid, end_lon_grid) = geo_to_grid(end_lat, end_lon);

            // Convert to u128 and create point as [lat, lon]
            Ok(vec![start_lat_grid as u128, start_lon_grid as u128])
        })
        .collect()
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    input: String,
    #[arg(short, long)]
    output: String,
    #[arg(short, long)]
    query_num: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let input_file = &args.input;
    let output_file = &args.output;
    let query_num = args.query_num;

    println!("Starting RideAustin client points JSON generation...");

    // Check if input file exists
    if !Path::new(input_file).exists() {
        eprintln!("Error: Input file '{}' does not exist.", input_file);
        eprintln!("Please make sure you have the ride data CSV file at this location.");
        return Ok(());
    }

    // Read and convert CSV data
    println!("Reading and converting CSV data from {}...", input_file);
    let client_points = read_csv_and_convert(input_file)?;
    println!(
        "Converted {} ride points to client format",
        client_points.len()
    );

    // Sample query_num points
    let client_points = if client_points.len() > query_num {
        let mut rng = rand::rng();
        let sampled_points: Vec<Vec<u128>> = client_points
            .choose_multiple(&mut rng, query_num)
            .cloned()
            .collect();
        sampled_points
    } else {
        client_points
    };

    // Write to JSON file in the same format as data/synthetic/client_points.json
    println!("Writing client points JSON to {}...", output_file);
    let file = File::create(output_file)?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, &client_points)?;

    println!("Successfully generated client points JSON!");
    println!("Total points: {}", client_points.len());
    println!("Output file: {}", output_file);

    // Print some sample statistics
    if !client_points.is_empty() {
        println!("\nSample points:");
        for (i, point) in client_points.iter().take(5).enumerate() {
            println!(
                "Point {}: [start_lat_grid: {}, start_lon_grid: {}]",
                i + 1,
                point[0],
                point[1]
            );
            // println!("  Point {}: [start_lat_grid: {}, start_lon_grid: {}, end_lat_grid: {}, end_lon_grid: {}]", i+1, point[0], point[1], point[2], point[3]);
        }
    }

    Ok(())
}
