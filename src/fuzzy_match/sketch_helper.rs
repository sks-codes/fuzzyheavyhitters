use crate::{
    data_structures::{
        modp::{BarrettCtx, Modp},
    },
};

// Precompute some transformations for vectors, such as scaling matrices
// Store in u128 first. Will load to the correct modulo later
pub enum SketchHelper<'a> {
    Ldcf {
        barrett_ctx: &'a BarrettCtx,
        inv_value: u128,
    },
    Rdcf {
        barrett_ctx: &'a BarrettCtx,
        inv_value: u128,
    }
}

impl<'a> SketchHelper<'a> {
    pub fn barrett_ctx(&self) -> BarrettCtx {
        self.barrett_ctx
    }

    pub fn get_helper_vector(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        match self {
            SketchHelper::Ldcf { barrett_ctx, inv_value } => self.get_helper_vector_dcf(level),
            SketchHelper::Rdcf { barrett_ctx, inv_value } => self.get_helper_vector_dcf(level),
        }
    }

    fn inv_value(&self) -> u128 {
        match self {
            SketchHelper::Ldcf { .., inv_value } => inv_value,
            SketchHelper::Rdcf { .., inv_value } => inv_value,
        }
    }

    fn get_helper_vector_dcf(&self, level: usize) -> (Vec<Modp>, Vec<Modp>) {
        let inv_value = self.inv_value();
        let domain_size = 1usize << level;
        let barrett_ctx = self.barrett_ctx();
        (
            vec![Modp::zero(&barrett_ctx)],
            vec![Modp::new(&barrett_ctx, inv_value)],
        )
    }
}