mod cli;

#[cfg(feature = "server")]
use cli::dispatch;

#[cfg(not(feature = "server"))]
use cli::dispatch_sync;

#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    if let Err(err) = dispatch().await {
        eprintln!("\u{001b}[31merror:\u{001b}[0m {err:?}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "server"))]
fn main() {
    if let Err(err) = dispatch_sync() {
        eprintln!("\u{001b}[31merror:\u{001b}[0m {err:?}");
        std::process::exit(1);
    }
}
