//! The MCP server (`docs/app-control.md` 2): one handler for the whole app, whatever
//! transport carries it. Tools come from both catalogs. Terminal tools and resources are
//! carried out through the executor on a daemon connection made when first needed; app tools
//! and resources go to the session's [`Backend`](crate::Backend), and `app_launch` to the
//! launcher. Both speak as the client that named itself in `initialize`, unless `--client`
//! names it.

use std::collections::BTreeSet;
use std::sync::Arc;

use colosso_app_control::{APP_LAUNCH, ClientInfo, info};
use colosso_term_control::exec::{Done, Executor};
use colosso_term_control::{ClientKind, Connection, ErrorCode, Hello, ProtocolError};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, GetPromptRequestParams,
    GetPromptResponse, Implementation, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ReadResourceResult, ResourceContents, ServerCapabilities, ServerConfig,
    SubscribeRequestParams, SubscriptionFilter, Tool, UnsubscribeRequestParams,
};
use rmcp::service::{RequestContext, RoleServer, SubscriptionContext};
use rmcp::transport::IntoTransport;
use rmcp::{ErrorData, ServerHandler, ServiceExt};
use serde_json::Value;

use crate::Config;
use crate::app::App;
use crate::backend::Connect;
use crate::guide::{self, INSTRUCTIONS};
use crate::launch::Launcher;
use crate::resources::{self, JSON, Uri};
use crate::subscriptions::subscribable;
use crate::tools;
use crate::watch::Watch;

/// One MCP session's server; clones share its connections.
#[derive(Clone)]
pub struct Server {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) config: Config,
    tools: Vec<Tool>,
    /// The executor on the daemon connection, made again when the daemon went away.
    executor: Mutex<Option<Arc<Executor>>>,
    pub(crate) app: App,
    pub(crate) watch: Arc<Watch>,
    /// The resources subscribed to with `resources/subscribe`.
    pub(crate) subscribed: Mutex<BTreeSet<Uri>>,
    /// Whether updates are forwarded to those subscriptions yet.
    pub(crate) forwarding: Mutex<bool>,
}

impl Server {
    /// A session's server: the terminals `config` reaches, with its grants, and the app
    /// through the backend `connect` makes for this session. With a `launcher` (the stdio
    /// relay) `app_launch` is listed and starts the app; without one (the app's own HTTP
    /// server) it isn't.
    pub fn new(config: Config, connect: &Connect, launcher: Option<Launcher>) -> Server {
        let app = App::new(connect, launcher);
        Server {
            inner: Arc::new(Inner {
                tools: tools::list(&config.grants, app.launches()),
                config,
                executor: Mutex::new(None),
                app,
                watch: Arc::new(Watch::new()),
                subscribed: Mutex::default(),
                forwarding: Mutex::new(false),
            }),
        }
    }
}

/// Serves `server` on `transport` until the client goes.
pub async fn serve<T, E, A>(server: Server, transport: T) -> Result<(), String>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = (server.serve(transport).await)
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

    /// Carries out tool `name`: an app operation through the app, anything else through the
    /// terminals' executor, which refuses what it doesn't know.
    fn call(&self, client: &ClientInfo, name: &str, args: Value) -> CallToolResult {
        if info(name).is_some() && !tools::is_terminal(name) {
            return self.app.call(client, name, args);
        }
        match self
            .executor(&client.name)
            .and_then(|executor| executor.call(name, args))
        {
            Ok(done) => success(done),
            Err(error) => tools::terminal_failure(&error),
        }
    }
}

impl Server {
    /// The executor, connecting on a blocking thread when there's none yet.
    pub(crate) async fn executor(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Arc<Executor>, ErrorData> {
        let client = self.identity(context).name;
        let inner = Arc::clone(&self.inner);
        tokio::task::spawn_blocking(move || inner.executor(&client))
            .await
            .map_err(joined)?
            .map_err(resources::error)
    }

    /// Who the request comes from: `--client`, else the client's own name, else `agent`.
    pub(crate) fn identity(&self, context: &RequestContext<RoleServer>) -> ClientInfo {
        let peer = context.client_info();
        let name = (self.inner.config.client.clone())
            .or_else(|| peer.as_ref().map(|peer| peer.name.clone()))
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "agent".to_owned());
        let version = peer.map_or_else(|| env!("CARGO_PKG_VERSION").to_owned(), |p| p.version);
        ClientInfo { name, version }
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder()
            .enable_prompts()
            .enable_tools()
            .enable_resources()
            .enable_resources_subscribe()
            .enable_resources_list_changed()
            .build();
        ServerConfig::new(capabilities)
            .with_server_info(
                Implementation::new("colosso", env!("CARGO_PKG_VERSION")).with_title("Colosso"),
            )
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        let tools = self.inner.tools.clone();
        std::future::ready(Ok(ListToolsResult::with_all_items(tools)))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.inner
            .tools
            .iter()
            .find(|tool| tool.name == name)
            .cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let client = self.identity(&context);
        let name = request.name.into_owned();
        if name == APP_LAUNCH
            && let Some(launched) = self.inner.app.launch().await
        {
            return Ok(launched.into());
        }
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
        std::future::ready(Ok(ListPromptsResult::with_all_items(guide::prompts())))
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetPromptResponse, ErrorData>> + Send + '_ {
        std::future::ready(guide::prompt(&request.name).map(Into::into))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let executor = self.executor(&context).await?;
        let terminals = tokio::task::spawn_blocking(move || resources::list(&executor))
            .await
            .map_err(joined)??;
        let mut listed = resources::app_resources();
        listed.extend(terminals);
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
        let contents = if let Some(Uri::App(resource)) = Uri::parse(&request.uri) {
            let client = self.identity(&context);
            let inner = Arc::clone(&self.inner);
            let output = tokio::task::spawn_blocking(move || inner.app.read(&client, resource))
                .await
                .map_err(joined)?
                .map_err(|error| resources::app_error(&error))?;
            ResourceContents::text(output.to_string(), request.uri).with_mime_type(JSON)
        } else {
            let executor = self.executor(&context).await?;
            tokio::task::spawn_blocking(move || resources::read(&executor, &request.uri))
                .await
                .map_err(joined)??
        };
        Ok(ReadResourceResult::new(vec![contents]).into())
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let uri = subscribable(&request.uri)?;
        // Forwarding starts first, so an update that comes as soon as the resource is
        // followed reaches the client.
        self.forward(context.peer.clone());
        if self.inner.subscribed.lock().insert(uri.clone())
            && let Err(error) = self.watch(vec![uri.clone()], false, &context).await
        {
            self.inner.subscribed.lock().remove(&uri);
            return Err(error);
        }
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let uri = subscribable(&request.uri)?;
        let removed = self.inner.subscribed.lock().remove(&uri);
        if removed {
            self.unwatch(&[uri]).await;
        }
        Ok(())
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
        self.listen_until_cancelled(&context).await
    }
}

#[expect(clippy::needless_pass_by_value, reason = "it's `map_err`'s function")]
pub(crate) fn joined(error: tokio::task::JoinError) -> ErrorData {
    ErrorData::internal_error(error.to_string(), None)
}

/// The summary for the reader, then the structured answer, also as text.
fn success(done: Done) -> CallToolResult {
    tools::success(done.summary, done.structured)
}
