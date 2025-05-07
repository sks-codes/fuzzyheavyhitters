use std::error::Error;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use csv::{Reader, Writer, StringRecord};
use rand::{seq::IteratorRandom, rngs::StdRng, SeedableRng};
use std::path::Path;

const CENTIDEGREES_SCALE: f64 = 100.0; // 2 decimal places (~1.1 km precision)

// Convert lat/lng floats to centidegrees (i16)
fn geo_to_int(lat: f64, lng: f64) -> (i16, i16) {
    let lat_int = (lat * CENTIDEGREES_SCALE).round() as i16;
    let lng_int = (lng * CENTIDEGREES_SCALE).round() as i16;
    (lat_int, lng_int)
}

// Convert centidegrees back to floats
fn int_to_geo(lat_int: i16, lng_int: i16) -> (f64, f64) {
    let lat = f64::from(lat_int) / CENTIDEGREES_SCALE;
    let lng = f64::from(lng_int) / CENTIDEGREES_SCALE;
    (lat, lng)
}

/// Convert i16 to 16-bit vector (MSB first)
pub fn i16_to_bitvec(value: i16) -> Vec<bool> {
    let bits = value as u16; // Safe for bit ops
    (0..16).map(|i| (bits >> (15 - i)) & 1 == 1).collect()
}

/// Convert 16-bit vector back to i16
fn bitvec_to_i16(bits: &[bool]) -> i16 {
    let mut value: u16 = 0;
    for (i, &bit) in bits.iter().enumerate() {
        if bit {
            value |= 1 << (15 - i);
        }
    }
    value as i16
}

/// Sample start locations as 16-bit centidegrees
// pub fn sample_start_locations<P: AsRef<Path>>(
//     path: P,
//     sample_size: usize,
//     seed: Option<u64>,
// ) -> Result<Vec<Vec<Vec<bool>>>, Box<dyn std::error::Error>> {
//     let mut rdr = Reader::from_path(path)?;
//     let mut rng = match seed {
//         Some(s) => StdRng::seed_from_u64(s),
//         None => StdRng::from_entropy(),
//     };
//
//     let records: Vec<StringRecord> = rdr.records().collect::<Result<_, _>>()?;
//
//     records
//         .iter()
//         .choose_multiple(&mut rng, sample_size)
//         .into_iter()
//         .map(|record| {
//             let (lat_int, lon_int) = geo_to_int(
//                 record[12].parse::<f64>()?, // start_lat
//                 record[13].parse::<f64>()?, // start_lon
//             );
//             Ok(vec![
//                 i16_to_bitvec(lat_int),
//                 i16_to_bitvec(lon_int),
//             ])
//         })
//         .collect()
// }

pub fn sample_start_locations<P: AsRef<Path>>(
        path: P,
        sample_size: usize,
        seed: Option<u64>,
    ) -> Result<Vec<(i16, i16)>, Box<dyn std::error::Error>> {
    let mut rdr = Reader::from_path(path)?;
    let mut rng = match seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_entropy(),
    };

    let records: Vec<StringRecord> = rdr.records().collect::<Result<_, _>>()?;

    records
        .iter()
        .choose_multiple(&mut rng, sample_size)
        .into_iter()
        .map(|record| {
            let (lat_int, lon_int) = geo_to_int(
                record[14].parse::<f64>()?, // start_lat
                record[13].parse::<f64>()?, // start_lon
            );
            Ok((lat_int, lon_int))
        })
        .collect()
}

fn keep_only_header<P: AsRef<Path>>(input_path: P, output_path: P) -> Result<(), Box<dyn Error>> {
    // Open input file
    let input_file = File::open(input_path)?;
    let mut reader = csv::Reader::from_reader(BufReader::new(input_file));

    // Open output file
    let output_file = File::create(output_path)?;
    let mut writer = csv::Writer::from_writer(BufWriter::new(output_file));

    // Get headers and write them to output
    let headers = reader.headers()?.clone();
    writer.write_record(&headers)?;

    writer.flush()?;
    Ok(())
}

/// Save heavy hitters with centidegree conversion
pub fn save_heavy_hitters(
    heavy_hitters: &[Vec<bool>],
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Open file in append mode (creates if doesn't exist)
    let file = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(output_path)?;

    let mut wtr = csv::Writer::from_writer(file);

    // Only write headers if file is empty
    if std::fs::metadata(output_path)?.len() == 0 {
        wtr.write_record(&["index", "latitude", "longitude"])?;
    }

    for (i, chunk) in heavy_hitters.chunks_exact(2).enumerate() {
        let lat = bitvec_to_i16(&chunk[0]);
        let lon = bitvec_to_i16(&chunk[1]);
        let (lat_float, lon_float) = int_to_geo(lat, lon);

        wtr.write_record(&[
            i.to_string(),
            lat_float.to_string(),
            lon_float.to_string(),
        ])?;
    }

    wtr.flush()?;
    Ok(())
}
#[test]
fn test_austin_coords() {
    let (lat, lon) = (30.26, -97.74); // Austin, 2 decimal places
    let (lat_int, lon_int) = geo_to_int(lat, lon);
    let bits_lat = i16_to_bitvec(lat_int);
    let bits_lon = i16_to_bitvec(lon_int);

    let reconstructed_lat = bitvec_to_i16(&bits_lat);
    let reconstructed_lon = bitvec_to_i16(&bits_lon);
    let (lat_back, lon_back) = int_to_geo(reconstructed_lat, reconstructed_lon);

    assert_eq!(lat, lat_back); // Exact match (no floating-point errors)
    assert_eq!(lon, lon_back);
    println!("Test passed! Coordinates: ({}, {})", lat_back, lon_back);
}