use crate::{data_structures::ringvec::RingVec, fss::{
    ldcf::{LdcfEval, LdcfKey},
    rdcf::{RdcfEval, RdcfKey},
}};

pub struct IntervalFSSEval<const N: usize> {
    ldcf_eval: LdcfEval<N>,
    rdcf_eval: RdcfEval<N>,
}

impl<const N: usize> IntervalFSSEval<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(self.ldcf_eval.to_bytes());
        bytes.extend(self.rdcf_eval.to_bytes());
        bytes.extend(self.result.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let (ldcf_eval, ldcf_size) = LdcfEval::from_bytes(&bytes[offset..], modulus);
        offset += ldcf_size;
        let (rdcf_eval, rdcf_size) = RdcfEval::from_bytes(&bytes[offset..], modulus);
        offset += rdcf_size;
        let (result, result_size) = RingVec::<N>::from_bytes(&bytes[offset..], modulus).expect("Failed to deserialize RingVec");
        offset += result_size;
        (
            IntervalFSSEval {
                ldcf_eval,
                rdcf_eval,
            },
            offset,
        )
    }

    pub fn ldcf_eval(&self) -> &LdcfEval<N> {
        &self.ldcf_eval 
    }

    pub fn rdcf_eval(&self) -> &RdcfEval<N> {
        &self.rdcf_eval 
    }

    pub fn result(&self) -> &RingVec<N> {
        &self.ldcf_eval.y() - &self.rdcf_eval.y()
    }
}


#[derive(Clone, Debug)]
pub struct IntervalFSSKey<const N: usize> {
    ldcf_key: LdcfKey<N>,
    rdcf_key: RdcfKey<N>,
}

impl<const N: usize> IntervalFSSKey<N> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend(self.ldcf_key.to_bytes());
        bytes.extend(self.rdcf_key.to_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> (Self, usize) {
        let mut offset = 0;
        let (ldcf_key, ldcf_size) = LdcfKey::from_bytes(&bytes[offset..], modulus);
        offset += ldcf_size;
        let (rdcf_key, rdcf_size) = RdcfKey::from_bytes(&bytes[offset..], modulus);
        offset += rdcf_size;
        (
            IntervalFSSKey {
                ldcf_key,
                rdcf_key,
            },
            offset,
        )
    }

    pub fn gen_interval_fss_key(
        alpha_bits: &[bool],
        beta_bits: &[bool],
        a: &RingVec<N>,
        b: &RingVec<N>,
        c: &RingVec<N>,
        modulus: u128,
    ) -> (IntervalFSSKey<N>, IntervalFSSKey<N>) {
        let right_left_payload = c.clone() - b.clone();
        let left_left_payload = a.clone() + right_left_payload.clone();
        let (ldcf_key0, ldcf_key1) = LdcfKey::gen_ldcf_key(alpha_bits, &left_left_payload, c, modulus);
        let (rdcf_key0, rdcf_key1) = RdcfKey::gen_rdcf_key(beta_bits, &right_left_payload, &RingVec::zero(modulus), modulus);
        (
            IntervalFSSKey {
                ldcf_key: ldcf_key0,
                rdcf_key: rdcf_key0,
            },
            IntervalFSSKey {
                ldcf_key: ldcf_key1,
                rdcf_key: rdcf_key1,
            },
        )
    }

    pub fn eval_bit(
        &self, 
        state: &IntervalFSSEval<N>,
        modulus: u128,
        dir: bool,
    ) -> IntervalFSSEval<N> {
        let ldcf_eval = self.ldcf_key.eval_bit(&state.ldcf_eval(), modulus, dir);
        let rdcf_eval = self.rdcf_key.eval_bit(&state.rdcf_eval(), modulus, dir);
        IntervalFSSEval {
            ldcf_eval,
            rdcf_eval,
        }
    }

    pub fn expand_prefix(
        &self,
        state: &IntervalFSSEval<N>,
        modulus: u128,
    ) -> (IntervalFSSEval<N>, IntervalFSSEval<N>) {
        let (ldcf_eval0, ldcf_eval1) = self.ldcf_key.expand_prefix(&state.ldcf_eval(), modulus);
        let (rdcf_eval0, rdcf_eval1) = self.rdcf_key.expand_prefix(&state.rdcf_eval(), modulus);

        (
            IntervalFSSEval {
                ldcf_eval: ldcf_eval0,
                rdcf_eval: rdcf_eval0,
            },
            IntervalFSSEval {
                ldcf_eval: ldcf_eval1,
                rdcf_eval: rdcf_eval1,
            },
        )
    }

    pub fn eval_init(
        &self,
        modulus: u128,
    ) -> IntervalFSSEval<N> {
        let ldcf_eval = self.ldcf_key.eval_init(modulus);
        let rdcf_eval = self.rdcf_key.eval_init(modulus);
        IntervalFSSEval {
            ldcf_eval,
            rdcf_eval,
        }
    }

    pub fn eval_interval_fss(
        &self,
        prefix: &[bool],
        modulus: u128,
    ) -> RingVec<N> {
        let ldcf_eval = self.ldcf_key.eval_ldcf(prefix, modulus);
        let rdcf_eval = self.rdcf_key.eval_rdcf(prefix, modulus);
        let result = ldcf_eval - rdcf_eval;
        result
    }

    pub fn full_domain_eval(
        &self,
        modulus: u128,
        domain_size: usize,
    ) -> Vec<RingVec<N>> {
        let ldcf_full_domain_eval = self.ldcf_key.full_domain_eval(modulus, domain_size);
        let rdcf_full_domain_eval = self.rdcf_key.full_domain_eval(modulus, domain_size);
        ldcf_full_domain_eval.iter().zip(rdcf_full_domain_eval.iter()).map(|(ldcf, rdcf)| ldcf - rdcf).collect()
    }

    pub fn full_domain_eval_ldcf(
        &self,
        modulus: u128,
        domain_size: usize,
    ) -> Vec<RingVec<N>> {
        self.ldcf_key.full_domain_eval(modulus, domain_size)
    }

    pub fn full_domain_eval_rdcf(
        &self,
        modulus: u128,
        domain_size: usize,
    ) -> Vec<RingVec<N>> {
        self.rdcf_key.full_domain_eval(modulus, domain_size)
    }
}