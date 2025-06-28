use clap::Parser;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::Level;

use daringsby::{Mouth, SpeechStream};

/// Simple HTTP server streaming text-to-speech audio.
///
/// Exposes two routes:
/// - `/` HTML page with an audio element.
/// - `/speech.wav` streaming WAV bytes.
///
/// Run with defaults:
/// `cargo run --bin speech_server`
#[derive(Parser)]
struct Args {
    /// Host interface to bind
    #[arg(long, default_value = "0.0.0.0")]
    host: String,
    /// Port to listen on
    #[arg(long, default_value_t = 3000)]
    port: u16,
    /// Base URL of the Coqui TTS service
    #[arg(long, default_value = "http://10.0.0.180:5002")]
    tts_url: String,
    /// Optional language identifier
    #[arg(long)]
    language_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    let args = Args::parse();

    let mouth = Mouth::new(args.tts_url, args.language_id);
    let rx = mouth.subscribe();
    let stream = Arc::new(SpeechStream::new(rx));
    let app = stream.clone().router();

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;
    tracing::info!(%addr, "serving speech stream");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
