//! `colosso mcp` end to end (`docs/terminals.md` 8): MCP's JSON-RPC over a pipe, the tools
//! the catalog lists, and a shell driven through them as the client that named itself.

use std::path::Path;

use colosso_term_control::catalog::Scope;
use colosso_term_control::{ClientKind, DaemonCommand};
use colosso_term_mcp::Config;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
};

use super::{DAEMON, StopOnDrop, connect, test_dir};

/// The client's end of the pipe.
struct Client {
    lines: Lines<BufReader<ReadHalf<DuplexStream>>>,
    writer: WriteHalf<DuplexStream>,
    next: u64,
    /// Notifications that came while an answer was awaited.
    heard: Vec<Value>,
}

impl Client {
    async fn notify(&mut self, method: &str) {
        let message = json!({ "jsonrpc": "2.0", "method": method });
        self.write(&message).await;
    }

    async fn write(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
    }

    /// Asks `method` and waits for its answer, past any notification.
    async fn ask(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.write(&message).await;
        loop {
            let line = self.lines.next_line().await.unwrap().unwrap();
            let answer: Value = serde_json::from_str(&line).unwrap();
            if answer["id"] == id {
                assert!(answer.get("error").is_none(), "{answer}");
                return answer["result"].clone();
            }
            if answer.get("id").is_none() {
                self.heard.push(answer);
            }
        }
    }

    /// Waits for `notifications/resources/updated` about `uri`, heard already or to come.
    async fn updated(&mut self, uri: &str) {
        let about = |message: &Value| {
            message["method"] == "notifications/resources/updated"
                && message["params"]["uri"] == uri
        };
        if let Some(at) = self.heard.iter().position(about) {
            self.heard.remove(at);
            return;
        }
        let wait = async {
            loop {
                let line = self.lines.next_line().await.unwrap().unwrap();
                let message: Value = serde_json::from_str(&line).unwrap();
                if about(&message) {
                    return;
                }
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), wait)
            .await
            .unwrap_or_else(|_| panic!("no update about {uri}"));
    }

    async fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let params = json!({ "name": tool, "arguments": arguments });
        self.ask("tools/call", params).await
    }
}

#[test]
fn an_agent_drives_a_shell_over_mcp() {
    let dir = test_dir("mcp");
    let _stop = StopOnDrop(dir.clone());
    let _daemon = connect(&dir, ClientKind::Cli);
    let checkout =
        std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    let config = Config {
        dir: dir.clone(),
        daemon: DaemonCommand::binary(DAEMON),
        grants: Scope::AGENT.to_vec(),
        cwd: Some(checkout),
        session: None,
    };
    let home = dir.root().parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "PS1='%# '\n").unwrap();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let server = tokio::spawn(colosso_term_mcp::serve(config, tokio::io::split(theirs)));
        let (reader, writer) = tokio::io::split(ours);
        let mut client = Client {
            lines: BufReader::new(reader).lines(),
            writer,
            next: 0,
            heard: Vec::new(),
        };
        let init = json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "mcp-test", "version": "0" },
        });
        let welcome = client.ask("initialize", init).await;
        assert_eq!(welcome["serverInfo"]["name"], "colosso-terminals");
        assert!(
            welcome["instructions"]
                .as_str()
                .unwrap()
                .contains("person_has_control")
        );
        client.notify("notifications/initialized").await;
        what_it_offers(&mut client).await;

        let open = json!({
            "name": "mcp-shell",
            "command": ["/bin/zsh", "-i"],
            "cwd": home,
            "env": { "HOME": home, "ZDOTDIR": home },
        });
        let opened = client.call("terminal_open", open).await;
        assert_eq!(opened["isError"], false, "{opened}");
        assert_eq!(
            opened["structuredContent"]["control"]["holder"]["label"],
            "mcp-test"
        );
        assert!(
            opened["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("opened mcp-shell")
        );
        client
            .call("terminal_wait", json!({ "for": "prompt" }))
            .await;

        let echo = json!({
            "text": "echo from-mcp",
            "enter": true,
            "then": { "wait": { "for": "prompt" }, "read": {} },
        });
        let sent = client.call("terminal_send", echo).await;
        let lines = &sent["structuredContent"]["read"]["lines"];
        assert!(
            lines.as_array().unwrap().contains(&json!("from-mcp")),
            "{sent}"
        );

        let handle = opened["structuredContent"]["terminal"]["terminal"]
            .as_str()
            .unwrap();
        resources(&mut client, handle).await;

        let refused = client.call("terminal_read", json!({ "bogus": true })).await;
        assert_eq!(refused["isError"], true);
        assert_eq!(refused["structuredContent"]["code"], "invalid_argument");
        assert_eq!(refused["structuredContent"]["retryable"], false);

        drop(client);
        server.await.unwrap().unwrap();
    });
}

/// The terminal as resources: listed, read, and its activity subscribed to.
async fn resources(client: &mut Client, handle: &str) {
    let listed = client.ask("resources/list", json!({})).await;
    let uris: Vec<&str> = (listed["resources"].as_array().unwrap().iter())
        .filter_map(|resource| resource["uri"].as_str())
        .collect();
    let screen = format!("colosso://terminals/{handle}/screen");
    assert!(
        uris.contains(&"colosso://worktrees") && uris.contains(&screen.as_str()),
        "{uris:?}"
    );
    let templates = client.ask("resources/templates/list", json!({})).await;
    assert_eq!(templates["resourceTemplates"].as_array().unwrap().len(), 4);

    let read = client.ask("resources/read", json!({ "uri": screen })).await;
    let text = read["contents"][0]["text"].as_str().unwrap();
    assert!(text.lines().any(|line| line == "from-mcp"), "{text}");
    let terminal = format!("colosso://terminals/{handle}");
    let read = client
        .ask("resources/read", json!({ "uri": terminal }))
        .await;
    let described: Value =
        serde_json::from_str(read["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(described["control"]["youHold"], true);

    let activity = format!("colosso://terminals/{handle}/activity");
    for uri in [&activity, &screen] {
        client
            .ask("resources/subscribe", json!({ "uri": uri }))
            .await;
    }
    client
        .call(
            "terminal_send",
            json!({ "text": "echo again", "enter": true }),
        )
        .await;
    client.updated(&activity).await;
    client.updated(&screen).await;
}

/// The guide among the prompts, and the catalog's tools with their hints and schemas.
async fn what_it_offers(client: &mut Client) {
    let prompts = client.ask("prompts/list", json!({})).await;
    assert_eq!(prompts["prompts"][0]["name"], "terminal_guide");
    let guide = client
        .ask("prompts/get", json!({ "name": "terminal_guide" }))
        .await;
    let guide = guide["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(guide.contains("`person_has_control` means the person took this terminal"));

    let tools = client.ask("tools/list", json!({})).await;
    let tools = tools["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        names.contains(&"terminal_send") && names.contains(&"terminal_close"),
        "{names:?}"
    );
    let read = tools.iter().find(|t| t["name"] == "terminal_read").unwrap();
    assert_eq!(read["annotations"]["readOnlyHint"], true);
    assert_eq!(read["inputSchema"]["additionalProperties"], false);
}
