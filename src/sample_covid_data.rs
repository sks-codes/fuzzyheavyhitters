use std::{collections::HashSet, error::Error, fs::File, sync::Arc, io};
use std::collections::HashMap;
use csv::{ReaderBuilder, StringRecord, Position};
use memmap2::Mmap;
use rand::{rngs::StdRng, thread_rng, Rng, SeedableRng};
use rand::distributions::Uniform;
use rayon::prelude::*;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CountyCentroid {
    fips_code: String,
    latitude: f64,
    longitude: f64,
}

fn load_centroids(path: &str) -> Result<(HashMap<String, (f64, f64)>, HashSet<String>), Box<dyn Error>> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    let mut rdr = ReaderBuilder::new().from_reader(&mmap[..]);
    let mut map = HashMap::new();
    let mut fips_set = HashSet::new();

    for result in rdr.deserialize() {
        let record: CountyCentroid = result?;
        fips_set.insert(record.fips_code.clone());
        map.insert(record.fips_code, (record.latitude, record.longitude));
    }
    Ok((map, fips_set))
}

fn f64_to_bool_vec(value: f64) -> Vec<bool> {
    let bits = value.to_bits();
    (0..64).map(|i| ((bits >> (63 - i)) & 1) == 1).collect()
}

fn fuzzy_coords((lat, lon): (f64, f64), decimal_places: usize, rng: &mut StdRng) -> (f64, f64) {
    let noise_magnitude = 0.5 / 10f64.powi(decimal_places as i32);
    (
        (lat + rng.gen_range(-noise_magnitude,noise_magnitude)).clamp(-90.0, 90.0),
        (lon + rng.gen_range(-noise_magnitude,noise_magnitude)).clamp(-180.0, 180.0)
    )
}

fn uniform_in_square(lat: f64, lon: f64, side_length_km: f64, rng: &mut StdRng) -> (f64, f64) {
    // Calculate degrees per km at this latitude
    let km_per_deg_lat = 111.32;
    let km_per_deg_lon = 111.32 * lat.to_radians().cos();

    // Half-side length in degrees
    let a_lat = (side_length_km / 2.0) / km_per_deg_lat;
    let a_lon = (side_length_km / 2.0) / km_per_deg_lon;

    // Uniform distribution in [-a, a]
    let dist_lat = Uniform::new(-a_lat, a_lat);
    let dist_lon = Uniform::new(-a_lon, a_lon);

    (
        (lat + rng.sample(dist_lat)).clamp(-90.0, 90.0),
        (lon + rng.sample(dist_lon)).clamp(-180.0, 180.0)
    )
}

pub fn sample_covid_locations(
    covid_path: &str,
    centroids_path: &str,
    sample_size: usize,
    fuzz_factor: Option<f64>,
) -> Result<Vec<Vec<Vec<bool>>>, Box<dyn Error>> {
    // Load centroids
    let (centroids, valid_fips) = load_centroids(centroids_path)?;
    println!("Loaded {} county centroids", centroids.len());

    // Build record index
    let file = File::open(covid_path)?;
    let mut reader = ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(io::BufReader::new(file.try_clone()?));

    const COUNTY_FIPS_COL: usize = 4;
    let mut positions = Vec::new();
    let mut record = StringRecord::new();

    while reader.read_record(&mut record)? {
        let fips = match record.get(COUNTY_FIPS_COL) {
            Some(f) => f.trim(),
            None => continue,
        };

        // Strict FIPS validation
        if fips.len() != 5 || fips == "NA" || fips.contains('N') || fips.contains('A') {
            continue;
        }

        let full_fips = format!("{:0>5}", fips);
        if valid_fips.contains(&full_fips) {
            positions.push(reader.position().clone());
        }
    }

    // Check sufficient samples
    if positions.len() < sample_size {
        return Err(format!(
            "Need {} valid samples but only found {}",
            sample_size,
            positions.len()
        ).into());
    }

    println!("[4/4] Beginning reservoir sampling with {} positions...", positions.len());
    let mut rng = StdRng::from_entropy();
    let mut samples = Vec::with_capacity(sample_size);
    let file = File::open(covid_path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    // Ensure we have enough positions
    if positions.is_empty() {
        return Err("No valid positions found for sampling".into());
    }

    // Reservoir sampling with proper bounds checking
    for (i, pos) in positions.iter().enumerate() {
        if i % 100_000 == 0 {
            println!("[4/4] Processed {}/{} samples", i, positions.len());
        }

        // Read record
        let mut record_reader = ReaderBuilder::new()
            .has_headers(false)
            .from_reader(&mmap[pos.byte() as usize..]);
        let mut record = StringRecord::new();
        if record_reader.read_record(&mut record).is_err() {
            continue;
        }

        // Get coordinates
        let fips = match record.get(COUNTY_FIPS_COL) {
            Some(f) => f.trim(),
            None => continue,
        };
        let coords = match centroids.get(fips) {
            Some(c) => c,
            None => continue,
        };

        // Generate sample
        let sample = match fuzz_factor {
            Some(places) => {
                let (lat, lon) = uniform_in_square(coords.0, coords.1, places, &mut rng);
                vec![f64_to_bool_vec(lat), f64_to_bool_vec(lon)]
            },
            None => {
                vec![f64_to_bool_vec(coords.0), f64_to_bool_vec(coords.1)]
            }
        };

        // Reservoir sampling algorithm
        if samples.len() < sample_size {
            samples.push(sample);
        } else {
            let j = rng.gen_range(0,i+1);
            if j < samples.len() {  // Proper bounds check
                samples[j] = sample;
            }
        }
    }

    // Final check
    if samples.len() < sample_size {
        eprintln!("Warning: Only generated {} of {} requested samples", samples.len(), sample_size);
    }

    println!("[4/4] Completed sampling. Generated {} samples", samples.len());
    Ok(samples)
}