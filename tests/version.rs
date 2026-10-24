use assert_cmd::Command;

#[test]
fn version_prints_the_cargo_version() {
    Command::cargo_bin("hali")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("hali {}\n", env!("CARGO_PKG_VERSION")));
}
