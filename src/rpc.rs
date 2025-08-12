use crate::collect;
use crate::FieldElm;
use crate::data_structures::fastfield::FE;

use serde::Deserialize;
use serde::Serialize;
use crate::data_structures::logexperiments::ServerSide;
use crate::fss::ibdcf::IbDCFKey;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResetRequest {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddKeysRequest {
    pub keys: Vec<Vec<(IbDCFKey, IbDCFKey)>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeInitRequest {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeCrawlRequest {
    pub gc_sender: bool,
    pub threshold: FE
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeCrawlLastRequest {
    pub gc_sender: bool,
    pub threshold: FieldElm
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreePruneRequest {
    pub keep: Vec<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreePruneLastRequest {
    pub keep: Vec<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeSketchFrontierRequest {
    pub level: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TreeSketchFrontierLastRequest {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinalSharesRequest {}

#[tarpc::service]
pub trait Collector {
    async fn reset(rst: ResetRequest) -> String;
    async fn add_keys(add: AddKeysRequest) -> String;
    async fn tree_init(req: TreeInitRequest) -> String;
    async fn tree_crawl(req: TreeCrawlRequest) -> (Vec<bool>, ServerSide);
    async fn tree_crawl_last(req: TreeCrawlLastRequest) -> (Vec<bool>, ServerSide);
    async fn tree_prune(req: TreePruneRequest) -> String;
    async fn tree_prune_last(req: TreePruneLastRequest) -> String;
    async fn final_shares(req: FinalSharesRequest) -> Vec<collect::Result<FieldElm>>;
}
