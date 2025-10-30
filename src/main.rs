use anyhow::Result;
use clap::Parser;
use rand::{Rng, SeedableRng};
use rand::rngs::SmallRng;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;

#[derive(Parser, Debug)]
#[command(author, version, about = "Inverse-Slow-Loris traffic generator", long_about = None)]
struct Args {
    /// Target host
    #[arg(short, long, default_value = "127.0.0.1")]
    host: String,

    /// Target port
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Number of parallel connections
    #[arg(long, default_value_t = 500_000)]
    clients: usize,

    /// Requests per second per connection (0 = as fast as possible)
    #[arg(long, default_value_t = 0)]
    rps: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Build once, leak to get 'static slices
    let head = format!(
        "GET /?r={:010} HTTP/1.1\r\nHost: {}\r\nUser-Agent: is/0.1\r\n\r\n",
        0, args.host
    );
    let head = Box::leak(head.into_boxed_str());
    let (prefix, suffix) = head.split_at(head.len() - 12);
    let prefix = prefix.as_bytes();
    let suffix = suffix.as_bytes();

    let mut rng = SmallRng::from_entropy();
    let mut set = tokio::task::JoinSet::new();

    for id in 0..args.clients {
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

    while set.join_next().await.is_some() {}
    Ok(())
}

async fn worker(
    _id: usize,
    host: String,
    port: u16,
    rps: u64,
    prefix: &'static [u8],
    suffix: &'static [u8],
    mut rng: SmallRng,
) -> Result<()> {
    let mut stream = TcpStream::connect((host.as_str(), port)).await?;
    stream.set_nodelay(true)?;

    let mut req = [0u8; 128];
    let prefix_len = prefix.len();
    let suffix_len = suffix.len();
    req[..prefix_len].copy_from_slice(prefix);
    req[prefix_len + 10..prefix_len + 10 + suffix_len].copy_from_slice(suffix);

    let interval = if rps == 0 {
        None
    } else {
        Some(Duration::from_nanos(1_000_000_000 / rps))
    };

    loop {
        let r = rng.gen::<u32>();
        write_u32_ascii(&mut req[prefix_len..prefix_len + 10], r);
        stream.write_all(&req[..prefix_len + 10 + suffix_len]).await?;

        if let Some(d) = interval {
            tokio::time::sleep(d).await;
        }
    }
}

#[inline(always)]
fn write_u32_ascii(dst: &mut [u8], mut v: u32) {
    let mut i = 9;
    loop {
        i -= 1;
        dst[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 { break; }
    }
    for b in &mut dst[..i] { *b = b'0'; }
}
