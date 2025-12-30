use clap::Parser;
use mosaic::{
    data_structures::ringvec::RingVec, fss::interval::IntervalFSSKey, util::u128_to_bits_msb,
};
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(long)]
    alpha: u128,
    #[arg(long)]
    beta: u128,
    #[arg(long)]
    domain_size: usize,
}

fn main() {
    let args = Args::parse();

    let alpha = args.alpha;
    let alpha_bits = u128_to_bits_msb(alpha, args.domain_size);
    let beta = args.beta;
    let beta_bits = u128_to_bits_msb(beta, args.domain_size);
    let domain_size = args.domain_size;

    // Hardcode the modulus and payload for benchmarking
    let modulus = 1u128 << 20;
    let a = RingVec::new(vec![5u128; 2], modulus).expect("Failed to create RingVec");
    let b = RingVec::new(vec![3u128; 2], modulus).expect("Failed to create RingVec");
    let c = RingVec::new(vec![10u128; 2], modulus).expect("Failed to create RingVec");

    let (key0, key1) = IntervalFSSKey::gen_interval_fss_key(
        &alpha_bits,
        &beta_bits,
        &a,
        &b,
        &c,
        modulus,
    )
    .expect("Failed to generate IntervalFSS keys");

    // Benchmark Full Domain Evaluation
    let start = Instant::now();
    let eval0 = key0
        .full_domain_eval(modulus, domain_size)
        .expect("Failed to eval key0");
    let eval1 = key1
        .full_domain_eval(modulus, domain_size)
        .expect("Failed to eval key1");
    let duration = start.elapsed();
    println!("Full domain evaluation took: {:?}", duration);

    // Verify correctness
    let eval_both = eval0
        .iter()
        .zip(eval1.iter())
        .map(|(x, y)| x - y)
        .collect::<Vec<RingVec>>();
    for i in 0..(1 << domain_size) {
        if i < alpha {
            assert_eq!(eval_both[i as usize], a);
        } else if i <= beta {
            assert_eq!(eval_both[i as usize], b);
        } else {
            assert_eq!(eval_both[i as usize], c);
        }
    }
}
