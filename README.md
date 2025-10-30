# **Inverse slow loris** 
# drip-client  
**Lightweight, single-binary inverse-Slow-Loris traffic generator written in Rust.**

[![license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)  
![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)

---

## What it does

drip-client opens **N** parallel TCP connections to the same host:port and  
**sends a valid HTTP request on every connection as fast as you allow**  
(∞ by default, or exactly **R** requests-per-second per connection).  
The payload changes only 10 bytes each time, so the kernel cost per request is microscopic.  
Because the tool **never reads the response**, the server’s send buffer fills up;  
when the server eventually blocks on `write()` its thread / epoll-slot / worker is tied  
until the socket is closed – the exact opposite of the classic 2009 “Slow Loris” attack  
(where the client *reads* very slowly).  
This makes drip-client an excellent burn-in tool for:

* HTTP reverse-proxies (nginx, HAProxy, Envoy, Traefik …)  
* Application servers (Node, Go, Java, .NET, Rust, Python …)  
* Load-balancers, firewalls, kernel TCP stacks, cloud WAFs, etc.

You get **millions of open connections** and **millions of requests per second**  
from a single 200 kB binary while staying under 1 GB of RAM.

---

## Features

* **Zero-copy request assembly** – only 10 bytes change per loop  
* **Zero syscalls on read path** – we simply never `read()`  
* **No per-task heap allocations** – everything lives on the stack  
* **Statically linked** – deploy by copying the binary  
* **Cross-platform** – Linux, *BSD, macOS, Windows (tokio)  
* **CLI flags only** – no config files  
* **MIT licensed** – use it in CI, chaos tests, labs, red-team exercises

---

## Quick start


If you have Rust ≥ 1.70 installed, build yourself in 15 s:

```
git clone https://github.com/yourname/drip-client.git
cd drip-client
RUSTFLAGS="-C target-cpu=native" cargo build --release
sudo ./target/release/drip-client --port 80 --clients 500_000
```

---

## CLI reference

```
USAGE:
    drip-client [OPTIONS]

OPTIONS:
    -h, --host <HOST>               Target host [default: 127.0.0.1]
    -p, --port <PORT>               Target port [default: 8080]
        --clients <N>               Number of parallel TCP connections [default: 500000]
        --rps <RPS>                 Requests per second **per connection** (0 = unlimited) [default: 0]
    -V, --version                   Print version
    -h, --help                      Print help
```


```
# saturate localhost on port 8080 as fast as possible
drip-client

# 100 k conn, each doing exactly 10 req/s → 1 M req/s aggregate
drip-client --clients 100000 --rps 10 --port 443

# IPv6
drip-client -h ::1 -p 8080
```

---

## Performance cheat-sheet

| metric | observed on i7-11800H (8 cores) |
|--------|----------------------------------|
| binary size (release, stripped) | 210 kB |
| max open connections | 1 000 000 |
| memory per connection | ≈ 760 bytes (kernel TCP buffer) |
| CPU @ 500 k conn, ∞ rps | 220 % (≈ 0.4 µs per request) |
| packets generated | 1.3 M req/s (≈ 11 Gbit/s with default 128-byte request) |

Tuning tips for Linux
```
# allow 1 M open fds
ulimit -n 1048576
echo 1048576 | sudo tee /proc/sys/fs/nr_open
echo 1048576 | sudo tee /proc/sys/fs/file-max

# shorten FIN-WAIT-2 to recycle sockets faster
echo 10 | sudo tee /proc/sys/net/ipv4/tcp_fin_timeout

# enlarge port range if you run client and server on same box
echo 1024 65535 | sudo tee /proc/sys/net/ipv4/ip_local_port_range
```

---

## How it works (mini deep-dive)

1. **Connection phase**  
   All sockets are created with `TCP_NODELAY` (one syscall) so the first
   request line is sent immediately.  
   No TLS – raw HTTP/1.1 – so we avoid the extra handshake latency.

2. **Request phase**  
   The request template is built once in `main` and leaked to get
   `'static` slices.  
   Per loop we overwrite only 10 ASCII digits (`{:010}`) with a cheap
   u32→decimal routine that touches no allocator.

3. **Read phase**  
   We deliberately **never** call `read`.  
   When the server’s send buffer fills, its next `write()` blocks,
   keeping a worker busy.  
   The closed socket is detected on our next `write_all`, which
   returns `BrokenPipe` and the task exits.

4. **Timer phase**  
   If `--rps` is given we `sleep(Duration::from_nanos(1_000_000_000 / rps))`;  
   otherwise the loop is bound only by the CPU and the kernel’s TCP
   stack, giving the highest possible request rate.

---

## Exit codes

| code | meaning |
|------|---------|
| 0 | all tasks finished (normally never happens) |
| 1 | CLI parse error |
| 2 | runtime error (bind, connect, etc.) |

---
