# MOSAIC

This is a Rust implementation of the Mosaic framework in the paper _Mosaic: A Modular Framework for Private Fuzzy Heavy Hitters_.

# How to use 

To compile, set the Rust flag:
```
$ export RUSTFLAGS+="-C target-cpu=native" 
$ cargo build --release
```

You should prepare four terminals and one config file. First, run server0: 
```
$ cargo run --release --bin fhh_cli server0 --config (path_to_config) --threads (num_threads) 
```

Then, run server1:
```
$ cargo run --release --bin fhh_cli server0 --config (path_to_config) --threads (num_threads) 
```

Now, the servers should be ready to process client requests. 

```
$ cargo run --release --bin fhh_cli client --config (path_to_config)
```

Wait until the client sent through everything, run the dealer:
```
$ cargo run --release --bin fhh_cli dealer --config (path_to_config) --threads (num_threads)
```

# How to set up data files:
Simply create a json file, for example, the following json file contains two client points:
```javascript
[
  [
    XXX,
    XXX
  ],
  [
    XXX,
    XXX
  ]
]
```    

# What about the config?
The config format is as follow:
```javascript
{
  "data_file": "path_to_client_points",
  "query_file": "path_to_server_points_if_apply",
  "protocol": {
    "delta": XXX, // distance threshold 
    "threshold": XXX, // count threshold
    "h1": XX,
    "h2": XX,
    "h3": XX,
    "d": X,
    "share_method": "FSS", // "FSS" or "OKVS"
    "dictionary_type": "Unknown", // "Known" or "Unknown"
    "check_method": "FSS", // "FSS" or "GC"
    "check_property": "Equality", // "Equality" or "MuBounded"
    "threshold_method": "FSS", // "FSS" or "GC"
    "distance_metric": "Linf", // "Linf", "L1", "L2", or "L3"
    "num_clients": XXXX
  },
  "network": {
    "server0_addr": "XXX.XX.XX.XX",
    "server1_addr": "XXX.XX.XX.XX",
    "server0_to_server1_port": XXXX,
    "dealer_to_server0_port": XXXX,
    "dealer_to_server1_port": XXXX,
    "client_to_server0_port": XXXX,
    "client_to_server1_port": XXXX
  },
  "output": {
    "verbose": true,
    "show_intermediate": false,
    "output_file": "results.json"
  }
}
```
