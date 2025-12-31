use crate::{
    data_structures::ringvec::RingVec,
    fss::{
        ldcf::{LdcfEval, LdcfKey},
        rdcf::{RdcfEval, RdcfKey},
    },
};
use anyhow::Result;

#[derive(Clone, Debug)]
pub struct IntervalFSSEval {
    ldcf_eval: LdcfEval,
    rdcf_eval: RdcfEval,
}

impl IntervalFSSEval {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        bytes.extend(self.ldcf_eval.to_bytes()?);
        bytes.extend(self.rdcf_eval.to_bytes()?);
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        let mut offset = 0;
        let (ldcf_eval, ldcf_size) = LdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += ldcf_size;
        let (rdcf_eval, rdcf_size) = RdcfEval::from_bytes(&bytes[offset..], modulus)?;
        offset += rdcf_size;
        Ok((
            IntervalFSSEval {
                ldcf_eval,
                rdcf_eval,
            },
            offset,
        ))
    }

    pub fn ldcf_eval(&self) -> &LdcfEval {
        &self.ldcf_eval
    }

    pub fn rdcf_eval(&self) -> &RdcfEval {
        &self.rdcf_eval
    }

    pub fn result(&self) -> RingVec {
        // Return by value to avoid borrowing a temporary
        self.ldcf_eval.y() - self.rdcf_eval.y()
    }
}

#[derive(Clone, Debug)]
pub struct IntervalFSSKey {
    ldcf_key: LdcfKey,
    rdcf_key: RdcfKey,
}

impl IntervalFSSKey {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        bytes.extend(self.ldcf_key.to_bytes()?);
        bytes.extend(self.rdcf_key.to_bytes()?);
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8], modulus: u128) -> Result<(Self, usize)> {
        let mut offset = 0;
        let (ldcf_key, ldcf_size) = LdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += ldcf_size;
        let (rdcf_key, rdcf_size) = RdcfKey::from_bytes(&bytes[offset..], modulus)?;
        offset += rdcf_size;
        Ok((IntervalFSSKey { ldcf_key, rdcf_key }, offset))
    }

    pub fn ldcf_key(&self) -> &LdcfKey {
        &self.ldcf_key
    }

    pub fn rdcf_key(&self) -> &RdcfKey {
        &self.rdcf_key
    }

    pub fn gen_interval_fss_key(
        alpha_bits: &[bool],
        beta_bits: &[bool],
        a: &RingVec,
        b: &RingVec,
        c: &RingVec,
        modulus: u128,
    ) -> Result<(IntervalFSSKey, IntervalFSSKey)> {
        let right_left_payload = c.clone() - b.clone();
        let left_left_payload = a.clone() + right_left_payload.clone();
        let (ldcf_key0, ldcf_key1) =
            LdcfKey::gen_ldcf_key(alpha_bits, &left_left_payload, c, modulus)?;
        let (rdcf_key0, rdcf_key1) = RdcfKey::gen_rdcf_key(
            beta_bits,
            &right_left_payload,
            &RingVec::zero_with_len(right_left_payload.len(), modulus)
                .expect("Failed to create zero ringvec"),
            modulus,
        )?;
        Ok((
            IntervalFSSKey {
                ldcf_key: ldcf_key0,
                rdcf_key: rdcf_key0,
            },
            IntervalFSSKey {
                ldcf_key: ldcf_key1,
                rdcf_key: rdcf_key1,
            },
        ))
    }

    pub fn eval_bit(
        &self,
        state: &IntervalFSSEval,
        modulus: u128,
        dir: bool,
    ) -> Result<IntervalFSSEval> {
        let ldcf_eval = self.ldcf_key.eval_bit(&state.ldcf_eval(), modulus, dir)?;
        let rdcf_eval = self.rdcf_key.eval_bit(&state.rdcf_eval(), modulus, dir)?;
        Ok(IntervalFSSEval {
            ldcf_eval,
            rdcf_eval,
        })
    }

    pub fn expand_prefix(
        &self,
        state: &IntervalFSSEval,
        modulus: u128,
    ) -> Result<(IntervalFSSEval, IntervalFSSEval)> {
        let (ldcf_eval0, ldcf_eval1) = self.ldcf_key.expand_prefix(&state.ldcf_eval(), modulus)?;
        let (rdcf_eval0, rdcf_eval1) = self.rdcf_key.expand_prefix(&state.rdcf_eval(), modulus)?;

        Ok((
            IntervalFSSEval {
                ldcf_eval: ldcf_eval0,
                rdcf_eval: rdcf_eval0,
            },
            IntervalFSSEval {
                ldcf_eval: ldcf_eval1,
                rdcf_eval: rdcf_eval1,
            },
        ))
    }

    pub fn init_eval(&self, modulus: u128) -> Result<IntervalFSSEval> {
        let ldcf_eval = self.ldcf_key.init_eval(modulus)?;
        let rdcf_eval = self.rdcf_key.init_eval(modulus)?;
        Ok(IntervalFSSEval {
            ldcf_eval,
            rdcf_eval,
        })
    }

    pub fn eval_interval_fss(&self, prefix: &[bool], modulus: u128) -> Result<RingVec> {
        let ldcf_eval = self.ldcf_key.eval_ldcf(prefix, modulus)?;
        let rdcf_eval = self.rdcf_key.eval_rdcf(prefix, modulus)?;
        let result = ldcf_eval - rdcf_eval;
        Ok(result)
    }

    pub fn full_domain_eval(&self, modulus: u128, domain_size: usize) -> Result<Vec<RingVec>> {
        let ldcf_full_domain_eval = self.ldcf_key.full_domain_eval(modulus, domain_size)?;
        let rdcf_full_domain_eval = self.rdcf_key.full_domain_eval(modulus, domain_size)?;
        Ok(ldcf_full_domain_eval
            .iter()
            .zip(rdcf_full_domain_eval.iter())
            .map(|(ldcf, rdcf)| ldcf - rdcf)
            .collect())
    }

    pub fn full_domain_eval_ldcf(&self, modulus: u128, domain_size: usize) -> Result<Vec<RingVec>> {
        self.ldcf_key.full_domain_eval(modulus, domain_size)
    }

    pub fn full_domain_eval_rdcf(&self, modulus: u128, domain_size: usize) -> Vec<RingVec> {
        self.rdcf_key
            .full_domain_eval(modulus, domain_size)
            .expect("Failed to eval rdcf key")
    }
}
