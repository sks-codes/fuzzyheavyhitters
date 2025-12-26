use core::cmp::Ordering;
use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

const MAX_MOD: u128 = 1u128 << 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct U256 {
    hi: u128,
    lo: u128,
}

impl U256 {
    #[inline] 
    fn sub(self, other: U256) -> (U256, bool) {
        // Subtracting two U256, returns b2 = true if self < other
        let (lo, b1) = self.lo.overflowing_sub(other.lo);
        let (hi, b2) = self.hi.overflowing_sub(other.hi + (b1 as u128));
        (U256 {hi, lo}, b2) 
    }

    #[inline]
    #[allow(dead_code)]
    fn add(self, other: U256) -> U256 {
        // Compute the sum mod 2^256. So if there is overflowing, it wraps around.
        // Should be fine for this particular interest of u128 modulo
        let (lo, c1) = self.lo.overflowing_add(other.lo);
        let hi = self.hi.wrapping_add(other.hi).wrapping_add(c1 as u128);
        U256 {hi, lo}
    }

    #[allow(dead_code)]
    fn add_u128(self, other: u128) -> U256 {
        let (lo, c0) = self.lo.overflowing_add(other);
        let hi = self.hi.wrapping_add(c0 as u128);
        U256 {hi, lo}
    }

    #[inline]
    fn cmp_u128(&self, m: u128) -> core::cmp::Ordering {
        if self.hi != 0 {
            core::cmp::Ordering::Greater
        } else {
            self.lo.cmp(&m)
        }
    }

    #[inline]
    fn to_u128_checked(&self) -> u128 {
        assert!(self.hi == 0);
        self.lo
    }
}

impl PartialEq<u128> for U256 {
    #[inline]
    fn eq(&self, other: &u128) -> bool {
        self.hi == 0 && self.lo == *other
    }
}

impl PartialOrd<u128> for U256 {
    #[inline]
    fn partial_cmp(&self, other: &u128) -> Option<Ordering> {
        Some(self.cmp_u128(*other))
    }
}

impl PartialOrd for U256 {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.hi.cmp(&other.hi) {
            Ordering::Equal => Some(self.lo.cmp(&other.lo)),
            ord => Some(ord),
        }
    }
}

impl Ord for U256 {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        match self.hi.cmp(&other.hi) {
            Ordering::Equal => self.lo.cmp(&other.lo),
            ord => ord,
        }
    }
}

impl Sub<u128> for U256 {
    type Output = U256;

    #[inline]
    fn sub(self, rhs: u128) -> U256 {
        let (res, borrow) = U256::sub(self, U256 { hi: 0, lo: rhs });
        debug_assert!(!borrow);
        res
    }
}

impl SubAssign<u128> for U256 {
    #[inline]
    fn sub_assign(&mut self, rhs: u128) {
        *self = *self - rhs;
    }
}

#[inline]
fn mul_u128_wide(a: u128, b: u128) -> U256 {
    // Multiplies two u128 into one U256
    let a0 = a as u64 as u128;
    let a1 = (a >> 64) as u64 as u128;
    let b0 = b as u64 as u128;
    let b1 = (b >> 64) as u64 as u128;

    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;

    let mask64 = (1u128 << 64) - 1;

    let (mid, mid_carry) = p01.overflowing_add(p10);
    let mid_lo = mid & mask64;
    let mid_hi = mid >> 64;

    let p00_lo = p00 & mask64;
    let p00_hi = p00 >> 64;
    
    let mid_sum = p00_hi + mid_lo;
    let limb1 = mid_sum & mask64;
    let carry1 = mid_sum >> 64;

    let lo = p00_lo | (limb1 << 64);
    let mut hi = p11;
    hi = hi.wrapping_add(mid_hi);
    hi = hi.wrapping_add(carry1);
    hi = hi.wrapping_add((mid_carry as u128) << 64);

    U256 {hi, lo}
}

#[inline]
fn mul_u256_high(a: U256, b: U256) -> U256 {
    // Multiply two U256, taking the high U256 bits as the result
    // Useful for Barrett Reduction
    let p00 = mul_u128_wide(a.lo, b.lo);
    let p01 = mul_u128_wide(a.lo, b.hi);
    let p10 = mul_u128_wide(a.hi, b.lo);
    let p11 = mul_u128_wide(a.hi, b.hi);

    // first limb
    let (limb0_hi, c00) = p00.hi.overflowing_add(p01.lo);
    let (limb0_hi, c01) = limb0_hi.overflowing_add(p10.lo);

    // second limb, lo
    let c0 = c00 as u128 + c01 as u128;
    let (limb1_lo, c10) = p11.lo.overflowing_add(c0 as u128);
    let (limb1_lo, c11) = limb1_lo.overflowing_add(p01.hi);
    let (limb1_lo, c12) = limb1_lo.overflowing_add(p10.hi);

    // second limb, hi
    let c1 = c10 as u128 + c11 as u128 + c12 as u128;
    let limb1_hi = p11.hi.wrapping_add(c1 as u128);

    let _ = limb0_hi;

    U256 {hi: limb1_hi, lo: limb1_lo}
}

#[inline]
fn div_u256_by_u128(n: U256, d: u128) -> (U256, u128) {
    // Used only ONCE for precompute Barrett Reduction context
    assert!(d != 0);
    assert!(d < MAX_MOD); // Prevent overflowing
    let mut q = U256 {hi: 0, lo: 0};
    let mut r: u128 = 0;

    for i in (0..128).rev() {
        let bit = (n.hi >> i) & 1;
        r = (r << 1) | bit; // at most 2d-1
        if r >= d {
            r -= d;
            q.hi |= 1u128 << i;
        }
    }
    for i in (0..128).rev() {
        let bit = (n.lo >> i) & 1;
        r = (r << 1) | bit; // at most 2d-1
        if r >= d {
            r -= d;
            q.lo |= 1u128 << i;
        }
    }

    (q, r)
}

#[inline]
fn add_mod(a: u128, b: u128, m: u128) -> u128 {
    assert!(a < m && b < m && m < MAX_MOD);
    let mut s = a + b;
    if s > m {
        s -= m;
    }
    s
}

#[inline]
fn sub_mod(a: u128, b: u128, m: u128) -> u128 {
    assert!(a < m && b < m && m < MAX_MOD);
    let (d, borrow) = a.overflowing_sub(b);
    if borrow { d.wrapping_add(m) } else { d }
}

// Barrett Context: fixed modulo and precomputed \mu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrettCtx {
    pub m: u128,
    mu: U256, // floor((2^256 - 1) / mu)
}

impl BarrettCtx {
    pub fn new(m: u128) -> Self {
        assert!(m > 1 && m & 1 == 1 && m < MAX_MOD);
        let numer = U256 { hi: u128::MAX, lo: u128::MAX };
        let (mu, _rem) = div_u256_by_u128(numer, m);

        Self {m, mu}
    }

    #[inline]
    fn reduce_u256(&self, x: U256) -> u128 {
        // Reduction of q = a*b to mod m
        // We have q < m^2

        // Compute q \approx floor(x / m)
        // This means q is like <= m
        let q256 = mul_u256_high(x, self.mu);
        assert!(q256.hi == 0);
        let q = q256.lo;

        // r = x - q*m
        let qm = mul_u128_wide(q, self.m);
        let (r256, borrow) = x.sub(qm);

        assert!(!borrow);
        let mut r = r256;
        if r >= self.m { r -= self.m};
        if r >= self.m { r -= self.m}; // should be enough

        r.to_u128_checked()
    }

    #[inline]
    fn mul_mod(&self, a: u128, b: u128) -> u128 {
        let x = mul_u128_wide(a, b);
        self.reduce_u256(x)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Modp<'a> {
    ctx: &'a BarrettCtx,
    v: u128,
}

impl<'a> Modp<'a> {
    pub fn new(ctx: &'a BarrettCtx, x: u128) -> Self {
        let m = ctx.m;
        Self { ctx, v: x % m }
    }
    pub fn zero(ctx: &'a BarrettCtx) -> Self { Self { ctx, v: 0 } }
    pub fn one(ctx: &'a BarrettCtx) -> Self { Self::new(ctx, 1) }

    pub fn value(&self) -> u128 { self.v }
    pub fn modulus(&self) -> u128 { self.ctx.m }

    #[inline]
    pub fn pow(mut self, mut e: u128) -> Self {
        let mut acc = Modp::one(self.ctx);
        while e != 0 {
            if (e & 1) == 1 { acc *= self; }
            e >>= 1;
            if e != 0 { self *= self; }
        }
        acc
    }

    /// Since m is a prime (your case): inv(a) = a^(m-2).
    #[inline]
    pub fn inv(self) -> Option<Self> {
        if self.v == 0 { return None; }
        Some(self.pow(self.ctx.m - 2))
    }
}

impl<'a> Add for Modp<'a> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self { ctx: self.ctx, v: add_mod(self.v, rhs.v, self.ctx.m) }
    }
}
impl<'a> AddAssign for Modp<'a> { #[inline] fn add_assign(&mut self, rhs: Self) { *self = *self + rhs; } }

impl<'a> Sub for Modp<'a> {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self { ctx: self.ctx, v: sub_mod(self.v, rhs.v, self.ctx.m) }
    }
}
impl<'a> SubAssign for Modp<'a> { #[inline] fn sub_assign(&mut self, rhs: Self) { *self = *self - rhs; } }

impl<'a> Neg for Modp<'a> {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        if self.v == 0 { self } else { Self { ctx: self.ctx, v: self.ctx.m - self.v } }
    }
}

impl<'a> Mul for Modp<'a> {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self { ctx: self.ctx, v: self.ctx.mul_mod(self.v, rhs.v) }
    }
}
impl<'a> MulAssign for Modp<'a> { #[inline] fn mul_assign(&mut self, rhs: Self) { *self = *self * rhs; } }
