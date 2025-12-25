use crate::{
    configs::cli_config::ProtocolParameters, fuzzy_match::share_types::{DictionaryType, DistanceMetric, ShareMethod}
};

#[derive(Debug, Clone)]
pub struct SketchConfig {
    pub h1: usize, // FSS phase input bit length
    pub h2: usize, // FSS phase output bit length
    pub q: u128, // sketching values will be in Zq
    pub delta: u128, // Distance threshold
    pub d: usize, // Number of dimensions
    pub method: ShareMethod, // The sharing method used. Only can sketch for FSS now
    pub metric: DistanceMetric, // Distance metric. Can support sketching both Linf and Lp
    pub dictionary_type: DictionaryType, // Known or Unknown
}

impl From<ProtocolParameters> for SketchConfig {
    fn from(config: ProtocolParameters) -> Self {
        unimplemented!()
    }
}