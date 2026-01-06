use anyhow::Result;
use mosaic::{
    fuzzy_match::{
        share_phase::SharePhase,
        share_phase_types::{DictionaryType, DistanceMetric, ShareConfig, ShareMethod},
    },
    util::{bits_to_u128_msb, u128_to_bits_msb},
};

const H1: usize = 5;
const H2: usize = 10;
const DELTA: u128 = 2;
const X: u128 = 10;
const MODULUS: u128 = 1u128 << H2;
const MAX_DISTANCE_L1: u128 = (DELTA + 1) & ((1u128 << H2) - 1);
const MAX_DISTANCE_L2: u128 = (DELTA * DELTA + 1) & ((1u128 << H2) - 1);

fn make_config(method: ShareMethod, metric: DistanceMetric, dictionary_type: DictionaryType) -> ShareConfig {
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
}

fn reconstruct(modulus: u128, share0: u128, share1: u128) -> u128 {
    (share0 + modulus - (share1 % modulus)) % modulus
}

#[test]
fn share_phase_linf_okvs_known_test() -> Result<()> {
    let cfg = make_config(ShareMethod::OKVS, DistanceMetric::LInfinity, DictionaryType::Known);
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha = X.saturating_sub(DELTA);
    let beta = (X + DELTA).min((1u128 << H1) - 1);
    let domain_size = 1usize << H1;

    for t in 0..domain_size {
        let bits = u128_to_bits_msb(t as u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        if (t as u128) < alpha || (t as u128) > beta {
            assert_ne!(
                share0, share1,
                "Shares should differ outside the range, t = {t}, got share0 = {share0}, share1 = {share1}"
            );
        } else {
            assert_eq!(
                share0, share1,
                "Shares should match for in-range t={}, alpha={}, beta={}", t, alpha, beta
            );
        }
    }

    Ok(())
}

#[test]
fn share_phase_linf_fss_known_test() -> Result<()> {
    let cfg = make_config(ShareMethod::FSS, DistanceMetric::LInfinity, DictionaryType::Known);
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let (alpha, beta) = (
        X.saturating_sub(DELTA),
        (X + DELTA).min((1u128 << H1) - 1),
    );
    let domain_size = 1usize << H1;

    for t in 0..domain_size {
        let t_u128 = t as u128;
        let expect = if t_u128 >= alpha && t_u128 <= beta { 0u128 } else { 1u128 };
        let bits = u128_to_bits_msb(t_u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        let res = reconstruct(MODULUS, share0, share1);
        assert_eq!(
            res, expect,
            "Mismatch at t={t_u128}: expected {expect}, got {res} (alpha={}, beta={})",
            alpha, beta
        );
    }

    Ok(())
}

#[test]
fn share_phase_linf_okvs_unknown_test() -> Result<()> {
    let cfg = make_config(ShareMethod::OKVS, DistanceMetric::LInfinity, DictionaryType::Unknown);
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let inside = (prefix as u128) >= prefix_alpha && (prefix as u128) <= prefix_beta;
            if inside {
                assert_eq!(
                    share0, share1,
                    "Shares should match for in-range prefix {} (len {}) alpha_prefix {} beta_prefix {}",
                    prefix, len, prefix_alpha, prefix_beta
                );
            } else {
                assert_ne!(
                    share0, share1,
                    "Shares should differ outside the range, t = {}, got share0 = {}, share1 = {}",
                    prefix, share0, share1
                );
            }
        }
    }

    Ok(())
}

#[test]
fn share_phase_linf_fss_unknown_test() -> Result<()> {
    let cfg = make_config(ShareMethod::FSS, DistanceMetric::LInfinity, DictionaryType::Unknown);
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let res = reconstruct(MODULUS, share0, share1);
            let expect = if (prefix as u128) >= prefix_alpha && (prefix as u128) <= prefix_beta {
                0u128
            } else {
                1u128
            };
            assert_eq!(
                res, expect,
                "Mismatch at prefix={} len {}: expected {}, got {} (alpha_prefix={}, beta_prefix={})",
                prefix, len, expect, res, prefix_alpha, prefix_beta
            );
        }
    }

    Ok(())
}

fn abs_diff_pow(distance: u128, p: u32) -> u128 {
    distance.pow(p)
}

fn expected_prefix_distance_lp(
    prefix: u128,
    len: usize,
    alpha_bits: &[bool],
    beta_bits: &[bool],
    x_bits: &[bool],
    max_distance: u128,
    p: u32,
) -> u128 {
    let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
    let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
    if prefix < prefix_alpha || prefix > prefix_beta {
        return max_distance;
    }
    let prefix_x = bits_to_u128_msb(&x_bits[..len]);
    let remaining = H1 - len;
    let t_min = prefix << remaining;
    let t_max = (prefix << remaining) | ((1u128 << remaining) - 1);
    let modulus_mask = (1u128 << H2) - 1;
    if prefix < prefix_x {
        (X - t_max).pow(p) & modulus_mask
    } else if prefix > prefix_x {
        (t_min - X).pow(p) & modulus_mask
    } else {
        0
    }
}

#[test]
fn share_phase_l1_okvs_known_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::OKVS,
        DistanceMetric::Lp { p: 1 },
        DictionaryType::Known,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha = X.saturating_sub(DELTA);
    let beta = (X + DELTA).min((1u128 << H1) - 1);
    let domain_size = 1usize << H1;
    let modulus_mask = (1u128 << H2) - 1;

    for t in 0..domain_size {
        let t_u128 = t as u128;
        if t_u128 < alpha || t_u128 > beta {
            continue; // Outside interval: OKVS known only encodes in-range points
        }
        let expect =
            abs_diff_pow(if t_u128 >= X { t_u128 - X } else { X - t_u128 }, 1) & modulus_mask;
        let bits = u128_to_bits_msb(t_u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        let res = (share0 + share1) & modulus_mask;
        assert_eq!(
            res, expect,
            "Mismatch at t={t_u128}: expected {expect}, got {res} (alpha={}, beta={})",
            alpha, beta
        );
    }

    Ok(())
}

#[test]
fn share_phase_l1_fss_known_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::FSS,
        DistanceMetric::Lp { p: 1 },
        DictionaryType::Known,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha = X.saturating_sub(DELTA);
    let beta = (X + DELTA).min((1u128 << H1) - 1);
    let max_distance = MAX_DISTANCE_L1;
    let domain_size = 1usize << H1;

    for t in 0..domain_size {
        let t_u128 = t as u128;
        let expect = if t_u128 < alpha || t_u128 > beta {
            max_distance
        } else if t_u128 >= X {
            (t_u128 - X) & ((1u128 << H2) - 1)
        } else {
            (X - t_u128) & ((1u128 << H2) - 1)
        };
        let bits = u128_to_bits_msb(t_u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        let res = (share0 + share1) % MODULUS;
        assert_eq!(
            res, expect,
            "Share mismatch at t={t_u128}: expected {expect}, got {res}"
        );
    }

    Ok(())
}

#[test]
fn share_phase_l1_okvs_unknown_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::OKVS,
        DistanceMetric::Lp { p: 1 },
        DictionaryType::Unknown,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);
    let x_bits = u128_to_bits_msb(X, H1);
    let modulus_mask = (1u128 << H2) - 1;

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            if (prefix as u128) < prefix_alpha || (prefix as u128) > prefix_beta {
                continue; // Outside interval: OKVS unknown can be random, skip
            }
            let expect = expected_prefix_distance_lp(
                prefix as u128,
                len,
                &alpha_bits,
                &beta_bits,
                &x_bits,
                MAX_DISTANCE_L1,
                1,
            ) & modulus_mask;
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let res = (share0 + share1) & modulus_mask;
            assert_eq!(
                res, expect,
                "Mismatch at prefix={} len {}: expected {}, got {} (alpha_prefix={}, beta_prefix={})",
                prefix, len, expect, res, prefix_alpha, prefix_beta
            );
        }
    }

    Ok(())
}

#[test]
fn share_phase_l1_fss_unknown_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::FSS,
        DistanceMetric::Lp { p: 1 },
        DictionaryType::Unknown,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);
    let x_bits = u128_to_bits_msb(X, H1);

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            let expect = expected_prefix_distance_lp(
                prefix as u128,
                len,
                &alpha_bits,
                &beta_bits,
                &x_bits,
                MAX_DISTANCE_L1,
                1,
            );
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let res = (share0 + share1) % MODULUS;
            assert_eq!(
                res, expect % MODULUS,
                "Share mismatch at prefix={} len {}: expected {}, got {} (alpha_prefix={}, beta_prefix={})",
                prefix, len, expect, res, prefix_alpha, prefix_beta
            );
        }
    }

    Ok(())
}

#[test]
fn share_phase_l2_okvs_known_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::OKVS,
        DistanceMetric::Lp { p: 2 },
        DictionaryType::Known,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha = X.saturating_sub(DELTA);
    let beta = (X + DELTA).min((1u128 << H1) - 1);
    let domain_size = 1usize << H1;
    let modulus_mask = (1u128 << H2) - 1;

    for t in 0..domain_size {
        let t_u128 = t as u128;
        if t_u128 < alpha || t_u128 > beta {
            continue;
        }
        let expect =
            abs_diff_pow(if t_u128 >= X { t_u128 - X } else { X - t_u128 }, 2) & modulus_mask;
        let bits = u128_to_bits_msb(t_u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        let res = (share0 + share1) & modulus_mask;
        assert_eq!(
            res, expect,
            "Mismatch at t={t_u128}: expected {expect}, got {res} (alpha={}, beta={})",
            alpha, beta
        );
    }

    Ok(())
}

#[test]
fn share_phase_l2_fss_known_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::FSS,
        DistanceMetric::Lp { p: 2 },
        DictionaryType::Known,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha = X.saturating_sub(DELTA);
    let beta = (X + DELTA).min((1u128 << H1) - 1);
    let max_distance = MAX_DISTANCE_L2;
    let domain_size = 1usize << H1;

    for t in 0..domain_size {
        let t_u128 = t as u128;
        let expect = if t_u128 < alpha || t_u128 > beta {
            max_distance
        } else if t_u128 >= X {
            abs_diff_pow(t_u128 - X, 2) & ((1u128 << H2) - 1)
        } else {
            abs_diff_pow(X - t_u128, 2) & ((1u128 << H2) - 1)
        };
        let bits = u128_to_bits_msb(t_u128, H1);
        let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
        let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
        let res = (share0 + share1) % MODULUS;
        assert_eq!(
            res, expect,
            "Share mismatch at t={t_u128}: expected {expect}, got {res}"
        );
    }

    Ok(())
}

#[test]
fn share_phase_l2_okvs_unknown_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::OKVS,
        DistanceMetric::Lp { p: 2 },
        DictionaryType::Unknown,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);
    let x_bits = u128_to_bits_msb(X, H1);
    let modulus_mask = (1u128 << H2) - 1;

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            if (prefix as u128) < prefix_alpha || (prefix as u128) > prefix_beta {
                continue;
            }
            let expect = expected_prefix_distance_lp(
                prefix as u128,
                len,
                &alpha_bits,
                &beta_bits,
                &x_bits,
                MAX_DISTANCE_L2,
                2,
            ) & modulus_mask;
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let res = (share0 + share1) & modulus_mask;
            assert_eq!(
                res, expect,
                "Mismatch at prefix={} len {}: expected {}, got {} (alpha_prefix={}, beta_prefix={})",
                prefix, len, expect, res, prefix_alpha, prefix_beta
            );
        }
    }

    Ok(())
}

#[test]
fn share_phase_l2_fss_unknown_test() -> Result<()> {
    let cfg = make_config(
        ShareMethod::FSS,
        DistanceMetric::Lp { p: 2 },
        DictionaryType::Unknown,
    );
    let share_phase = SharePhase::new(cfg);

    let (range0, range1) = share_phase.share_range(&[X], DELTA)?;

    let alpha_bits = u128_to_bits_msb(X.saturating_sub(DELTA), H1);
    let beta_bits = u128_to_bits_msb((X + DELTA).min((1u128 << H1) - 1), H1);
    let x_bits = u128_to_bits_msb(X, H1);

    for len in 1..=H1 {
        let prefix_alpha = bits_to_u128_msb(&alpha_bits[..len]);
        let prefix_beta = bits_to_u128_msb(&beta_bits[..len]);
        let domain_size = 1usize << len;
        for prefix in 0..domain_size {
            let expect = expected_prefix_distance_lp(
                prefix as u128,
                len,
                &alpha_bits,
                &beta_bits,
                &x_bits,
                MAX_DISTANCE_L2,
                2,
            );
            let bits = u128_to_bits_msb(prefix as u128, len);
            let share0 = share_phase.evaluate_at_single_dimension(&range0, &bits, 0)?;
            let share1 = share_phase.evaluate_at_single_dimension(&range1, &bits, 0)?;
            let res = (share0 + share1) % MODULUS;
            assert_eq!(
                res, expect % MODULUS,
                "Share mismatch at prefix={} len {}: expected {}, got {} (alpha_prefix={}, beta_prefix={})",
                prefix, len, expect, res, prefix_alpha, prefix_beta
            );
        }
    }

    Ok(())
}
