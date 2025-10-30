use anyhow::{Context, Result};
use clap::Parser;
use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;
use std::io::{self, Write};            // for flush
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

static NEXT_WORKER_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Parser, Debug)]
#[command(author, version, about = "Inverse-Slow-Loris traffic generator (verbose)", long_about = None)]
struct Args {
    /// Target host
    #[arg(short = 't', long, default_value = "127.0.0.1")]
    host: String,

    /// Target port
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Number of parallel connections
    #[arg(long, default_value_t = 10)]
    clients: usize,

    /// Requests per second per connection (0 = as fast as possible)
    #[arg(long, default_value_t = 5)]
    rps: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Build request template once and leak it to get 'static slices
    let head = format!(
        "GET /?r={:010} HTTP/1.1\r\nHost: {}\r\nUser-Agent: is/0.1\r\n\r\n",
        0, args.host
    );
    let head = Box::leak(head.into_boxed_str());
    let (prefix, suffix) = head.split_at(head.len() - 12);
    let prefix = prefix.as_bytes();
    let suffix = suffix.as_bytes();

    eprintln!(
        "[MAIN]  started  (clients={}, rps={})\n[MAIN]  prefix={} B, suffix={} B",
              args.clients,
              args.rps,
              prefix.len(),
              suffix.len()
    );

    let mut rng = SmallRng::from_entropy();
    let mut set = tokio::task::JoinSet::new();

    for _ in 0..args.clients {
        let id = NEXT_WORKER_ID.fetch_add(1, Ordering::Relaxed);
        set.spawn(worker(
            id,
            args.host.clone(),
                         args.port,
                         args.rps,
                         prefix,
                         suffix,
                         SmallRng::from_seed(rng.gen()),
        ));
    }

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok(())) => continue,
            Ok(Err(e)) => eprintln!("[MAIN]  worker failed: {:#}", e),
            Err(join_err) => eprintln!("[MAIN]  task join error: {}", join_err),
        }
    }
    eprintln!("[MAIN]  all workers finished");
    Ok(())
}

//------------------------------------------------------------------------------

async fn worker(
    id: usize,
    host: String,
    port: u16,
    rps: u64,
    prefix: &'static [u8],
    suffix: &'static [u8],
    mut rng: SmallRng,
) -> Result<()> {
    let worker_tag = format!("[W#{}]", id);
    macro_rules! log {
        ($($arg:tt)*) => {{
            eprintln!("{}   {}", worker_tag, format!($($arg)*));
            io::stderr().lock().flush().unwrap();
        }}
    }

    log!("spawned");

    // ---------- connect ----------
    let mut stream = TcpStream::connect((host.as_str(), port))
    .await
    .with_context(|| format!("TCP connect to {}:{}", host, port))?;
    stream.set_nodelay(true)?;
    log!("TCP connected");

    // ---------- build request template ----------
    let mut req = [0u8; 128];
    let prefix_len = prefix.len();
    let suffix_len = suffix.len();
    let total_len = prefix_len + 10 + suffix_len;
    req[..prefix_len].copy_from_slice(prefix);
    req[prefix_len + 10..total_len].copy_from_slice(suffix);
    log!("req buffer filled (prefix+10+suffix={} B)", total_len);

    // ---------- rate-limiting ----------
    let interval = if rps == 0 {
        None
    } else {
        Some(Duration::from_nanos(1_000_000_000 / rps))
    };

    // ---------- main loop ----------
    let mut counter: u64 = 0;
    loop {
        counter += 1;
        let r = rng.gen::<u32>();
        log!("REQ #{}  r={:010}", counter, r);

        // --- critical section: encode r into ASCII ---
        // (this is where the old overflow happened)
        write_u32_ascii_verbose(&mut req[prefix_len..prefix_len + 10], r);

        // --- send ---
        stream
        .write_all(&req[..total_len])
        .await
        .context("TCP write_all")?;
        log!("write_all returned Ok({})", total_len);

        // --- rate limit ---
        if let Some(d) = interval {
            log!("sleeping {:?} (rps={})", d, rps);
            tokio::time::sleep(d).await;
        }
    }
}

//------------------------------------------------------------------------------

/// Verbose version of the encoder: logs the value and every digit position.
#[inline(always)]
fn write_u32_ascii_verbose(dst: &mut [u8], mut v: u32) {
    eprint!("[ENC]   encoding {} -> ", v);
    let mut i = 9;
    loop {
        dst[i] = b'0' + (v % 10) as u8;
        eprint!("{}", dst[i] as char);
        v /= 10;
        if v == 0 {
            break;
        }
        // --- OLD BUG WAS HERE: i -= 1 before the break check ---
        i -= 1;
    }
    // zero-pad leading positions
    for b in &mut dst[..i] {
        *b = b'0';
    }
    eprintln!(" (pad {} zeros)", i);
    io::stderr().lock().flush().unwrap();
}
