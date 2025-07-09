use axum_server::tls_rustls::RustlsConfig;
use futures::Future;
use psyche_rs::AbortGuard;
use rcgen::generate_simple_self_signed;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use crate::{SpeechStream, VisionSensor, args::Args};
use axum::Router;

async fn ensure_tls_certs(cert: &str, key: &str, host: &str) -> std::io::Result<()> {
    if tokio::fs::try_exists(cert).await? && tokio::fs::try_exists(key).await? {
        return Ok(());
    }
    tracing::info!(%cert, %key, "generating self-signed TLS certs");
    if let Some(dir) = Path::new(cert).parent() {
        if !dir.exists() {
            tokio::fs::create_dir_all(dir).await?;
        }
    }
    if let Some(dir) = Path::new(key).parent() {
        if !dir.exists() {
            tokio::fs::create_dir_all(dir).await?;
        }
    }
    let certificate = generate_simple_self_signed(vec![host.into()]).unwrap();
    tokio::fs::write(cert, certificate.serialize_pem().unwrap()).await?;
    tokio::fs::write(key, certificate.serialize_private_key_pem()).await?;
    Ok(())
}

/// Run the HTTP server exposing speech, vision, canvas, and memory graph streams.
pub async fn run_server(
    stream: Arc<SpeechStream>,
    vision: Arc<VisionSensor>,
    memory: Router,
    args: &Args,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> AbortGuard {
    let app = stream
        .clone()
        .router()
        .merge(vision.clone().router())
        .merge(memory);

    let addr: SocketAddr = format!("{}:{}", args.host, args.port)
        .parse()
        .expect("invalid addr");

    let cert = args.tls_cert.clone();
    let key = args.tls_key.clone();
    let host = args.host.clone();

    let handle = tokio::spawn(async move {
        let mut shutdown = Box::pin(shutdown);
        if let Err(e) = ensure_tls_certs(&cert, &key, &host).await {
            tracing::error!(error=?e, "failed to ensure TLS certs");
            return;
        }
        tracing::info!(%addr, "serving HTTPS interface");
        let config = match RustlsConfig::from_pem_file(&cert, &key).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error=?e, "failed to load TLS config");
                return;
            }
        };

        tokio::select! {
            res = axum_server::tls_rustls::bind_rustls(addr, config).serve(app.into_make_service()) => {
                if let Err(e) = res {
                    tracing::error!(?e, "axum serve failed");
                }
            }
            _ = &mut shutdown => {
                tracing::info!("Shutting down Axum server");
            }
        }
    });

    AbortGuard::new(handle)
}
