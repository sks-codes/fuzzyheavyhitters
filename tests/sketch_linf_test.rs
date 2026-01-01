use mosaic::{
    data_structures::ringvec::RingVec, fss::interval::IntervalFSSKey, 
    fuzzy_match::{
        share_phase::SharePhase, 
        share_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod}, 
        shared_range::SharedRange,
        sketch_phase::SketchPhase, 
        sketch_types::{SketchConfig, SketchValues},
    }, randomness::prg::PRG, util::u128_to_bits_msb
};
use anyhow::{anyhow, Result};

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

    let mut prg1 = PRG::new(Some(&seed), 0);
    let sketch1 = sketch_phase.sketch(&range1, &sketch_helper, &mut prg1)?;

    for (sketch_value0, sketch_value1) in sketch0.iter().zip(sketch1.iter()) {
        let (ldcf0, rdcf0, consistency0) = match sketch_value0 {
            SketchValues::Linf { ldcf, rdcf, consistency } => 
                (&**ldcf, &**rdcf, consistency), 
            _ => return Err(anyhow!("Wrong returned sketch value for sketch_value0")),
        };
        let (ldcf1, rdcf1, consistency1) = match sketch_value1 {
            SketchValues::Linf { ldcf, rdcf, consistency } => 
                (&**ldcf, &**rdcf, consistency), 
            _ => return Err(anyhow!("Wrong returned sketch value for sketch_value1")),
        };

        // Check if ldcf sketch is correct
        let (last_layer_ldcf0_case0, last_layer_ldcf0_case1, consistency_ldcf0) = match ldcf0 {
            SketchValues::Dcf { last_layer_case0, last_layer_case1, consistency } => 
                (last_layer_case0, last_layer_case1, consistency),
            _ => return Err(anyhow!("Wrong returned sketch value for ldcf0")),
        };
        let (last_layer_ldcf1_case0, last_layer_ldcf1_case1, consistency_ldcf1) = match ldcf1 {
            SketchValues::Dcf { last_layer_case0, last_layer_case1, consistency } => 
                (last_layer_case0, last_layer_case1, consistency),
            _ => return Err(anyhow!("Wrong returned sketch value for ldcf1")),
        };

        let z_ast = last_layer_ldcf0_case0.0 - last_layer_ldcf1_case0.0;

        // println!("z_ast: {:?}", z_ast);

        let z2_ast = last_layer_ldcf0_case0.1 - last_layer_ldcf1_case0.1;

        let z_bullet= last_layer_ldcf0_case1.0 - last_layer_ldcf1_case1.1;
        let z2_bullet= last_layer_ldcf0_case1.1 - last_layer_ldcf1_case1.1;

        let z_case0 = z_ast * z_ast - z2_ast;
        let z_case1 = z_bullet * z_bullet - z2_bullet;
        let z_ldcf = z_case0 * z_case1;

        assert_eq!(z_ldcf.value(), 0, "Wrong sketch for last layer ldcf");

        let mut layer = 1usize;
        for ((z0_ldcf0, z1_ldcf0), (z0_ldcf1, z1_ldcf1)) in consistency_ldcf0.iter().zip(consistency_ldcf1.iter()) {
            let z0 = *z0_ldcf0 - *z0_ldcf1;
            let z1 = *z1_ldcf0 - *z1_ldcf1;
            let z = z0 * z1;
            assert_eq!(z.value(), 0, "Wrong consistency check at level {} of ldcf", layer);
            layer += 1;
        }

        // Check if rdcf sketch is correct
        let (last_layer_rdcf0_case0, last_layer_rdcf0_case1, consistency_rdcf0) = match rdcf0 {
            SketchValues::Dcf { last_layer_case0, last_layer_case1, consistency } => 
                (last_layer_case0, last_layer_case1, consistency),
            _ => return Err(anyhow!("Wrong returned sketch value for rdcf0")),
        };
        let (last_layer_rdcf1_case0, last_layer_rdcf1_case1, consistency_rdcf1) = match rdcf1 {
            SketchValues::Dcf { last_layer_case0, last_layer_case1, consistency } => 
                (last_layer_case0, last_layer_case1, consistency),
            _ => return Err(anyhow!("Wrong returned sketch value for rdcf1")),
        };

        let z_ast = last_layer_rdcf0_case0.0 - last_layer_rdcf1_case0.0;

        // println!("z_ast: {:?}", z_ast);

        let z2_ast = last_layer_rdcf0_case0.1 - last_layer_rdcf1_case0.1;

        let z_bullet= last_layer_rdcf0_case1.0 - last_layer_rdcf1_case1.1;
        let z2_bullet= last_layer_rdcf0_case1.1 - last_layer_rdcf1_case1.1;

        let z_case0 = z_ast * z_ast - z2_ast;
        let z_case1 = z_bullet * z_bullet - z2_bullet;
        let z_rdcf = z_case0 * z_case1;

        assert_eq!(z_rdcf.value(), 0, "Wrong sketch for last layer rdcf");

        let mut layer = 1usize;
        for ((z0_rdcf0, z1_rdcf0), (z0_rdcf1, z1_rdcf1)) in consistency_rdcf0.iter().zip(consistency_rdcf1.iter()) {
            let z0 = *z0_rdcf0 - *z0_rdcf1;
            let z1 = *z1_rdcf0 - *z1_rdcf1;
            let z = z0 * z1;
            assert_eq!(z.value(), 0, "Wrong consistency check at level {} of rdcf", layer);
            layer += 1;
        }


        // println!("z_case0: {:?}", z_case0);
        // println!("z_case1: {:?}", z_case1);

        // Sketch shift consistency
        let consistency = *consistency0 - *consistency1;
        assert_eq!(consistency.value(), 0, "Wrong shift consistency");
        println!("Checked consistency shift!");
    }

    Ok(())
}

/*
[Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 97 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 649 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 355 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 0 }]
[Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 97 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 649 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 355 }, Modp { ctx: BarrettCtx { m: 892270022585185806328177, mu: U256 { hi: 381367028262401, lo: 278199558304884313758889181833495774535 } }, v: 0 }]
 */