# Mredis

An asynchronous cache server based on Tokio runtime, compatible with RESP3.

Simple cache server to experiment with asynchronous and network programming in Rust.


## How the code is organized

We use the RESP framing protocol. So you can use a regular redis client to interact with the server. We will show an
example soon.

- [Framing](src/frame.rs): RESP implementation. This module encodes and decode RESP frames. I wrote RESP implementation
many times but this time, there is something different. I use iterative approach to decode nested frames.
- [Commands](src/command.rs): Actual user commands. For now, GET, SET, DEL and PING are implemented.
- [Db](src/db.rs): Sharded Hashmap + Binary Heap. The heap is used to sort entries by expiration. We do not rely on 
eviction strategies like LRU or LFU. We instead do lazy eviction on insertion of new keys. 
- [Server](src/server.rs): This is the server logic. It contains the code to listen on a TCP socket, read user command
calls the appropriate function to process and respond to them.
- [Config](src/config.rs): Server configuration options and command line parsing. We use clap to directly parse the
config struct as cli options.
- Main: The entrypoint of the program. It is located on bin/server.rs.

## Build

```commandline
cargo build --release --bin server                 
```

## Run
```commandline
➜  mredis git:(main) ./target/release/server --help                             
Simple distributed cache server

Usage: server [OPTIONS]

Options:
  -h, --hostname <hostname>
          Hostname or IP address to listen on
          
          [default: 127.0.0.1]

  -p, --port <PORT>
          Port to listen on
          
          [default: 6379]

  -c, --capacity <CAPACITY>
          Pre-allocated storage capacity
          
          [default: 1000000]

  -s, --shard <shard>
          Number of storage shards
          
          [default: 8]

  -b, --buffer <buffer>
          Network read and write buffer size
          
          [default: 8192]

  -l, --limit <limit>
          Maximum number of concurrent connections
          
          [default: 250]

  -v, --verbosity <VERBOSITY>
          Max log level
          
          [default: info]

          Possible values:
          - error: Max level Error
          - warn:  Max level Warning
          - info:  Max level Info
          - debug: Max level Debug
          - trace: Higher level Trace

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version               
```

## Benchmark
Let's benchmark our implementation and compare to a real Redis server. Benchmark done on a M2 Macbook Air with 16g
of ram. We use the official redis benchmark tool. We can do that because our server is compatible with redis clients.

Benchmark param: 1_000_000 entries, GET, SET and 120 concurrent clients.

<details>

<summary>Real redis server</summary>

```commandline
➜  mredis git:(main) ✗ redis-benchmark -t set,get -n 1000000 -r 1000000 -c 120    
====== SET ======                                                     
  1000000 requests completed in 5.80 seconds
  120 parallel clients
  3 bytes payload
  keep alive: 1
  host configuration "save": 3600 1 300 100 60 10000
  host configuration "appendonly": no
  multi-thread: no

Latency by percentile distribution:
0.000% <= 0.191 milliseconds (cumulative count 1)
50.000% <= 0.359 milliseconds (cumulative count 673342)
75.000% <= 0.367 milliseconds (cumulative count 809670)
87.500% <= 0.375 milliseconds (cumulative count 890680)
93.750% <= 0.391 milliseconds (cumulative count 952169)
96.875% <= 0.415 milliseconds (cumulative count 972825)
98.438% <= 0.463 milliseconds (cumulative count 984837)
99.219% <= 0.607 milliseconds (cumulative count 992338)
99.609% <= 0.903 milliseconds (cumulative count 996137)
99.805% <= 1.447 milliseconds (cumulative count 998141)
99.902% <= 2.103 milliseconds (cumulative count 999031)
99.951% <= 2.559 milliseconds (cumulative count 999518)
99.976% <= 3.175 milliseconds (cumulative count 999757)
99.988% <= 3.351 milliseconds (cumulative count 999879)
99.994% <= 3.479 milliseconds (cumulative count 999940)
99.997% <= 3.663 milliseconds (cumulative count 999970)
99.998% <= 4.103 milliseconds (cumulative count 999985)
99.999% <= 4.311 milliseconds (cumulative count 999993)
100.000% <= 4.399 milliseconds (cumulative count 999997)
100.000% <= 4.455 milliseconds (cumulative count 999999)
100.000% <= 4.487 milliseconds (cumulative count 1000000)
100.000% <= 4.487 milliseconds (cumulative count 1000000)

Cumulative distribution of latencies:
0.000% <= 0.103 milliseconds (cumulative count 0)
0.001% <= 0.207 milliseconds (cumulative count 6)
0.048% <= 0.303 milliseconds (cumulative count 483)
96.857% <= 0.407 milliseconds (cumulative count 968568)
98.843% <= 0.503 milliseconds (cumulative count 988425)
99.234% <= 0.607 milliseconds (cumulative count 992338)
99.411% <= 0.703 milliseconds (cumulative count 994114)
99.537% <= 0.807 milliseconds (cumulative count 995373)
99.614% <= 0.903 milliseconds (cumulative count 996137)
99.678% <= 1.007 milliseconds (cumulative count 996782)
99.714% <= 1.103 milliseconds (cumulative count 997139)
99.743% <= 1.207 milliseconds (cumulative count 997433)
99.759% <= 1.303 milliseconds (cumulative count 997592)
99.775% <= 1.407 milliseconds (cumulative count 997753)
99.843% <= 1.503 milliseconds (cumulative count 998425)
99.852% <= 1.607 milliseconds (cumulative count 998521)
99.857% <= 1.703 milliseconds (cumulative count 998572)
99.863% <= 1.807 milliseconds (cumulative count 998633)
99.871% <= 1.903 milliseconds (cumulative count 998711)
99.887% <= 2.007 milliseconds (cumulative count 998870)
99.903% <= 2.103 milliseconds (cumulative count 999031)
99.974% <= 3.103 milliseconds (cumulative count 999741)
99.999% <= 4.103 milliseconds (cumulative count 999985)
100.000% <= 5.103 milliseconds (cumulative count 1000000)

Summary:
  throughput summary: 172562.55 requests per second
  latency summary (msec):
          avg       min       p50       p95       p99       max
        0.362     0.184     0.359     0.391     0.535     4.487
====== GET ======                                                     
  1000000 requests completed in 5.76 seconds
  120 parallel clients
  3 bytes payload
  keep alive: 1
  host configuration "save": 3600 1 300 100 60 10000
  host configuration "appendonly": no
  multi-thread: no

Latency by percentile distribution:
0.000% <= 0.175 milliseconds (cumulative count 1)
50.000% <= 0.351 milliseconds (cumulative count 514628)
75.000% <= 0.367 milliseconds (cumulative count 846411)
87.500% <= 0.375 milliseconds (cumulative count 923874)
93.750% <= 0.383 milliseconds (cumulative count 959985)
96.875% <= 0.391 milliseconds (cumulative count 975637)
98.438% <= 0.407 milliseconds (cumulative count 988096)
99.219% <= 0.423 milliseconds (cumulative count 993151)
99.609% <= 0.447 milliseconds (cumulative count 996628)
99.805% <= 0.495 milliseconds (cumulative count 998091)
99.902% <= 0.623 milliseconds (cumulative count 999039)
99.951% <= 1.639 milliseconds (cumulative count 999514)
99.976% <= 3.303 milliseconds (cumulative count 999759)
99.988% <= 3.567 milliseconds (cumulative count 999878)
99.994% <= 5.159 milliseconds (cumulative count 999939)
99.997% <= 5.447 milliseconds (cumulative count 999970)
99.998% <= 5.591 milliseconds (cumulative count 999985)
99.999% <= 5.655 milliseconds (cumulative count 999993)
100.000% <= 5.695 milliseconds (cumulative count 999997)
100.000% <= 5.711 milliseconds (cumulative count 999999)
100.000% <= 5.719 milliseconds (cumulative count 1000000)
100.000% <= 5.719 milliseconds (cumulative count 1000000)

Cumulative distribution of latencies:
0.000% <= 0.103 milliseconds (cumulative count 0)
0.002% <= 0.207 milliseconds (cumulative count 21)
0.179% <= 0.303 milliseconds (cumulative count 1786)
98.810% <= 0.407 milliseconds (cumulative count 988096)
99.821% <= 0.503 milliseconds (cumulative count 998208)
99.894% <= 0.607 milliseconds (cumulative count 998943)
99.930% <= 0.703 milliseconds (cumulative count 999299)
99.940% <= 0.807 milliseconds (cumulative count 999403)
99.946% <= 0.903 milliseconds (cumulative count 999458)
99.948% <= 1.007 milliseconds (cumulative count 999479)
99.948% <= 1.103 milliseconds (cumulative count 999481)
99.948% <= 1.303 milliseconds (cumulative count 999483)
99.950% <= 1.607 milliseconds (cumulative count 999502)
99.952% <= 1.703 milliseconds (cumulative count 999520)
99.956% <= 2.103 milliseconds (cumulative count 999560)
99.968% <= 3.103 milliseconds (cumulative count 999677)
99.988% <= 4.103 milliseconds (cumulative count 999880)
99.993% <= 5.103 milliseconds (cumulative count 999930)
100.000% <= 6.103 milliseconds (cumulative count 1000000)

Summary:
  throughput summary: 173611.11 requests per second
  latency summary (msec):
          avg       min       p50       p95       p99       max
        0.354     0.168     0.351     0.383     0.415     5.719
                
```
</details>

<details>

<summary>Mredis server benchmark</summary>

```commandline
# server options
➜  mredis git:(main) ✗ ./target/release/server -l 2000 -s 16 
2024-04-21T13:27:00.881984Z  INFO mredis::server: Starting mredis server: Config { ip_addr: "127.0.0.1", port: 6379, capacity: 1000000, shard_count: 16, network_buffer_size: 8192, max_conn: 2000, verbosity: Info }

➜  mredis git:(main) ✗ redis-benchmark -t set,get -n 1000000 -r 1000000 -c 120
WARNING: Could not fetch server CONFIG
====== SET ======                                                     
  1000000 requests completed in 6.09 seconds
  120 parallel clients
  3 bytes payload
  keep alive: 1
  multi-thread: no

Latency by percentile distribution:
0.000% <= 0.191 milliseconds (cumulative count 2)
50.000% <= 0.375 milliseconds (cumulative count 597877)
75.000% <= 0.391 milliseconds (cumulative count 802472)
87.500% <= 0.407 milliseconds (cumulative count 911925)
93.750% <= 0.415 milliseconds (cumulative count 940840)
96.875% <= 0.431 milliseconds (cumulative count 970174)
98.438% <= 0.455 milliseconds (cumulative count 985539)
99.219% <= 0.487 milliseconds (cumulative count 992399)
99.609% <= 0.535 milliseconds (cumulative count 996200)
99.805% <= 0.623 milliseconds (cumulative count 998109)
99.902% <= 0.991 milliseconds (cumulative count 999025)
99.951% <= 2.047 milliseconds (cumulative count 999513)
99.976% <= 3.415 milliseconds (cumulative count 999756)
99.988% <= 3.951 milliseconds (cumulative count 999878)
99.994% <= 4.391 milliseconds (cumulative count 999939)
99.997% <= 4.631 milliseconds (cumulative count 999970)
99.998% <= 5.135 milliseconds (cumulative count 999986)
99.999% <= 5.167 milliseconds (cumulative count 999993)
100.000% <= 5.271 milliseconds (cumulative count 999997)
100.000% <= 5.343 milliseconds (cumulative count 999999)
100.000% <= 5.359 milliseconds (cumulative count 1000000)
100.000% <= 5.359 milliseconds (cumulative count 1000000)

Cumulative distribution of latencies:
0.000% <= 0.103 milliseconds (cumulative count 0)
0.001% <= 0.207 milliseconds (cumulative count 5)
0.009% <= 0.303 milliseconds (cumulative count 92)
91.192% <= 0.407 milliseconds (cumulative count 911925)
99.412% <= 0.503 milliseconds (cumulative count 994124)
99.795% <= 0.607 milliseconds (cumulative count 997952)
99.861% <= 0.703 milliseconds (cumulative count 998611)
99.891% <= 0.807 milliseconds (cumulative count 998909)
99.900% <= 0.903 milliseconds (cumulative count 999003)
99.904% <= 1.007 milliseconds (cumulative count 999037)
99.910% <= 1.103 milliseconds (cumulative count 999097)
99.916% <= 1.207 milliseconds (cumulative count 999155)
99.923% <= 1.303 milliseconds (cumulative count 999234)
99.933% <= 1.407 milliseconds (cumulative count 999330)
99.938% <= 1.503 milliseconds (cumulative count 999375)
99.939% <= 1.607 milliseconds (cumulative count 999386)
99.941% <= 1.703 milliseconds (cumulative count 999405)
99.942% <= 1.807 milliseconds (cumulative count 999422)
99.943% <= 1.903 milliseconds (cumulative count 999433)
99.949% <= 2.007 milliseconds (cumulative count 999487)
99.953% <= 2.103 milliseconds (cumulative count 999530)
99.972% <= 3.103 milliseconds (cumulative count 999722)
99.989% <= 4.103 milliseconds (cumulative count 999890)
99.998% <= 5.103 milliseconds (cumulative count 999979)
100.000% <= 6.103 milliseconds (cumulative count 1000000)

Summary:
  throughput summary: 164311.53 requests per second
  latency summary (msec):
          avg       min       p50       p95       p99       max
        0.375     0.184     0.375     0.423     0.479     5.359
====== GET ======                                                     
  1000000 requests completed in 6.07 seconds
  120 parallel clients
  3 bytes payload
  keep alive: 1
  multi-thread: no

Latency by percentile distribution:
0.000% <= 0.199 milliseconds (cumulative count 1)
50.000% <= 0.375 milliseconds (cumulative count 620483)
75.000% <= 0.391 milliseconds (cumulative count 819381)
87.500% <= 0.399 milliseconds (cumulative count 879668)
93.750% <= 0.415 milliseconds (cumulative count 944112)
96.875% <= 0.431 milliseconds (cumulative count 969408)
98.438% <= 0.455 milliseconds (cumulative count 984595)
99.219% <= 0.487 milliseconds (cumulative count 992613)
99.609% <= 0.527 milliseconds (cumulative count 996114)
99.805% <= 0.607 milliseconds (cumulative count 998168)
99.902% <= 0.711 milliseconds (cumulative count 999044)
99.951% <= 1.407 milliseconds (cumulative count 999513)
99.976% <= 1.943 milliseconds (cumulative count 999756)
99.988% <= 3.175 milliseconds (cumulative count 999877)
99.994% <= 3.911 milliseconds (cumulative count 999941)
99.997% <= 4.023 milliseconds (cumulative count 999969)
99.998% <= 4.071 milliseconds (cumulative count 999984)
99.999% <= 4.119 milliseconds (cumulative count 999993)
100.000% <= 4.127 milliseconds (cumulative count 999997)
100.000% <= 4.135 milliseconds (cumulative count 999998)
100.000% <= 4.143 milliseconds (cumulative count 999999)
100.000% <= 4.143 milliseconds (cumulative count 999999)

Cumulative distribution of latencies:
0.000% <= 0.103 milliseconds (cumulative count 0)
0.001% <= 0.207 milliseconds (cumulative count 6)
0.034% <= 0.303 milliseconds (cumulative count 336)
91.903% <= 0.407 milliseconds (cumulative count 919025)
99.444% <= 0.503 milliseconds (cumulative count 994438)
99.817% <= 0.607 milliseconds (cumulative count 998168)
99.901% <= 0.703 milliseconds (cumulative count 999010)
99.923% <= 0.807 milliseconds (cumulative count 999233)
99.933% <= 0.903 milliseconds (cumulative count 999333)
99.936% <= 1.007 milliseconds (cumulative count 999362)
99.938% <= 1.103 milliseconds (cumulative count 999377)
99.941% <= 1.207 milliseconds (cumulative count 999407)
99.946% <= 1.303 milliseconds (cumulative count 999457)
99.951% <= 1.407 milliseconds (cumulative count 999513)
99.958% <= 1.503 milliseconds (cumulative count 999575)
99.960% <= 1.607 milliseconds (cumulative count 999604)
99.973% <= 1.703 milliseconds (cumulative count 999733)
99.974% <= 1.807 milliseconds (cumulative count 999736)
99.975% <= 1.903 milliseconds (cumulative count 999746)
99.976% <= 2.007 milliseconds (cumulative count 999762)
99.977% <= 2.103 milliseconds (cumulative count 999766)
99.986% <= 3.103 milliseconds (cumulative count 999856)
99.999% <= 4.103 milliseconds (cumulative count 999990)
100.000% <= 5.103 milliseconds (cumulative count 999999)

Summary:
  throughput summary: 164826.11 requests per second
  latency summary (msec):
          avg       min       p50       p95       p99       max
        0.374     0.192     0.375     0.423     0.479     4.143               
```
</details>

Mredis' performance is not very far from the one of Redis.

## Similar project
I did another unfinished implementation in go. You can check it [here](https://github.com/ynachi/gcache/tree/main).
