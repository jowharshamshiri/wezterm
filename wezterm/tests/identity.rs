//! The name the command line program reports itself by, which programs that
//! drive it read from `--version`.
use std::process::Command;

#[test]
fn version_names_the_program_and_its_build() {
    let output = Command::new(env!("CARGO_BIN_EXE_wsb-wezterm"))
        .arg("--version")
        .output()
        .expect("run wsb-wezterm --version");
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).expect("utf-8 version");
    let mut words = stdout.split_whitespace();
    assert_eq!(words.next(), Some("wsb-wezterm"), "{:?}", stdout);
    let build = words.next().expect("a build after the name");
    assert!(
        build.len() >= "YYYYMMDD-HHMMSS-HASH".len() && build.matches('-').count() == 2,
        "{:?}",
        stdout
    );
    assert_eq!(words.next(), None, "{:?}", stdout);
}
