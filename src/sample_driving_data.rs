use std::error::Error;
use std::fs::File;
use std::path::Path;
use csv::{Reader, Writer};
use std::io::BufWriter;

// Austin bounding box
const AUSTIN_CENTER: (f64, f64) = (30.267, -97.743);
const BUFFER_DEGREES: f64 = 1.0;
const AUSTIN_MIN_LAT: f64 = AUSTIN_CENTER.0 - BUFFER_DEGREES;
const AUSTIN_MAX_LAT: f64 = AUSTIN_CENTER.0 + BUFFER_DEGREES;
const AUSTIN_MIN_LON: f64 = AUSTIN_CENTER.1 - BUFFER_DEGREES;
const AUSTIN_MAX_LON: f64 = AUSTIN_CENTER.1 + BUFFER_DEGREES;

//Grid metadata
const PRECISION: i32 = 3;
const DECIMAL_SCALE: u32 = 1000;

const LAT_GRID_SIZE: u32 = 2000;
const LON_GRID_SIZE: u32 = 2000;

const LAT_BITS: usize = 11;
const LON_BITS: usize = 11;

pub fn geo_to_grid(lat: f64, lon: f64) -> (u32, u32) {
    let lat_grid = ((lat - AUSTIN_MIN_LAT) * DECIMAL_SCALE as f64).round() as u32;
    let lon_grid = ((lon - AUSTIN_MIN_LON) * DECIMAL_SCALE as f64).round() as u32;
    (
        if lat_grid < LAT_GRID_SIZE { lat_grid } else { LAT_GRID_SIZE - 1 },
        if lon_grid < LON_GRID_SIZE { lon_grid } else { LON_GRID_SIZE - 1 }
    )
}

pub fn grid_to_geo(lat_grid: u32, lon_grid: u32) -> (f64, f64) {
    (
        AUSTIN_MIN_LAT + (lat_grid as f64 / DECIMAL_SCALE as f64),
        AUSTIN_MIN_LON + (lon_grid as f64 / DECIMAL_SCALE as f64)
    )
}

pub fn to_bitvec(value: u32, bits: usize) -> Vec<bool> {
    (0..bits).map(|i| ((value >> (bits - 1 - i)) & 1) == 1).collect()
}

pub fn from_bitvec(bits: &[bool]) -> u32 {
    bits.iter().enumerate().fold(0, |acc, (i, &bit)| {
        acc | ((bit as u32) << (bits.len() - 1 - i))
    })
}

pub fn csv_to_bitvecs<P: AsRef<Path>>(
    path: P,
) -> Result<Vec<Vec<Vec<bool>>>, Box<dyn Error>> {
    let mut rdr = Reader::from_path(path)?;

    rdr.records().map(|record| {
        let record = record?;
        let start_lon = record[15].parse::<f64>()?;
        let start_lat = record[16].parse::<f64>()?;
        // let end_lon = record[7].parse::<f64>()?;
        // let end_lat = record[6].parse::<f64>()?;

        let (start_lat_grid, start_lon_grid) = geo_to_grid(start_lat, start_lon);
        // let (end_lat_grid, end_lon_grid) = geo_to_grid(end_lat, end_lon);
        let start_lat_bits = to_bitvec(start_lat_grid, LAT_BITS);
        let start_lon_bits = to_bitvec(start_lon_grid, LON_BITS);
        // let end_lat_bits = to_bitvec(end_lat_grid, LAT_BITS);
        // let end_lon_bits = to_bitvec(end_lon_grid, LON_BITS);
        Ok(vec![start_lat_bits, start_lon_bits])//, end_lat_bits, end_lon_bits])
    }).collect()
}

pub fn save_heavy_hitters(
    heavy_hitters: Vec<Vec<bool>>,
    output_path: &str,
) -> Result<(), Box<dyn Error>> {
    let file = File::options()
        .append(true)
        .create(true)
        .open(output_path)?;

    let mut wtr = Writer::from_writer(BufWriter::new(file));

    if std::fs::metadata(output_path)?.len() == 0 {
        wtr.write_record(&["latitude", "longitude"])?;
    }
    let lat_bits = heavy_hitters[0].clone();
    let lon_bits = heavy_hitters[1].clone();
    let lat_grid = from_bitvec(lat_bits.as_slice());
    let lon_grid = from_bitvec(lon_bits.as_slice());
    let (lat, lon) = grid_to_geo(lat_grid, lon_grid);

    wtr.write_record(&[
        lat.to_string(),
        lon.to_string(),
    ])?;

    wtr.flush()?;
    Ok(())
}

#[test]
fn test_grid_conversion() {
    let (lat, lon) = (30.2672, -97.7431);
    let (lat_grid, lon_grid) = geo_to_grid(lat, lon);
    let (lat_back, lon_back) = grid_to_geo(lat_grid, lon_grid);

    let tolerance = 1.0 / DECIMAL_SCALE as f64;
    assert!((lat - lat_back).abs() < tolerance);
    assert!((lon - lon_back).abs() < tolerance);
    println!("Test passed! Grid coordinates: ({}, {})", lat_grid, lon_grid);

    let bits = to_bitvec(lat_grid, LAT_BITS);
    let reconstructed = from_bitvec(&bits);
    assert_eq!(lat_grid, reconstructed);
}