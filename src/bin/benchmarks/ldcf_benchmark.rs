use mosaic::{
    util::u128_to_bits_msb,
    data_structures::ringvec::RingVec,
    fss::ldcf::LdcfKey,
};
use clap::Parser;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    alpha: u128,
    #[arg(long)]
    domain_size: usize,
}

fn main() {
    let args = Args::parse();

    let alpha = args.alpha;
    let alpha_bits = u128_to_bits_msb(alpha, args.domain_size);
    let domain_size = args.domain_size;

    // Hardcode the modulus and payload for benchmarking
    let modulus = 1u128 << 20;
    let a = RingVec::new(vec![5u128; 2], modulus).expect("Failed to create RingVec");
    let b = RingVec::new(vec![3u128; 2], modulus).expect("Failed to create RingVec");

    let (key0, key1) = LdcfKey::<2>::gen_ldcf_key(
        &alpha_bits,
        &a,
        &b,
        modulus,
    );

    // Benchmark Full Domain Evaluation
    let start = Instant::now();
    let eval0 = key0.full_domain_eval(modulus, domain_size);
    let eval1 = key1.full_domain_eval(modulus, domain_size);
    let duration = start.elapsed();
    println!("Full domain evaluation took: {:?}", duration);

    // Verify correctness
    let eval_both = eval0.iter().zip(eval1.iter()).map(|(x, y)| x - y).collect::<Vec<RingVec>>();
    println!("Full evaluation: {:?}", eval_both);
}
