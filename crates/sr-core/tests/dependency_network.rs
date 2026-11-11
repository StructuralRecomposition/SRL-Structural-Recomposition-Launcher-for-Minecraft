//! Real Modrinth dependency analysis. Ignored by default because it requires
//! network access; run explicitly with `--ignored --nocapture`.

use sr_core::dependency::resolve::AdapterRef;
use sr_core::dependency::{
    analyze, DependencyRequest, DependencySource, InstalledIndex, ResolveLimits,
};
use sr_core::modrinth::ModrinthService;

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(sr_core::USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
}

#[tokio::test]
#[ignore = "fetches real Modrinth metadata"]
async fn analyzes_real_iris_required_dependency() {
    let service = ModrinthService::new(http());
    let request = DependencyRequest {
        source: "modrinth".into(),
        project_id: "YL57xq9U".into(),
        version_id: Some("zsoi0dso".into()),
        file_id: None,
        instance_id: None,
        game_version: "1.21.1".into(),
        loader: "fabric".into(),
    };
    let adapter =
        AdapterRef::Modrinth(sr_core::dependency::resolve::ModrinthAdapter::new(&service));
    let analysis = analyze(
        &request,
        adapter.as_adapter(),
        &InstalledIndex::default(),
        &ResolveLimits::default(),
    )
    .await
    .expect("real Modrinth analysis");
    assert_eq!(analysis.source, DependencySource::Modrinth.as_str());
    assert!(analysis.complete, "warnings: {:?}", analysis.warnings);
    assert!(
        analysis.blockers.is_empty(),
        "blockers: {:?}",
        analysis.blockers
    );
    let sodium = analysis
        .nodes
        .iter()
        .find(|node| node.project_id == "AANobbMI")
        .expect("Iris requires Sodium");
    assert_eq!(
        sodium.relation,
        sr_core::dependency::DependencyRelation::Required
    );
    assert!(sodium.compatible);
    assert!(sodium.installable);
    assert!(analysis.default_plan.len() >= 2);
    assert!(analysis
        .default_plan
        .iter()
        .all(|entry| !entry.file_name.is_empty()));
    println!(
        "nodes={} edges={} plan={} bytes={}",
        analysis.nodes.len(),
        analysis.edges.len(),
        analysis.default_plan.len(),
        analysis.stats.total_bytes
    );
}
