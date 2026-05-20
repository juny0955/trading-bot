pub fn setup() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
