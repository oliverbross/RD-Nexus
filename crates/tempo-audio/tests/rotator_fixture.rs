#[test]
#[ignore = "requires an explicitly supplied task-owned rotctld fixture"]
fn reads_the_explicit_task_owned_rotctld_fixture() {
    let address = std::env::var("SHACKCQ_TEST_ROTCTLD_ADDRESS")
        .expect("SHACKCQ_TEST_ROTCTLD_ADDRESS must name the task-owned fixture");
    assert!(
        address.starts_with("127.0.0.1:"),
        "fixture test refuses non-loopback rotctld"
    );
    let (azimuth, elevation) = tempo_audio::rotator::read_position(&address)
        .expect("task-owned rotctld must answer the native Nexus parser");
    assert!((0.0..360.0).contains(&azimuth));
    assert!(elevation.is_none_or(|value| value.is_finite()));
}
