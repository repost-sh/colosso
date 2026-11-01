//! The MCP server: tools listed from the catalog and resources over the terminals, carried
//! out through the executor on a connection made when first needed, as the client that
//! named itself in `initialize`; resource subscriptions both as `resources/subscribe` and
//! as `subscriptions/listen` streams.

use std::collections::BTreeSet;
use std::sync::Arc;

use colosso_term_control::catalog::{CATALOG, Operation};
use colosso_term_control::exec::{Done, Executor};
use colosso_term_control::{ClientKind, Connection, ErrorCode, Hello, ProtocolError};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, Implementation, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    Prompt, PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    ResourceUpdatedNotificationParam, Role, ServerCapabilities, ServerConfig,
    SubscribeRequestParams, SubscriptionFilter, Tool, ToolAnnotations, UnsubscribeRequestParams,
};
use rmcp::service::{Peer, RequestContext, RoleServer, SubscriptionContext};
use rmcp::transport::IntoTransport;
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;

use crate::Config;
use crate::guide::{GUIDE, GUIDE_NAME, INSTRUCTIONS};
use crate::resources::{self, Uri};
use crate::watch::{Update, Watch};

/// The server; clones share one connection to the daemon.
#[derive(Clone)]
pub struct Terminals {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    /// The executor on the daemon connection, made again when the daemon went away.
    executor: Mutex<Option<Arc<Executor>>>,
    watch: Arc<Watch>,
    /// The resources subscribed to with `resources/subscribe`.
    subscribed: Mutex<BTreeSet<Uri>>,
    /// Whether updates are forwarded to those subscriptions yet.
    forwarding: Mutex<bool>,
}

impl Terminals {
    pub fn new(config: Config) -> Terminals {
        Terminals {
            inner: Arc::new(Inner {
                config,
                executor: Mutex::new(None),
                watch: Arc::new(Watch::new()),
                subscribed: Mutex::default(),
                forwarding: Mutex::new(false),
            }),
        }
    }
}

/// Serves `transport` until the client goes.
pub async fn serve<T, E, A>(config: Config, transport: T) -> Result<(), String>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = (Terminals::new(config).serve(transport).await)
        .map_err(|error| format!("the client didn't start: {error}"))?;
    running
        .waiting()
        .await
        .map(drop)
        .map_err(|error| format!("the server stopped: {error}"))
}

impl Inner {
    /// The executor, connecting first when there's none or the daemon went away.
    fn executor(&self, client: &str) -> Result<Arc<Executor>, ProtocolError> {
        let mut slot = self.executor.lock();
        if let Some(executor) = slot.as_ref().filter(|executor| !executor.is_closed()) {
            return Ok(Arc::clone(executor));
        }
        let mut hello = Hello {
            cwd: self.config.cwd.clone(),
            ..Hello::new(ClientKind::Relay, client, env!("CARGO_PKG_VERSION"))
        };
        hello.client.session.clone_from(&self.config.session);
        let (inbox, events) = colosso_term_control::client::inbox();
        let (connection, welcome) =
            Connection::connect_or_start(&self.config.dir, &self.config.daemon, &hello, inbox)
                .map_err(|error| {
                    ProtocolError::new(
                        ErrorCode::Internal,
                        format!("the terminal daemon couldn't be reached: {error}"),
                    )
                    .retryable()
                })?;
        let executor = Arc::new(Executor::new(
            connection,
            self.config.dir.clone(),
            welcome.actor,
            &self.config.grants,
        ));
        // The daemon's events are read as they come, the inbox having no bound, and tell
        // subscribers what changed.
        let watch = Arc::clone(&self.watch);
        let following = Arc::clone(&executor);
        let followed = std::thread::Builder::new()
            .name("mcp-events".into())
            .spawn(move || watch.follow(&events, &following));
        if let Err(error) = followed {
            log::warn!("couldn't start reading the daemon's events: {error}");
        }
        *slot = Some(Arc::clone(&executor));
        Ok(executor)
    }

    fn call(&self, client: &str, name: &str, args: Value) -> CallToolResult {
        match self
            .executor(client)
            .and_then(|executor| executor.call(name, args))
        {
            Ok(done) => success(done),
            Err(error) => failure(&error),
        }
    }
}

impl Terminals {
    /// The executor, connecting on a blocking thread when there's none yet.
    async fn executor(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Arc<Executor>, ErrorData> {
        let client = client_name(context);
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || inner.executor(&client))
            .await
            .map_err(joined)?
            .map_err(resources::error)
    }

    /// Watches `uris`, on the daemon connection whose events say what changed.
    async fn watch(
        &self,
        uris: Vec<Uri>,
        context: &RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let executor = self.executor(context).await?;
        let watch = Arc::clone(&self.inner.watch);
        tokio::task::spawn_blocking(move || {
            for uri in &uris {
                watch.add(uri, &executor);
            }
        })
        .await
        .map_err(joined)
    }

    /// Forwards updates to the `resources/subscribe` subscriptions, from the first one on.
    fn forward(&self, peer: Peer<RoleServer>) {
        let mut forwarding = self.inner.forwarding.lock();
        if *forwarding {
            return;
        }
        *forwarding = true;
        let inner = Arc::clone(&self.inner);
        let mut updates = inner.watch.updates();
        tokio::spawn(async move {
            loop {
                let sent = match updates.recv().await {
                    Ok(Update::List) => peer.notify_resource_list_changed().await,
                    Ok(Update::Resource(uri)) => {
                        let wanted = inner.subscribed.lock().contains(&uri);
                        if !wanted {
                            continue;
                        }
                        let update = ResourceUpdatedNotificationParam::new(uri.text());
                        peer.notify_resource_updated(update).await
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                if sent.is_err() {
                    return;
                }
            }
        });
    }
}

impl ServerHandler for Terminals {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder()
            .enable_prompts()
            .enable_tools()
            .enable_resources()
            .enable_resources_subscribe()
            .enable_resources_list_changed()
            .build();
        ServerConfig::new(capabilities)
            .with_server_info(Implementation::new(
                "colosso-terminals",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let grants = &self.inner.config.grants;
        let tools = (CATALOG.iter())
            .filter(|operation| grants.contains(&operation.scope))
            .map(tool)
            .collect();
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let client = client_name(&context);
        let name = request.name.into_owned();
        let args = request.arguments.map_or(Value::Null, Value::Object);
        let inner = Arc::clone(&self.inner);
        let result = tokio::task::spawn_blocking(move || inner.call(&client, &name, args))
            .await
            .map_err(joined)?;
        Ok(result.into())
    }

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListPromptsResult, ErrorData>> + Send + '_ {
        let guide = Prompt::new(
            GUIDE_NAME,
            Some("How to work in Colosso's terminals: the loop, waits, control, logs"),
            None,
        )
        .with_title("Terminal guide");
        std::future::ready(Ok(ListPromptsResult::with_all_items(vec![guide])))
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResponse, ErrorData>> + Send + '_ {
        let answer = if request.name == GUIDE_NAME {
            let message = PromptMessage::new_text(Role::User, GUIDE);
            let guide = GetPromptResult::new(vec![message])
                .with_description("How to work in Colosso's terminals");
            Ok(guide.into())
        } else {
            let message = format!("there is no prompt called {}", request.name);
            Err(ErrorData::invalid_params(message, None))
        };
        std::future::ready(answer)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let executor = self.executor(&context).await?;
        let listed = tokio::task::spawn_blocking(move || resources::list(&executor))
            .await
            .map_err(joined)??;
        Ok(ListResourcesResult::with_all_items(listed))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        let templates = ListResourceTemplatesResult::with_all_items(resources::templates());
        std::future::ready(Ok(templates))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let executor = self.executor(&context).await?;
        let contents =
            tokio::task::spawn_blocking(move || resources::read(&executor, &request.uri))
                .await
                .map_err(joined)??;
        Ok(ReadResourceResult::new(vec![contents]).into())
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let uri = subscribable(&request.uri)?;
        if self.inner.subscribed.lock().insert(uri.clone()) {
            self.watch(vec![uri], &context).await?;
        }
        self.forward(context.peer.clone());
        Ok(())
    }

    fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), ErrorData>> + Send + '_ {
        let unsubscribed = subscribable(&request.uri).map(|uri| {
            if self.inner.subscribed.lock().remove(&uri) {
                self.inner.watch.remove(&uri);
            }
        });
        std::future::ready(unsubscribed)
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let mut accepted = SubscriptionFilter::new();
        accepted.resources_list_changed = requested.resources_list_changed;
        accepted.resource_subscriptions = (requested.resource_subscriptions.as_ref()).map(|uris| {
            (uris.iter())
                .filter(|uri| subscribable(uri).is_ok())
                .cloned()
                .collect()
        });
        Some(accepted)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let accepted = context.accepted();
        let list = accepted.resources_list_changed == Some(true);
        let uris: Vec<Uri> = (accepted.resource_subscriptions.iter().flatten())
            .filter_map(|uri| Uri::parse(uri))
            .collect();
        let mut updates = self.inner.watch.updates();
        self.watch(uris.clone(), context.request_context()).await?;
        loop {
            let update = tokio::select! {
                () = context.cancelled() => break,
                update = updates.recv() => update,
            };
            let sent = match update {
                Ok(Update::List) if list => context.sink().notify_resource_list_changed().await,
                Ok(Update::Resource(uri)) if uris.contains(&uri) => {
                    context.sink().notify_resource_updated(uri.text()).await
                }
                Ok(_) | Err(RecvError::Lagged(_)) => Ok(()),
                Err(RecvError::Closed) => break,
            };
            if sent.is_err() {
                break;
            }
        }
        for uri in &uris {
            self.inner.watch.remove(uri);
        }
        Ok(())
    }
}

fn client_name(context: &RequestContext<RoleServer>) -> String {
    (context.peer.peer_info())
        .map_or_else(|| "agent".to_owned(), |info| info.client_info.name.clone())
}

fn subscribable(uri: &str) -> Result<Uri, ErrorData> {
    (Uri::parse(uri).filter(Uri::subscribable))
        .ok_or_else(|| ErrorData::invalid_params(format!("{uri} can't be subscribed to"), None))
}

#[expect(clippy::needless_pass_by_value, reason = "it's `map_err`'s function")]
fn joined(error: tokio::task::JoinError) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

fn tool(operation: &Operation) -> Tool {
    let schema = match operation.input_schema() {
        Value::Object(schema) => schema,
        _ => serde_json::Map::new(),
    };
    let hints = operation.hints;
    let annotations = ToolAnnotations::with_title(operation.title)
        .read_only(hints.read_only)
        .destructive(hints.destructive)
        .idempotent(hints.idempotent)
        .open_world(false);
    Tool::new(operation.name, operation.description, schema)
        .with_title(operation.title)
        .with_annotations(annotations)
}

/// The summary for the reader, then the structured answer, also as text for the clients
/// that read only text.
fn success(done: Done) -> CallToolResult {
    let mut result = CallToolResult::structured(done.structured);
    result.content.insert(0, ContentBlock::text(done.summary));
    result
}

fn failure(error: &ProtocolError) -> CallToolResult {
    let structured = serde_json::to_value(error).unwrap_or_default();
    let mut result = CallToolResult::structured_error(structured);
    result
        .content
        .insert(0, ContentBlock::text(error.message.clone()));
    result
}
