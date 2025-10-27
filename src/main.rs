mod cli;

use cli::dispatch;

#[tokio::main]
async fn main() {
    if let Err(err) = dispatch().await {
        eprintln!("\u{001b}[31merror:\u{001b}[0m {err:?}");
        std::process::exit(1);
    }
}
