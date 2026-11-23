//! Shared helpers for the e2e_pipe suite: unique pipe names,
//! the transport-specific JSON-RPC client shim, and the in-process
//! pipe server spawner used by the functional tests.

use std::time::Duration;

use pipe_server::{register_all, Dispatcher, Server, ServerConfig, ServerState};

pub(crate) fn free_pipe_name(tag: &str, kind: &str) -> String {
    // Both Windows named pipes and Unix domain sockets get a
    // unique path per test invocation.
    let unique = format!(
        "{}-{}-{}",
        tag,
        kind,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    #[cfg(windows)]
    {
        // Windows: \\.\pipe\aco_test_<unique>
        format!(r"\\.\pipe\aco_test_{unique}")
    }
    #[cfg(not(windows))]
    {
        // Unix: per-test temp dir + .sock
        let dir = std::env::temp_dir().join(format!("aco-pipe-test-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{kind}.sock"));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }
}

// ── Transport abstraction for the test client ────────────────────

#[cfg(not(windows))]
pub(crate) mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    pub async fn connect_and_request(addr: &str, body: serde_json::Value) -> serde_json::Value {
        let mut conn = UnixStream::connect(addr).await.expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();

        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
            .expect("read failed");
        assert!(n > 0, "empty response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
    }
}

#[cfg(windows)]
pub(crate) mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;

    pub async fn connect_and_request(addr: &str, body: serde_json::Value) -> serde_json::Value {
        let mut conn = ClientOptions::new().open(addr).expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();

        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
            .expect("read failed");
        assert!(n > 0, "empty response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
    }
}

pub(crate) async fn spawn_server(
    tag: &str,
) -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let rpc_path = free_pipe_name(tag, "rpc");
    let events_path = free_pipe_name(tag, "events");
    let cfg = ServerConfig {
        rpc_path: rpc_path.clone(),
        events_path,
    };
    let mut d = Dispatcher::new();
    // Each test gets its own storage dir so secrets, models, and
    // providers from one test don't leak into another. The dir is
    // wiped on entry and removed on drop via a scopeguard.
    let unique = format!(
        "{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let data_root = std::env::temp_dir().join(format!("flowntier-e2e-{unique}"));
    let _ = std::fs::remove_dir_all(&data_root);
    let _ = std::fs::create_dir_all(&data_root);
    let state = ServerState::new(data_root.clone(), data_root.clone()).await;
    register_all(&mut d, state.clone());
    let server = Server::new(cfg, d, state.events.clone());
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    (rpc_path, handle)
}
