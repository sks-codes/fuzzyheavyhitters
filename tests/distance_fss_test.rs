use anyhow::Result;
use mosaic::{
    fss::distance::{DistanceFSSEval, DistanceFSSKey},
    util::{bits_to_u128_msb, u128_to_bits_msb},
};

const ALPHA: u128 = 6;
const BETA: u128 = 26;
const X: u128 = 18;
const BIT_LENGTH: usize = 5;
const MODULUS: u128 = 1 << 20;
const DELTA: u128 = 8;

fn expected_distance(prefix: u128, level: usize, max_distance: u128, p: usize) -> u128 {
    let alpha_prefix = bits_to_u128_msb(&u128_to_bits_msb(ALPHA, BIT_LENGTH)[..level]);
    let beta_prefix = bits_to_u128_msb(&u128_to_bits_msb(BETA, BIT_LENGTH)[..level]);
    let x_prefix = bits_to_u128_msb(&u128_to_bits_msb(X, BIT_LENGTH)[..level]);
    if prefix < alpha_prefix || prefix > beta_prefix {
        return max_distance % MODULUS;
    }

    let remaining = BIT_LENGTH - level;
    let t_min = prefix << remaining;
    let t_max = (prefix << remaining) | ((1u128 << remaining) - 1);

    if prefix < x_prefix {
        debug_assert!(X >= t_max);
        (X - t_max).pow(p as u32) % MODULUS
    } else if prefix > x_prefix {
        debug_assert!(t_min >= X);
        (t_min - X).pow(p as u32) % MODULUS
    } else {
        0
    }
}

#[test]
fn distance_fss_expand_prefix_l1() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let x_bits = u128_to_bits_msb(X, BIT_LENGTH);
    let p = 1usize;
    let max_distance = DELTA.pow(p as u32) + 1;

    let (key0, key1) = DistanceFSSKey::gen_distance_fss_key(
        X,
        &x_bits,
        &alpha_bits,
        &beta_bits,
        DELTA,
        p,
        MODULUS,
    )?;

    let mut evals0: Vec<(Vec<bool>, DistanceFSSEval)> =
        vec![(Vec::new(), key0.init_eval(MODULUS)?)];
    let mut evals1: Vec<(Vec<bool>, DistanceFSSEval)> =
        vec![(Vec::new(), key1.init_eval(MODULUS)?)];

    for level in 1..BIT_LENGTH {
        let mut next0 = Vec::with_capacity(evals0.len() * 2);
        let mut next1 = Vec::with_capacity(evals1.len() * 2);

        for (prefix, eval) in evals0.iter() {
            let (left, right) = key0.expand_prefix(eval, MODULUS)?;
            let mut left_prefix = prefix.clone();
            left_prefix.push(false);
            let mut right_prefix = prefix.clone();
            right_prefix.push(true);
            next0.push((left_prefix, left));
            next0.push((right_prefix, right));
        }

        for (prefix, eval) in evals1.iter() {
            let (left, right) = key1.expand_prefix(eval, MODULUS)?;
            let mut left_prefix = prefix.clone();
            left_prefix.push(false);
            let mut right_prefix = prefix.clone();
            right_prefix.push(true);
            next1.push((left_prefix, left));
            next1.push((right_prefix, right));
        }

        evals0 = next0;
        evals1 = next1;

        let domain_size = 1usize << level;
        for i in 0..domain_size {
            let prefix_bits = &evals0[i].0;
            let prefix_val = bits_to_u128_msb(prefix_bits);
            let res0 = evals0[i]
                .1
                .eval(prefix_bits, BIT_LENGTH, MODULUS, p)?
                % MODULUS;
            let res1 = evals1[i]
                .1
                .eval(prefix_bits, BIT_LENGTH, MODULUS, p)?
                % MODULUS;
            let res = (res0 + MODULUS - res1) % MODULUS;
            let expected = expected_distance(prefix_val, level, max_distance, p);
            assert_eq!(
                res, expected,
                "[DistanceFSS] Fail at level {level}, position {i}, prefix {prefix_val}, expected {expected}, got {res}"
            );
        }
    }
    Ok(())
}

#[test]
fn distance_fss_expand_prefix_l2() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let x_bits = u128_to_bits_msb(X, BIT_LENGTH);
    let p = 2usize;
    let max_distance = DELTA.pow(p as u32) + 1;

    let (key0, key1) = DistanceFSSKey::gen_distance_fss_key(
        X,
        &x_bits,
        &alpha_bits,
        &beta_bits,
        DELTA,
        p,
        MODULUS,
    )?;

    let mut evals0: Vec<(Vec<bool>, DistanceFSSEval)> =
        vec![(Vec::new(), key0.init_eval(MODULUS)?)];
    let mut evals1: Vec<(Vec<bool>, DistanceFSSEval)> =
        vec![(Vec::new(), key1.init_eval(MODULUS)?)];

    for level in 1..BIT_LENGTH {
        let mut next0 = Vec::with_capacity(evals0.len() * 2);
        let mut next1 = Vec::with_capacity(evals1.len() * 2);

        for (prefix, eval) in evals0.iter() {
            let (left, right) = key0.expand_prefix(eval, MODULUS)?;
            let mut left_prefix = prefix.clone();
            left_prefix.push(false);
            let mut right_prefix = prefix.clone();
            right_prefix.push(true);
            next0.push((left_prefix, left));
            next0.push((right_prefix, right));
        }

        for (prefix, eval) in evals1.iter() {
            let (left, right) = key1.expand_prefix(eval, MODULUS)?;
            let mut left_prefix = prefix.clone();
            left_prefix.push(false);
            let mut right_prefix = prefix.clone();
            right_prefix.push(true);
            next1.push((left_prefix, left));
            next1.push((right_prefix, right));
        }

        evals0 = next0;
        evals1 = next1;

        let domain_size = 1usize << level;
        for i in 0..domain_size {
            let prefix_bits = &evals0[i].0;
            let prefix_val = bits_to_u128_msb(prefix_bits);
            let res0 = evals0[i]
                .1
                .eval(prefix_bits, BIT_LENGTH, MODULUS, p)?
                % MODULUS;
            let res1 = evals1[i]
                .1
                .eval(prefix_bits, BIT_LENGTH, MODULUS, p)?
                % MODULUS;
            let res = (res0 + MODULUS - res1) % MODULUS;
            let expected = expected_distance(prefix_val, level, max_distance, p);
            assert_eq!(
                res, expected,
                "[DistanceFSS] Fail at level {level}, position {i}, prefix {prefix_val}, expected {expected}, got {res}"
            );
        }
    }
    Ok(())
}


#[test]
fn distance_fss_eval_l1() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let x_bits = u128_to_bits_msb(X, BIT_LENGTH);
    let p = 1usize;
    let max_distance = DELTA.pow(p as u32) + 1;

    let (key0, key1) = DistanceFSSKey::gen_distance_fss_key(
        X,
        &x_bits,
        &alpha_bits,
        &beta_bits,
        DELTA,
        p,
        MODULUS,
    )?;

    for t in 0..(1 << BIT_LENGTH) {
        let t_bits = u128_to_bits_msb(t as u128, BIT_LENGTH);
        let res0 = key0.eval_distance_fss(&t_bits, BIT_LENGTH, MODULUS)?;
        let res1 = key1.eval_distance_fss(&t_bits, BIT_LENGTH, MODULUS)?;
        let res = (res0 + MODULUS - res1 % MODULUS) % MODULUS;

        let t_u128 = t as u128;
        let expected = if t_u128 < ALPHA || t_u128 > BETA {
            max_distance % MODULUS
        } else if t_u128 < X {
            (X - t_u128).pow(p as u32) % MODULUS
        } else if t_u128 > X {
            (t_u128 - X).pow(p as u32) % MODULUS
        } else {
            0
        };

        assert_eq!(
            res, expected,
            "[DistanceFSS] Fail at position {t}, expected {expected}, got {res}"
        );
    }

    Ok(())
}

#[test]
fn distance_fss_eval_l2() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let x_bits = u128_to_bits_msb(X, BIT_LENGTH);
    let p = 2usize;
    let max_distance = DELTA.pow(p as u32) + 1;

    let (key0, key1) = DistanceFSSKey::gen_distance_fss_key(
        X,
        &x_bits,
        &alpha_bits,
        &beta_bits,
        DELTA,
        p,
        MODULUS,
    )?;

    for t in 0..(1 << BIT_LENGTH) {
        let t_bits = u128_to_bits_msb(t as u128, BIT_LENGTH);
        let res0 = key0.eval_distance_fss(&t_bits, BIT_LENGTH, MODULUS)?;
        let res1 = key1.eval_distance_fss(&t_bits, BIT_LENGTH, MODULUS)?;
        let res = (res0 + MODULUS - res1 % MODULUS) % MODULUS;

        let t_u128 = t as u128;
        let expected = if t_u128 < ALPHA || t_u128 > BETA {
            max_distance % MODULUS
        } else if t_u128 < X {
            (X - t_u128).pow(p as u32) % MODULUS
        } else if t_u128 > X {
            (t_u128 - X).pow(p as u32) % MODULUS
        } else {
            0
        };

        assert_eq!(
            res, expected,
            "[DistanceFSS] Fail at position {t}, expected {expected}, got {res}"
        );
    }

    Ok(())
}


#[test]
fn distance_fss_serialization() -> Result<()> {
    let alpha_bits = u128_to_bits_msb(ALPHA, BIT_LENGTH);
    let beta_bits = u128_to_bits_msb(BETA, BIT_LENGTH);
    let x_bits = u128_to_bits_msb(X, BIT_LENGTH);

    let (key0, key1) = DistanceFSSKey::gen_distance_fss_key(
        X,
        &x_bits,
        &alpha_bits,
        &beta_bits,
        DELTA,
        1,
        MODULUS,
    )?;

    let key0_bytes = key0.to_bytes()?;
    let (key0_cmp, size0) = DistanceFSSKey::from_bytes(&key0_bytes, MODULUS)?;
    assert_eq!(size0, key0_bytes.len(), "Wrong deserialization length for distance fss key0!");
    assert_eq!(key0_cmp, key0, "Wrong serialization round-trip for distance fss key0!");

    let key1_bytes = key1.to_bytes()?;
    let (key1_cmp, size1) = DistanceFSSKey::from_bytes(&key1_bytes, MODULUS)?;
    assert_eq!(size1, key1_bytes.len(), "Wrong deserialization length for distance fss key1!");
    assert_eq!(key1_cmp, key1, "Wrong serialization round-trip for distance fss key1!");

    Ok(())
}
