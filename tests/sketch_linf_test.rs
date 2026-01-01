use mosaic::{
    data_structures::ringvec::RingVec, fss::interval::IntervalFSSKey, fuzzy_match::{
        share_types::{DictionaryType, DistanceMetric, ShareMethod}, sketch_phase::SketchPhase, sketch_types::SketchConfig
    }, util::u128_to_bits_msb
};
use anyhow::Result;

const H1: usize = 5;
const H2: usize = 10;
const MAX_INPUT: u128 = 1u128 << H1 - 1;
const DELTA: u128 = 5;

fn init_sketch_config() -> Result<SketchConfig> {
    Ok(SketchConfig {
        h1: H1,
        h2: H2,
        q: 892270022585185806328177,
        delta: DELTA,
        d: 1,
        method: ShareMethod::FSS,
        metric: DistanceMetric::LInfinity,
        dictionary_type: DictionaryType::Unknown,
    })
}

fn generate_fss_key() -> Result<(IntervalFSSKey, IntervalFSSKey)> {
    let x = 10u128;
    let alpha = x.saturating_sub(DELTA);
    let beta = x.saturating_add(DELTA).min(MAX_INPUT);
    let alpha_bits = u128_to_bits_msb(alpha, H1);
    let beta_bits = u128_to_bits_msb(beta, H1);
    let modulus = 1u128 << H2;
    let a = RingVec::new(vec![1], modulus)?;
    let b = RingVec::new(vec![0], modulus)?;
    let c = RingVec::new(vec![1], modulus)?;
    let (fss_key0, fss_key1) = IntervalFSSKey::gen_interval_fss_key(&alpha_bits, &beta_bits, &a, &b, &c, modulus)?;
    Ok(
        (fss_key0, fss_key1)
    )
}

#[test]
fn sketch_interval_fss_test() -> Result<()> {
    // Generate evals for interval fss
    let (fss_key0, fss_key1) = generate_fss_key()?;
    let sketch_config = init_sketch_config()?;
    let sketch_phase = SketchPhase::new(sketch_config);
    Ok(())    
}