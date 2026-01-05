use crate::data_structures::u256::{U256, MAX_MOD, div_u256_by_u128, mul_u128_wide, mul_u256_high, add_mod, sub_mod};
use core::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

// Barrett Context: fixed modulo and precomputed \mu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarrettCtx {
    pub m: u128,
    mu: U256, // floor((2^256 - 1) / mu)
}

impl BarrettCtx {
    pub fn new(m: u128) -> Self {
        assert!(m > 1 && m & 1 == 1 && m < MAX_MOD);
        let numer = U256::new(u128::MAX, u128::MAX);
        let (mu, _rem) = div_u256_by_u128(numer, m);

        Self { m, mu }
    }

    #[inline]
    fn reduce_u256(&self, x: U256) -> u128 {
        // Reduction of q = a*b to mod m
        // We have q < m^2

        // Compute q \approx floor(x / m)
        // This means q is like <= m
        let q256 = mul_u256_high(x, self.mu);
        assert!(q256.hi() == 0);
        let q = q256.lo();

        // r = x - q*m
        let qm = mul_u128_wide(q, self.m);
        let (r256, borrow) = x.sub(qm);

        assert!(!borrow);
        let mut r = r256;
        if r >= self.m {
            r -= self.m
        };
        if r >= self.m {
            r -= self.m
        }; // should be enough

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
    pub fn zero(ctx: &'a BarrettCtx) -> Self {
        Self { ctx, v: 0 }
    }
    pub fn one(ctx: &'a BarrettCtx) -> Self {
        Self::new(ctx, 1)
    }

    pub fn value(&self) -> u128 {
        self.v
    }

    pub fn context(&self) -> &'a BarrettCtx {
        self.ctx
    }

    pub fn modulus(&self) -> u128 {
        self.ctx.m
    }

    #[inline]
    pub fn pow(mut self, mut e: u128) -> Self {
        let mut acc = Modp::one(self.ctx);
        while e != 0 {
            if (e & 1) == 1 {
                acc *= self;
            }
            e >>= 1;
            if e != 0 {
                self *= self;
            }
        }
        acc
    }

    /// Since m is a prime (your case): inv(a) = a^(m-2).
    #[inline]
    pub fn inv(self) -> Option<Self> {
        if self.v == 0 {
            return None;
        }
        Some(self.pow(self.ctx.m - 2))
    }
}

impl<'a> Add for Modp<'a> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self {
            ctx: self.ctx,
            v: add_mod(self.v, rhs.v, self.ctx.m),
        }
    }
}
impl<'a> AddAssign for Modp<'a> {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl<'a> Sub for Modp<'a> {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self {
            ctx: self.ctx,
            v: sub_mod(self.v, rhs.v, self.ctx.m),
        }
    }
}
impl<'a> SubAssign for Modp<'a> {
    #[inline]
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

impl<'a> Neg for Modp<'a> {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        if self.v == 0 {
            self
        } else {
            Self {
                ctx: self.ctx,
                v: self.ctx.m - self.v,
            }
        }
    }
}

impl<'a> Mul for Modp<'a> {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        debug_assert!(core::ptr::eq(self.ctx, rhs.ctx));
        Self {
            ctx: self.ctx,
            v: self.ctx.mul_mod(self.v, rhs.v),
        }
    }
}
impl<'a> MulAssign for Modp<'a> {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        *self = *self * rhs;
    }
}
