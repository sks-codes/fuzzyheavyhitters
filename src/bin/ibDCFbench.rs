//
// use counttree::{
//     FieldElm,
//     collect, config, fastfield, mpc,
//     rpc::{
//         AddKeysRequest, FinalSharesRequest, ResetRequest,
//         TreeInitRequest,
//         TreeCrawlRequest,
//         TreeCrawlLastRequest,
//         TreeOutSharesRequest,
//         TreeOutSharesLastRequest,
//         TreePruneRequest,
//         TreePruneLastRequest,
//         TreeSketchFrontierRequest,
//         TreeSketchFrontierLastRequest,
//     },
//     sketch,
// };
//
// use csv::Writer;
// use std::time::Instant;
//
// use futures::try_join;
use std::{io, mem};
// use std::net::SocketAddr;
// use rand::Rng;
// use rayon::prelude::*;
// use tarpc::{
//     client,
//     context,
//     serde_transport::tcp,
//     tokio_serde::formats::Bincode,
//     //server::{self, Channel},
// };
//
// use rand::distributions::Alphanumeric;
//
// use std::time::{Duration, SystemTime};
// use counttree::config::Config;
// use counttree::ibDCF::ibDCFKey;
//
// type SketchKey = sketch::SketchDPFKey<fastfield::FE,FieldElm>;
//
// fn long_context() -> context::Context {
//     let mut ctx = context::current();
//
//     // Increase timeout to one hour
//     ctx.deadline = SystemTime::now() + Duration::from_secs(1000000);
//     ctx
// }
//
// // Function to calculate the size of ibDCFKey
// fn size_of_key(key: &ibDCFKey) -> usize {
//     let seed_size = size_of_val(&key.cor_words[0].seed); // Size of PrgSeed
//     let bits_size = size_of_val(&key.cor_words[0].bits); // Size of (bool, bool)
//     let y_bits_size = size_of_val(&key.cor_words[0].y_bits); // Size of (bool, bool)
//
//     let cor_word_size = seed_size + bits_size + y_bits_size;
//
//     let key_idx_size = size_of_val(&key.key_idx); // Size of key_idx (bool)
//     let root_seed_size = size_of_val(&key.root_seed); // Size of PrgSeed
//     let cor_words_metadata_size = mem::size_of_val(&key.cor_words); // Metadata size of Vec<CorWord>
//
//     let cor_words_data_size: usize = cor_word_size * key.cor_words.len();
//
//     key_idx_size + root_seed_size + cor_words_metadata_size + cor_words_data_size
// }
//
//
// fn sample_string(len: usize) -> String {
//     let mut rng = rand::thread_rng();
//     std::iter::repeat(())
//         .map(|()| rng.sample(Alphanumeric))
//         .take(len / 8)
//         .collect()
// }
// fn generate_ibDCF_keys(num_sites : usize, data_len : usize) -> (Vec<ibDCFKey<T, U>>, Vec<ibDCFKey<T, U>>) {
//     let (keys0, keys1): (Vec<ibDCFKey<T, U>>, Vec<ibDCFKey<T, U>>) = rayon::iter::repeat(0)
//         .take(num_sites)
//         .map(|_| {
//             let data_string = sample_string(data_len);
//             let keys = ibDCFKey::gen(&counttree::string_to_bits(&data_string), false);//sketch::SketchDPFKey::gen_from_str(&data_string);
//
//             // XXX remove these clones
//             (keys.0.clone(), keys.1.clone())
//         })
//         .unzip();
//     let encoded: Vec<u8> = bincode::serialize(&keys0[0]).unwrap();
//     // println!("Key size: {:?} bytes", encoded.len());
//     (keys0, keys1)
// }

#[tokio::main]
async fn main() -> io::Result<()> {
    // println!("Using only one thread!");
    // rayon::ThreadPoolBuilder::new().num_threads(1).build_global().unwrap();
    //
    // let mut wtr = Writer::from_path("ibDCFbench.csv")?;
    // wtr.write_record(&["string_length", "number_keys", "time", "avg_time", "size"])?;
    //
    // let string_lengths = [128, 256, 384, 512, 640, 768, 896, 1024];
    //
    // for i in string_lengths{
    //     let start = Instant::now();
    //     let (keys0, keys1) = generate_ibDCF_keys(10000, i);
    //     let delta = start.elapsed().as_secs_f64();
    //     wtr.write_record(&[i.to_string(), keys0.len().to_string(), delta.to_string(), (delta / (keys0.len() as f64)).to_string(), size_of_key(&keys0[0]).to_string()])?;
    // }
    // // Flush the writer to ensure data is written to the file
    // wtr.flush()?;

    Ok(())
}