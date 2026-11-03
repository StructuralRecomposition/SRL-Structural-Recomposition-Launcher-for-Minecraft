//! Real-network recommendation smoke test (ignored by default):
//!   cargo test -p sr-core --test recommend_network -- --ignored --nocapture

#[tokio::test]
#[ignore = "requires network access to BMCLAPI/Fabric meta"]
async fn recommends_loaders_for_1_21_1() {
    let http = reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();
    let recs = sr_core::recommend::recommend(&http, "1.21.1").await;
    for item in &recs.items {
        println!(
            "{} {:?} {} — {}",
            item.name, item.version, item.status, item.note
        );
    }
    let fabric = recs.items.iter().find(|i| i.id == "fabric").unwrap();
    assert_eq!(fabric.status, "recommended");
    assert!(fabric.version.is_some());
    let forge = recs.items.iter().find(|i| i.id == "forge").unwrap();
    assert_eq!(forge.status, "available");
    let optifine = recs.items.iter().find(|i| i.id == "optifine").unwrap();
    assert_eq!(optifine.status, "available");
    let neo = recs.items.iter().find(|i| i.id == "neoforge").unwrap();
    assert_eq!(neo.status, "available");
}
