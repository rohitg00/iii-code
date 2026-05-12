use std::process::Command;

#[test]
#[ignore = "requires running iii engine, worker stack, and provider credentials"]
fn local_engine_smoke() {
    let exe = env!("CARGO_BIN_EXE_iii-code");

    let setup = Command::new(exe)
        .args(["setup"])
        .status()
        .expect("run iii-code setup");
    assert!(setup.success());

    let doctor = Command::new(exe)
        .args(["doctor"])
        .status()
        .expect("run iii-code doctor");
    assert!(doctor.success());

    let run = Command::new(exe)
        .args([
            "run",
            "reply with hi",
            "--wait",
            "--stream-timeout-ms",
            "30000",
        ])
        .status()
        .expect("run iii-code smoke prompt");
    assert!(run.success());
}
