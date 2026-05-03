use std::process::{Command, Output};

fn carrel_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_carrel-cli"))
}

#[test]
fn init_info_migrate_and_query_work() {
    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();

    let init = run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));
    assert!(init.stdout.contains("Initialized at"));
    assert!(init.stdout.contains("Master pubkey:"));
    assert!(init.stderr.contains("Generating master keypair"));

    let info = run_ok(carrel_cli().args(["--data-dir", data_dir]).arg("info"));
    assert!(info.stdout.contains("Schema:        v1"));
    assert!(info.stdout.contains("Items:         0"));
    assert!(info.stdout.contains("Feeds:         0"));
    assert!(info.stdout.contains("Highlights:    0"));
    assert!(info.stdout.contains("Peers:         2"));

    let migrate = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["db", "migrate"]),
    );
    assert!(migrate.stdout.contains("Already at v1."));

    let table = run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "db",
        "query",
        "?[version] := *schema_version{version}",
    ]));
    assert!(table.stdout.contains("version"));
    assert!(table.stdout.contains('1'));

    let json = run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "--json",
        "db",
        "query",
        "?[version] := *schema_version{version}",
    ]));
    assert!(json.stdout.contains("\"version\": 1"));
}

#[test]
fn init_refuses_to_clobber_existing_install() {
    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();

    run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));

    let second = run(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));
    assert!(!second.status.success());
    assert!(second.stderr.contains("already initialized"));
}

#[test]
fn info_requires_initialized_data_dir() {
    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();

    let info = run(carrel_cli().args(["--data-dir", data_dir]).arg("info"));
    assert!(!info.status.success());
    assert!(info.stderr.contains("not initialized"));
}

struct CapturedOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

fn run_ok(command: &mut Command) -> CapturedOutput {
    let output = run(command);
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        output.stdout,
        output.stderr
    );
    output
}

fn run(command: &mut Command) -> CapturedOutput {
    let Output {
        status,
        stdout,
        stderr,
    } = command.output().unwrap();

    CapturedOutput {
        status,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}
