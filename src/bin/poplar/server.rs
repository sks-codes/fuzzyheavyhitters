// Starter code from:
//   https://github.com/google/tarpc/blob/master/example-service/src/server.rs

use mosaic::{
    collect,
    configs::poplar_config,
    data_structures::fastfield::FE,
    data_structures::logexperiments::ServerSide,
    data_structures::prg,
    rpc::TreeCrawlLastRequest,
    rpc::{
        AddKeysRequest, Collector, FinalSharesRequest, ResetRequest, TreeCrawlRequest,
        TreeInitRequest, TreePruneLastRequest, TreePruneRequest,
    },
    FieldElm,
};

use futures::{
    future::{self, Ready},
    prelude::*,
};
use std::io::{BufReader, BufWriter};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::available_parallelism;
use std::time::Duration;
use std::{
    io,
    sync::{Arc, Mutex},
};
use tarpc::{
    context,
    serde_transport::tcp,
    server::{self, Channel},
    tokio_serde::formats::Bincode,
};

extern crate num_cpus;
// type MyChannel = scuttlebutt::SyncChannel<BufReader<UnixStream>, BufWriter<UnixStream>>;
type MyChannel = scuttlebutt::SyncChannel<BufReader<TcpStream>, BufWriter<TcpStream>>;

#[derive(Clone)]
struct CollectorServer {
    seed: prg::PrgSeed,
    data_len: usize,
    arc: Arc<Mutex<collect::KeyCollection<FE, FieldElm>>>,
    // gc_channel: Option<Arc<Mutex<MyChannel>>>
    gc_channels: Vec<Arc<Mutex<MyChannel>>>,
}

impl Collector for CollectorServer {
    type AddKeysFut = Ready<String>;
    type TreeInitFut = Ready<String>;
    type TreeCrawlFut = Ready<(Vec<bool>, ServerSide)>;
    type TreeCrawlLastFut = Ready<(Vec<bool>, ServerSide)>;
    type TreePruneFut = Ready<String>;
    type TreePruneLastFut = Ready<String>;
    type FinalSharesFut = Ready<Vec<collect::Result<FieldElm>>>;
    type ResetFut = Ready<String>;

    fn reset(self, _: context::Context, _rst: ResetRequest) -> Self::ResetFut {
        let mut coll = self.arc.lock().unwrap();
        *coll = collect::KeyCollection::new(&self.seed, self.data_len);

        future::ready("Done".to_string())
    }

    fn add_keys(self, _: context::Context, add: AddKeysRequest) -> Self::AddKeysFut {
        let mut coll = self.arc.lock().unwrap();
        for k in add.keys {
            coll.add_key(k);
        }
        future::ready("".to_string())
    }

    fn tree_init(self, _: context::Context, _req: TreeInitRequest) -> Self::TreeInitFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_init();
        future::ready("Done".to_string())
    }

    fn tree_crawl(self, _: context::Context, req: TreeCrawlRequest) -> Self::TreeCrawlFut {
        let mut coll = self.arc.lock().unwrap();

        let mut locked_channels: Vec<_> =
            self.gc_channels.iter().map(|c| c.lock().unwrap()).collect();

        let mut channel_refs: Vec<&mut MyChannel> = locked_channels
            .iter_mut()
            .map(|guard| &mut **guard)
            .collect();

        let results = coll.tree_crawl(req.gc_sender, &mut channel_refs[..], req.threshold);

        future::ready(results)
    }

    fn tree_crawl_last(
        self,
        _: context::Context,
        req: TreeCrawlLastRequest,
    ) -> Self::TreeCrawlLastFut {
        let mut coll = self.arc.lock().unwrap();

        let mut locked_channels: Vec<_> =
            self.gc_channels.iter().map(|c| c.lock().unwrap()).collect();

        let mut channel_refs: Vec<&mut MyChannel> = locked_channels
            .iter_mut()
            .map(|guard| &mut **guard)
            .collect();

        let results = coll.tree_crawl_last(req.gc_sender, &mut channel_refs[..], req.threshold);

        future::ready(results)
    }

    fn tree_prune(self, _: context::Context, req: TreePruneRequest) -> Self::TreePruneFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_prune(&req.keep);
        future::ready("Done".to_string())
    }

    fn tree_prune_last(
        self,
        _: context::Context,
        req: TreePruneLastRequest,
    ) -> Self::TreePruneLastFut {
        let mut coll = self.arc.lock().unwrap();
        coll.tree_prune_last(&req.keep);
        future::ready("Done".to_string())
    }

    fn final_shares(self, _: context::Context, _req: FinalSharesRequest) -> Self::FinalSharesFut {
        let coll = self.arc.lock().unwrap();
        let out = coll.final_shares();
        future::ready(out)
    }
}

fn create_server_tcp_socket(port: u16) -> io::Result<MyChannel> {
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port)));
    let (stream, _) = listener.unwrap().accept()?;
    stream.set_nodelay(true)?;
    stream.set_nonblocking(false)?; // Ensure blocking mode
    stream.set_read_timeout(Some(Duration::from_secs(30)))?; // Add reasonable timeouts
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    // stream.set_keepalive(Some(Duration::from_secs(30)))?;

    Ok(scuttlebutt::SyncChannel::new(
        BufReader::with_capacity(64 * 4096 * 4096, stream.try_clone().unwrap()),
        BufWriter::with_capacity(64 * 4096 * 4096, stream), // BufReader::new(stream.try_clone()?),
                                                            // BufWriter::new(stream),
    ))
}

fn setup_tcp_sockets(
    server_idx: u16,
    num_cpus: usize,
    server1_addr: SocketAddr,
) -> io::Result<Vec<Arc<Mutex<MyChannel>>>> {
    let mut channels = Vec::with_capacity(num_cpus);
    let base_port = server1_addr.port(); // Use the port from the provided address

    for i in 0..num_cpus {
        let port = base_port + i as u16;
        let channel_result = if server_idx == 0 {
            // Garbler (client) side - connect to server1
            let target_addr = SocketAddr::new(server1_addr.ip(), port);
            connect_with_retries_tcp(target_addr)
        } else {
            // Evaluator (server) side - listen for connections from server0
            create_server_tcp_socket(port)
        };

        let channel = channel_result?;
        channels.push(Arc::new(Mutex::new(channel)));
    }

    Ok(channels)
}

fn connect_with_retries_tcp(addr: SocketAddr) -> io::Result<MyChannel> {
    let mut retries = 0;

    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                return Ok(scuttlebutt::SyncChannel::new(
                    BufReader::new(stream.try_clone()?),
                    BufWriter::new(stream),
                ));
            }
            Err(e) => {
                let last_error = Some(e);
                if retries >= 10 {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionRefused,
                        format!(
                            "Failed to connect to {} after {} retries: {:?}",
                            addr, 10, last_error
                        ),
                    ));
                }
                retries += 1;
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    env_logger::init();

    let (cfg, sid, _) = poplar_config::get_args("Server", true, false);
    let server_addr = match sid {
        0 => cfg.server0,
        1 => cfg.server1,
        _ => panic!("Oh no!"),
    };

    let server_idx = match sid {
        0 => 0,
        1 => 1,
        _ => panic!("Oh no!"),
    };

    // XXX This is bogus
    let seed = prg::PrgSeed { key: [1u8; 16] };

    let coll = collect::KeyCollection::new(&seed, cfg.data_len);
    let arc = Arc::new(Mutex::new(coll));

    let num_cpus = available_parallelism().unwrap().get();

    let gc_channels = setup_tcp_sockets(server_idx, num_cpus, cfg.server1).unwrap_or_else(|e| {
        eprintln!("Warning: Failed to setup GC channels: {}", e);
        vec![]
    });

    let mut server_addr = server_addr;
    // Listen on any IP
    server_addr.set_ip("0.0.0.0".parse().expect("Could not parse"));
    tcp::listen(&server_addr, Bincode::default)
        .await?
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        .map(|channel| {
            let coll_server = CollectorServer {
                seed: seed.clone(),
                data_len: cfg.data_len,
                arc: arc.clone(),
                gc_channels: gc_channels.clone(),
            };
            channel.execute(coll_server.serve())
        })
        .buffer_unordered(100)
        .for_each(|_| async {})
        .await;

    Ok(())
}
