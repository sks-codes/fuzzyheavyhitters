use csv::Writer;
use std::time::Instant;

use std::{io, mem};
use rand::Rng;
use rayon::prelude::*;
use rand::distributions::Alphanumeric;

use std::time::{Duration, SystemTime};
use counttree::config::Config;
use counttree::ibDCF::ibDCFKey;
use counttree::string_to_bits;


fn sample_string(len: usize) -> String {
    let mut rng = rand::thread_rng();
    std::iter::repeat(())
        .map(|()| rng.sample(Alphanumeric))
        .take(len / 8)
        .collect()
}
fn generate_ibDCF_keys(string_length : usize, num_keys : usize) -> (Vec<ibDCFKey>, Vec<ibDCFKey>) {

    rayon::iter::repeat(0)
        .take(num_keys)
        .enumerate()
        .map(|(i, _)| {
            let data_string = sample_string(string_length);
            let keys = ibDCFKey::gen_ibDCF(string_to_bits(&data_string).as_slice(), false);
            (keys.0.clone(), keys.1.clone())
        })
        .collect::<Vec<_>>()
        .into_iter()
        .unzip()
}
// fn generate_l_inf_keys(string_length : usize, num_keys : usize, num_dims : usize) -> (Vec<ibDCFKey>, Vec<ibDCFKey>) {
//
//     rayon::iter::repeat(0)
//         .take(num_keys)
//         .enumerate()
//         .map(|(i, _)| {
//             let data_string = sample_string(string_length);
//             let keys = ibDCFKey::gen_l_inf_ball(string_to_bits(&data_string).as_slice(), num_dims);
//             (keys.0.clone(), keys.1.clone())
//         })
//         .collect::<Vec<_>>()
//         .into_iter()
//         .unzip()
// }


#[tokio::main]
async fn main() -> io::Result<()> {
    println!("Using only one thread!");
    rayon::ThreadPoolBuilder::new().num_threads(1).build_global().unwrap();

    let mut wtr = Writer::from_path("src/bin/benchmarks/ibDCFbench.csv")?;
    wtr.write_record(&["string_length", "number_keys", "time", "avg_time", "size"])?;

    let string_lengths = [128, 256, 384, 512, 640, 768, 896, 1024];
    let num_keys = 10000;

    for i in string_lengths{
        let start = Instant::now();
        let keys= generate_ibDCF_keys(i, num_keys);
        let delta = start.elapsed().as_secs_f64();
        let encoded: Vec<u8> = bincode::serialize(&keys.0[0]).unwrap();
        wtr.write_record(&[i.to_string(), num_keys.to_string(), delta.to_string(), (delta / (num_keys as f64)).to_string(), encoded.len().to_string()])?;
    }
    wtr.flush()?;

    //Testing L-infinity ball
    let mut wtr = Writer::from_path("src/bin/benchmarks/ibDCFbench.csv")?;
    wtr.write_record(&["string_length", "number_keys", "time", "avg_time", "size"])?;

    let string_lengths = [128, 256, 384, 512, 640, 768, 896, 1024];
    let num_keys = 10000;

    for i in string_lengths{
        let start = Instant::now();
        let keys= generate_ibDCF_keys(i, num_keys);
        let delta = start.elapsed().as_secs_f64();
        let encoded: Vec<u8> = bincode::serialize(&keys.0[0]).unwrap();
        wtr.write_record(&[i.to_string(), num_keys.to_string(), delta.to_string(), (delta / (num_keys as f64)).to_string(), encoded.len().to_string()])?;
    }
    wtr.flush()?;

    Ok(())
}