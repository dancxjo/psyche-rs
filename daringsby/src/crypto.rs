use ctor::ctor;
use rustls::crypto::CryptoProvider;

/// Installs the rustls CryptoProvider early during program startup.
#[ctor]
fn install_crypto_provider() {
    let provider = CryptoProvider::get_default()
        .expect("No default CryptoProvider found. Did you enable the `ring` feature?");
    CryptoProvider::install_default(provider).expect("Failed to install CryptoProvider");
}
