//! Real-network loader metadata smoke test. Ignored by default:
//!   cargo test -p sr-core --test loader_network -- --ignored --nocapture
//! Verifies that Rust's HTTP stack can reach the official Fabric/Quilt meta
//! services and resolve a real profile (no game files are downloaded).

use sr_core::fabric::FabricService;

#[tokio::test]
#[ignore = "requires network access to Fabric/Quilt meta"]
async fn fetches_fabric_and_quilt_profiles() {
    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap();

    let fabric = FabricService::new(http.clone()).with_mirror(true);
    let loaders = fabric.list_loaders("1.21.1").await.expect("fabric loaders");
    assert!(!loaders.is_empty());
    let loader = loaders
        .iter()
        .find(|l| l.stable)
        .unwrap_or(&loaders[0])
        .version
        .clone();
    let profile = fabric
        .profile("1.21.1", &loader)
        .await
        .expect("fabric profile");
    assert_eq!(profile.inherits_from.as_deref(), Some("1.21.1"));
    assert!(profile.main_class.is_some());
    println!(
        "fabric {loader}: {} libraries",
        profile.libraries.as_deref().unwrap_or_default().len()
    );

    // Quilt's official per-game meta is not mirrored by BMCLAPI, so it needs
    // direct access to meta.quiltmc.org. Report it but do not fail the Fabric
    // mirror test when that host is blocked by the current network.
    let quilt = FabricService::quilt(http).with_mirror(true);
    match quilt.list_loaders("1.21.1").await {
        Ok(loaders) => {
            assert!(!loaders.is_empty());
            let loader = loaders[0].version.clone();
            let profile = quilt
                .profile("1.21.1", &loader)
                .await
                .expect("quilt profile");
            assert_eq!(profile.inherits_from.as_deref(), Some("1.21.1"));
            println!(
                "quilt {loader}: {} libraries",
                profile.libraries.as_deref().unwrap_or_default().len()
            );
        }
        Err(e) => println!("quilt official meta unavailable in this network: {e}"),
    }
}
