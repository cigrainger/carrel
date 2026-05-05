use std::process::{Command, Output};

use carrel_store::ids::{canonicalize_external_identifier, id_for_external};
use serde_json::Value;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

#[tokio::test]
async fn feed_add_list_fetch_and_remove_work() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"feed-etag\"")
                .set_body_string(format!(
                    r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
  <channel>
    <title>CLI Feed</title>
    <description>From a fixture server</description>
    <item>
      <guid>cli-1</guid>
      <title>CLI Item</title>
      <link>{}/article</link>
      <description>Summary</description>
    </item>
  </channel>
</rss>"#,
                    server.uri()
                )),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/article"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"
<!doctype html>
<html>
  <head>
    <title>CLI Article</title>
    <meta name="author" content="Ada">
  </head>
  <body>
    <article>
      <h1>CLI Article</h1>
      <p>This readable article body is fetched from the item URL and stored as a local content blob.</p>
      <pre><code>let answer = 42;</code></pre>
      <img src="/image.png" alt="fixture">
    </article>
  </body>
</html>
"#))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1, 2, 3]))
        .mount(&server)
        .await;

    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();
    run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));

    let feed_url = format!("{}/feed.xml", server.uri());
    let add = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "add", &feed_url]),
    );
    assert!(add.stdout.contains("Subscribed to"));

    let list = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "list"]),
    );
    assert!(list.stdout.contains(&feed_url));

    let fetch = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "fetch", "--all"]),
    );
    assert!(fetch.stdout.contains("1 new"));

    let info = run_ok(carrel_cli().args(["--data-dir", data_dir]).arg("info"));
    assert!(info.stdout.contains("Items:         1"));
    assert!(info.stdout.contains("Feeds:         1"));

    let item_id = id_for_external(&canonicalize_external_identifier(&format!(
        "{}/article",
        server.uri()
    )));
    let show = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["item", "show", &item_id, "--words", "12"]),
    );
    assert!(show.stdout.contains("CLI Item"));
    assert!(show.stdout.contains("readable article body is fetched"));

    let shape = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["item", "shape", &item_id]),
    );
    assert!(shape.stdout.contains("has_code:           true"));
    assert!(shape.stdout.contains("has_video_embed:    false"));

    let recompute =
        run_ok(
            carrel_cli()
                .args(["--data-dir", data_dir])
                .args(["db", "recompute", "shapes"]),
        );
    assert!(
        recompute
            .stdout
            .contains("Recomputed shapes for 1/1 cached readable items.")
    );

    let remove = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "remove", &feed_url]),
    );
    assert!(remove.stdout.contains("Removed"));
}

#[tokio::test]
async fn feed_fetch_uses_embedded_content_without_fetching_item_url() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"{{
  "version": "https://jsonfeed.org/version/1.1",
  "title": "Embedded Feed",
  "items": [
    {{
      "id": "embedded-1",
      "url": "{}/embedded",
      "title": "Embedded Item",
      "content_html": "<p>Embedded body content should be stored without fetching the item URL.</p>"
    }}
  ]
}}"#,
            server.uri()
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/embedded"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();
    run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));

    let feed_url = format!("{}/feed.json", server.uri());
    run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "add", &feed_url]),
    );
    let fetch = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "fetch", "--all"]),
    );
    assert!(fetch.stdout.contains("1 new"));

    let item_id = id_for_external(&canonicalize_external_identifier(&format!(
        "{}/embedded",
        server.uri()
    )));
    let show = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["item", "show", &item_id, "--words", "10"]),
    );
    assert!(show.stdout.contains("Embedded Item"));
    assert!(show.stdout.contains("Embedded body content"));
}

#[tokio::test]
async fn feed_fetch_skips_network_error_article_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("User-agent: *\nAllow: /\n"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
  <channel>
    <title>Offline Shell Feed</title>
    <item>
      <guid>offline-1</guid>
      <title>Offline Shell Item</title>
      <link>{}/offline</link>
      <description>Summary survives when extracted content is junk.</description>
    </item>
  </channel>
</rss>"#,
            server.uri()
        )))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/offline"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!doctype html>
<html>
  <body>
    <main>
      <h1>We can't find the internet</h1>
      <p>Attempting to reconnect</p>
    </main>
  </body>
</html>"#,
        ))
        .mount(&server)
        .await;

    let tempdir = tempfile::tempdir().unwrap();
    let data_dir = tempdir.path().to_str().unwrap();
    run_ok(carrel_cli().args(["--data-dir", data_dir]).args([
        "init",
        "--passphrase",
        "test-passphrase",
    ]));

    let feed_url = format!("{}/feed.xml", server.uri());
    run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "add", &feed_url]),
    );
    let fetch = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["feed", "fetch", "--all"]),
    );
    assert!(fetch.stdout.contains("1 new"));
    assert!(
        fetch.stdout.contains("1 content errors"),
        "stdout:\n{}",
        fetch.stdout
    );

    let item_id = id_for_external(&canonicalize_external_identifier(&format!(
        "{}/offline",
        server.uri()
    )));
    let show = run_ok(
        carrel_cli()
            .args(["--data-dir", data_dir])
            .args(["--json", "item", "show", &item_id]),
    );
    let item: Value = serde_json::from_str(&show.stdout).unwrap();
    assert!(item["readable"].is_null());
    assert!(item["preview"].is_null());
    assert_eq!(
        item["summary"].as_str(),
        Some("Summary survives when extracted content is junk.")
    );
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
