use mosaic::{
    fuzzy_match::{
        share_phase::SharePhase,
        share_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod},
        shared_range::SharedRange,
        sketch_phase::SketchPhase,
        sketch_types::{SketchConfig, SketchValues},
    },
    randomness::prg::PRG,
};
use anyhow::{anyhow, Result};

const H1: usize = 5;
const H2: usize = 10;
const X: u128 = 10;
const DELTA: u128 = 2;

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

#[test]
fn sketch_distance_fss_test() -> Result<()> {
    let p: u32 = 2;
    let share_cfg =
        share_phase_config(ShareMethod::FSS, DistanceMetric::Lp { p }, DictionaryType::Known)?;
    let share_phase = SharePhase::new(share_cfg);
    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let sketch_cfg =
        sketch_phase_config(ShareMethod::FSS, DistanceMetric::Lp { p }, DictionaryType::Known)?;
    let sketch_phase = SketchPhase::new(sketch_cfg);
    let sketch_helper = sketch_phase.get_sketch_helper()?;

    let seed = [0u8; 16];
    let mut prg0 = PRG::new(Some(&seed), 0);
    let sketch0 = sketch_phase.sketch(&range0, &sketch_helper, &mut prg0)?;

    let mut prg1 = PRG::new(Some(&seed), 0);
    let sketch1 = sketch_phase.sketch(&range1, &sketch_helper, &mut prg1)?;

    let expected_payload_len = p as usize + 1;
    let expected_levels = H1 - 1;
    let check_payload = |name: &str, left: &SketchValues, right: &SketchValues| -> Result<()> {
        let (len_left, last_left, consistency_left) = match left {
            SketchValues::DcfPayload {
                length,
                last_layer_consistency,
                consistency,
            } => (*length, last_layer_consistency, consistency),
            _ => return Err(anyhow!("Wrong sketch type for left {}", name)),
        };
        let (len_right, last_right, consistency_right) = match right {
            SketchValues::DcfPayload {
                length,
                last_layer_consistency,
                consistency,
            } => (*length, last_layer_consistency, consistency),
            _ => return Err(anyhow!("Wrong sketch type for right {}", name)),
        };

        assert_eq!(
            len_left, len_right,
            "Length mismatch for payload {}: left {} right {}",
            name, len_left, len_right
        );
        assert_eq!(
            len_left, expected_payload_len,
            "Unexpected payload length for {}: got {}, expected {}",
            name, len_left, expected_payload_len
        );

        assert_eq!(
            consistency_left.len(),
            consistency_right.len(),
            "Consistency level mismatch for {}",
            name
        );
        assert_eq!(
            consistency_left.len(),
            expected_levels,
            "Unexpected number of consistency levels for {}: got {}, expected {}",
            name,
            consistency_left.len(),
            expected_levels
        );
        for (level, (l_vec, r_vec)) in consistency_left
            .iter()
            .zip(consistency_right.iter())
            .enumerate()
        {
            assert_eq!(
                l_vec.len(),
                r_vec.len(),
                "Consistency length mismatch for {} at level {}",
                name,
                level + 1
            );
            assert_eq!(
                l_vec.len(),
                expected_payload_len,
                "Unexpected payload count for {} at level {}: got {}, expected {}",
                name,
                level + 1,
                l_vec.len(),
                expected_payload_len
            );
        }

        // Check last layer
        for (idx, (z_l, z_r)) in
            last_left.iter().zip(last_right.iter()).enumerate()
        {
            let z = *z_l - *z_r;
            assert_eq!(z.value(), 0, "Failed payload consistency of last level at {}, idx {}", name, idx);
        }

        // Check consistency between layers
        let mut level = 1;
        for (consistency_l, consistency_r) in consistency_left.iter().zip(consistency_right.iter()) {
            let mut idx = 0;
            for ((z0_ast, z0_bullet), (z1_ast, z1_bullet)) in consistency_l.iter().zip(consistency_r.iter()) {
                let z_ast = *z0_ast - *z1_ast;
                let z_bullet = *z0_bullet - *z1_bullet;
                let z = z_ast * z_bullet;
                assert_eq!(z.value(), 0, "Failed consistency between levels at {}, level {}, idx {}", name, level, idx);
                idx += 1;
            }
            level += 1;
        }
        println!("Passed all checks for {}!", name);

        Ok(())
    };

    for (sv0, sv1) in sketch0.iter().zip(sketch1.iter()) {
        let (p0, ldcf0_0, ldcf1_0, rdcf0_0, rdcf1_0, reference_dpf0_case0, reference_dpf0_case1) = match sv0 {
            SketchValues::Lp {
                p,
                ldcf0,
                ldcf1,
                rdcf0,
                rdcf1,
                reference_dpf_case0,
                reference_dpf_case1,
            } => (*p, &**ldcf0, &**ldcf1, &**rdcf0, &**rdcf1, reference_dpf_case0, reference_dpf_case1),
            _ => return Err(anyhow!("Wrong returned sketch value for sketch0")),
        };

        let (p1, ldcf0_1, ldcf1_1, rdcf0_1, rdcf1_1, reference_dpf1_case0, reference_dpf1_case1) = match sv1 {
            SketchValues::Lp {
                p,
                ldcf0,
                ldcf1,
                rdcf0,
                rdcf1,
                reference_dpf_case0,
                reference_dpf_case1,
            } => (*p, &**ldcf0, &**ldcf1, &**rdcf0, &**rdcf1, reference_dpf_case0, reference_dpf_case1),
            _ => return Err(anyhow!("Wrong returned sketch value for sketch1")),
        };

        // Sketch reference dpf 
        let z_ast = reference_dpf0_case0.0 - reference_dpf1_case0.0;
        let z2_ast = reference_dpf0_case0.1 - reference_dpf1_case0.1;
        let z_case0 = z_ast * z_ast - z2_ast;

        let z_bullet= reference_dpf0_case1.0 - reference_dpf1_case1.0;
        let z2_bullet= reference_dpf0_case1.1 - reference_dpf1_case1.1;
        let z_case1 = z_bullet * z_bullet - z2_bullet;

        let z_dpf = z_case0 * z_case1;

        assert_eq!(z_dpf.value(), 0, "Wrong sketch for reference dpf");

        // Check consistency of distance metric between sketch and share phase
        assert_eq!(p0, p as usize, "Sketch p mismatch for sketch0");
        assert_eq!(p1, p as usize, "Sketch p mismatch for sketch1");

        // Check each ldcf/rdcf
        if let (SharedRange::DistanceFSS { keys: keys0, .. }, SharedRange::DistanceFSS { keys: keys1, .. }) =
            (&range0, &range1)
        {
            let modulus = 1u128 << H2;
            let last0 = keys0[0]
                .left_fss1()
                .full_domain_incremental_eval(modulus, H1)
                .map_err(|e| anyhow!("ldcf1 full eval0 failed: {}", e))?;
            let last1 = keys1[0]
                .left_fss1()
                .full_domain_incremental_eval(modulus, H1)
                .map_err(|e| anyhow!("ldcf1 full eval1 failed: {}", e))?;
            let layer0 = &last0[H1];
            let layer1 = &last1[H1];
            let reconstructed: Vec<Vec<u128>> = layer0
                .iter()
                .zip(layer1.iter())
                .map(|(a, b)| {
                    a.values()
                        .iter()
                        .zip(b.values().iter())
                        .map(|(x0, x1)| (x0 + modulus - (x1 % modulus)) % modulus)
                        .collect()
                })
                .collect();
            println!("ldcf1 last layer reconstructed (payloads): {:?}", reconstructed);
        }
        check_payload("ldcf0", ldcf0_0, ldcf0_1)?;
        check_payload("ldcf1", ldcf1_0, ldcf1_1)?;
        check_payload("rdcf0", rdcf0_0, rdcf0_1)?;
        check_payload("rdcf1", rdcf1_0, rdcf1_1)?;
    }

    Ok(())
}
