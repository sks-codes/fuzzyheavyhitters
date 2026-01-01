use mosaic::{
    data_structures::ringvec::RingVec, fss::interval::IntervalFSSKey, fuzzy_match::{
        share_phase::SharePhase, share_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod}, sketch_phase::SketchPhase, sketch_types::SketchConfig
    }, randomness::prg::PRG, util::u128_to_bits_msb
};
use anyhow::Result;

const H1: usize = 5;
const H2: usize = 10;
const X: u128 = 10;
const DELTA: u128 = 2;
const MAX_INPUT: u128 = 1u128 << H1 - 1;

fn sketch_phase_config(method: ShareMethod, metric: DistanceMetric, dictionary_type: DictionaryType) -> Result<SketchConfig> {
    Ok(SketchConfig {
        method,
        metric,
        dictionary_type,
        h1: H1,
        h2: H2,
        q: 892270022585185806328177,
        delta: DELTA,
        d: 1,
    })
}

fn share_phase_config(method: ShareMethod, metric: DistanceMetric, dictionary_type: DictionaryType) -> Result<ShareConfig> {
    Ok(
        ShareConfig {
            method,
            metric,
            dictionary_type,
            h1: H1,
            h2: H2,
            d: 1,
            sketch_modulus: 1u128 << H2,
            delta: DELTA,
        }
    )
}

#[test]
fn sketch_interval_fss_test() -> Result<()> {
    // Generate keys from share phase
    let share_cfg = share_phase_config(ShareMethod::FSS, DistanceMetric::LInfinity, DictionaryType::Known)?;
    let share_phase = SharePhase::new(share_cfg);
    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    // Init sketch phase and get sketch helper
    let sketch_cfg = sketch_phase_config(ShareMethod::FSS, DistanceMetric::LInfinity, DictionaryType::Known)?;

    let sketch_phase = SketchPhase::new(sketch_cfg);

    let sketch_helper = sketch_phase.get_sketch_helper()?;

    let seed = [0u8; 16];
    let mut prg0 = PRG::new(Some(&seed), 0);
    let sketch0 = sketch_phase.sketch(&range0, &sketch_helper, &mut prg0)?;
    println!("lmao");

    let mut prg1 = PRG::new(Some(&seed), 0);
    let sketch1 = sketch_phase.sketch(&range1, &sketch_helper, &mut prg1)?;

    // println!("{:?}", sketch0);

    Ok(())
}