//! Declarative tool registry for per-backend MCP routers.
//!
//! A backend declares its tools as a `const` slice of [`ToolSpec`] rows, each
//! row carrying the gating flags for that tool. [`ToolRouterExt::from_specs`]
//! folds the slice into a [`ToolRouter`], skipping tools the current mode forbids.
//! This keeps the read-only / pinned gating matrix as data, not control flow.
//!
//! Tools are registered so that an error returned by a tool's `invoke` reaches
//! the client as a tool result with `isError: true`, not a JSON-RPC error. The
//! caller sees the message (e.g. a database error) and can self-correct. The
//! message first passes through [`ToolHandler::redact_error`], so a backend can
//! apply PII redaction like it does to any other tool output. When the request
//! carries an [`ErrorContext`], its text is appended before redaction.

use std::pin::Pin;
use std::sync::Arc;

use rmcp::ErrorData;
use rmcp::handler::server::common::{AsRequestContext, FromContextPart, schema_for_empty_input};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase, ToolRouter};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{CallToolResult, ContentBlock, Tool};

/// Backend handler hooks the tool registry calls at tool-call time.
pub trait ToolHandler: Send + Sync + 'static {
    /// Rewrites a tool error message before it is returned to the client.
    ///
    /// Backends use this to apply PII redaction to error text. The returned
    /// string is sent verbatim, so implementations must fail closed.
    fn redact_error(&self, message: &str) -> String;
}

/// Supplies extra context appended to selected tool error messages.
///
/// [`crate::Server`] places it in each request's extensions. The callback
/// sees each tool error; when it returns `Some(context)`, the message becomes
/// `<message> (<context>)` before [`ToolHandler::redact_error`] runs, so the
/// context is redacted too.
#[derive(Clone)]
pub struct ErrorContext(Arc<ContextFn>);

/// Callback behind [`ErrorContext`]: returns context for a tool error, if any.
type ContextFn = dyn Fn(&ErrorData) -> Option<String> + Send + Sync;

impl ErrorContext {
    /// Creates a context from a callback evaluated on every tool error.
    pub fn new(context: impl Fn(&ErrorData) -> Option<String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(context))
    }

    /// Returns `error`'s message, with ` (<context>)` appended when available.
    fn annotate(&self, error: ErrorData) -> String {
        match (self.0)(&error) {
            Some(context) => format!("{} ({context})", error.message),
            None => error.message.into_owned(),
        }
    }
}

/// Extractor for the request's [`ErrorContext`], if any.
///
/// Unlike rmcp's `Extensions` extractor, it does not clone the whole map.
struct ErrorContextPart(Option<ErrorContext>);

impl<C: AsRequestContext> FromContextPart<C> for ErrorContextPart {
    fn from_context_part(context: &mut C) -> Result<Self, ErrorData> {
        Ok(Self(
            context.as_request_context().extensions.get::<ErrorContext>().cloned(),
        ))
    }
}

impl std::fmt::Debug for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ErrorContext").finish_non_exhaustive()
    }
}

/// Declarative registration entry for one MCP tool.
///
/// Pairs the tool's router-registration function with its mode gates.
pub struct ToolSpec<H: ToolHandler> {
    /// Registers the tool on a router, returning the extended router.
    register: fn(ToolRouter<H>) -> ToolRouter<H>,
    /// Whether the tool registers only when a database name is pinned in config.
    pinned: bool,
    /// Whether the tool is hidden in read-only mode.
    read_only: bool,
}

impl<H: ToolHandler> std::fmt::Debug for ToolSpec<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolSpec")
            .field("pinned", &self.pinned)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl<H: ToolHandler> ToolSpec<H> {
    /// Creates a spec for an async tool `T` with its pinned and read-only gates.
    #[must_use]
    pub const fn async_tool<T: AsyncTool<H> + 'static>(pinned: bool, read_only: bool) -> Self {
        Self {
            register: register::<H, T>,
            pinned,
            read_only,
        }
    }
}

/// Registers tool `T` on `router`, reporting its errors in-band.
///
/// Tools without an input schema take no arguments and are invoked with
/// `T::Parameter::default()`, matching rmcp's own registration.
fn register<H: ToolHandler, T: AsyncTool<H> + 'static>(router: ToolRouter<H>) -> ToolRouter<H> {
    if T::input_schema().is_some() {
        router.with_route((attribute::<T>(), invoke::<H, T>))
    } else {
        router.with_route((attribute::<T>(), invoke_without_params::<H, T>))
    }
}

/// Builds the [`Tool`] metadata advertised for `T` in `tools/list`.
fn attribute<T: ToolBase>() -> Tool {
    let input_schema = T::input_schema().unwrap_or_else(schema_for_empty_input);
    let mut tool = Tool::new_with_raw(T::name(), T::description(), input_schema);
    tool.title = T::title();
    tool.output_schema = T::output_schema();
    tool.annotations = T::annotations();
    tool.icons = T::icons();
    tool.meta = T::meta();
    tool
}

/// Boxed future returned by the tool invoke wrappers.
type ToolFuture<'a, T> = Pin<Box<dyn Future<Output = Result<Json<T>, CallToolResult>> + Send + 'a>>;

/// Runs tool `T` with arguments extracted from the request.
fn invoke<H: ToolHandler, T: AsyncTool<H>>(
    handler: &H,
    Parameters(params): Parameters<T::Parameter>,
    ErrorContextPart(context): ErrorContextPart,
) -> ToolFuture<'_, T::Output> {
    run::<H, T>(handler, params, context)
}

/// Runs argument-less tool `T` with its default parameter value.
fn invoke_without_params<H: ToolHandler, T: AsyncTool<H>>(
    handler: &H,
    ErrorContextPart(context): ErrorContextPart,
) -> ToolFuture<'_, T::Output> {
    run::<H, T>(handler, T::Parameter::default(), context)
}

/// Runs tool `T`, mapping its error to a [`CallToolResult`] with `isError: true`.
fn run<H: ToolHandler, T: AsyncTool<H>>(
    handler: &H,
    params: T::Parameter,
    context: Option<ErrorContext>,
) -> ToolFuture<'_, T::Output> {
    Box::pin(async move {
        T::invoke(handler, params).await.map(Json).map_err(|error| {
            let error = error.into();
            let message = match &context {
                Some(context) => context.annotate(error),
                None => error.message.into_owned(),
            };
            let message = handler.redact_error(&message);
            CallToolResult::error(vec![ContentBlock::text(message)])
        })
    })
}

/// Extends [`ToolRouter`] with declarative construction from a [`ToolSpec`] table.
pub trait ToolRouterExt<H: ToolHandler>: Sized {
    /// Builds a router from `specs`, skipping mode-gated tools.
    ///
    /// A spec is skipped when its `read_only` gate coincides with `read_only`
    /// mode, or its `pinned` gate disagrees with the current `pinned` mode.
    #[must_use]
    fn from_specs(specs: &[ToolSpec<H>], read_only: bool, pinned: bool) -> Self;
}

impl<H: ToolHandler> ToolRouterExt<H> for ToolRouter<H> {
    fn from_specs(specs: &[ToolSpec<H>], read_only: bool, pinned: bool) -> Self {
        specs
            .iter()
            .filter(|spec| (!spec.read_only || !read_only) && spec.pinned == pinned)
            .fold(ToolRouter::new(), |router, spec| (spec.register)(router))
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use rmcp::handler::server::tool::IntoCallToolResult;
    use rmcp::model::{CallToolResponse, JsonObject, ToolAnnotations};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    use super::*;

    const ERROR: &str = "Duplicate entry 'jane@doe.com' for key 'email'";

    const REDACTED: &str = "Duplicate entry '<EMAIL_ADDRESS>' for key 'email'";

    /// Test handler whose hook optionally masks the email in [`ERROR`].
    struct Handler {
        redact: bool,
    }

    impl ToolHandler for Handler {
        fn redact_error(&self, message: &str) -> String {
            if self.redact {
                message.replace("jane@doe.com", "<EMAIL_ADDRESS>")
            } else {
                message.to_owned()
            }
        }
    }

    #[derive(Default, Deserialize, JsonSchema)]
    struct Request {
        marker: Option<String>,
    }

    #[derive(Serialize, JsonSchema)]
    struct Response {
        ok: bool,
    }

    struct FailingTool;

    impl ToolBase for FailingTool {
        type Parameter = Request;
        type Output = Response;
        type Error = ErrorData;

        fn name() -> Cow<'static, str> {
            "failing".into()
        }

        fn title() -> Option<String> {
            Some("Failing".into())
        }

        fn description() -> Option<Cow<'static, str>> {
            Some("Always fails.".into())
        }

        fn annotations() -> Option<ToolAnnotations> {
            Some(ToolAnnotations::new().read_only(true))
        }
    }

    impl AsyncTool<Handler> for FailingTool {
        async fn invoke(_: &Handler, _: Request) -> Result<Response, ErrorData> {
            Err(ErrorData::internal_error(ERROR, None))
        }
    }

    /// Tool without an input schema; fails unless given the default parameter.
    struct NoInputTool;

    impl ToolBase for NoInputTool {
        type Parameter = Request;
        type Output = Response;
        type Error = ErrorData;

        fn name() -> Cow<'static, str> {
            "no_input".into()
        }

        fn input_schema() -> Option<Arc<JsonObject>> {
            None
        }
    }

    impl AsyncTool<Handler> for NoInputTool {
        async fn invoke(_: &Handler, request: Request) -> Result<Response, ErrorData> {
            match request.marker {
                None => Err(ErrorData::internal_error(ERROR, None)),
                Some(_) => Ok(Response { ok: true }),
            }
        }
    }

    /// Returns the text of the `isError` result produced by `output`.
    fn error_text(output: Result<Json<Response>, CallToolResult>) -> String {
        let response = output.into_call_tool_result();
        let Ok(CallToolResponse::Complete(result)) = response else {
            panic!("expected a completed tool result, got {response:?}");
        };
        assert_eq!(result.is_error, Some(true));
        result.content[0].as_text().expect("text content").text.clone()
    }

    #[tokio::test]
    async fn tool_error_becomes_tool_result_with_is_error() {
        let handler = Handler { redact: false };
        let output =
            invoke::<Handler, FailingTool>(&handler, Parameters(Request::default()), ErrorContextPart(None)).await;

        assert_eq!(error_text(output), ERROR);
    }

    #[tokio::test]
    async fn tool_error_passes_through_redact_error_hook() {
        let handler = Handler { redact: true };
        let output =
            invoke::<Handler, FailingTool>(&handler, Parameters(Request::default()), ErrorContextPart(None)).await;

        assert_eq!(error_text(output), REDACTED);
    }

    #[tokio::test]
    async fn error_context_is_appended_before_redaction() {
        let handler = Handler { redact: true };
        let context = ErrorContext::new(|_| Some("SSH tunnel: auth failed for jane@doe.com".into()));
        let output = invoke::<Handler, FailingTool>(
            &handler,
            Parameters(Request::default()),
            ErrorContextPart(Some(context)),
        )
        .await;

        assert_eq!(
            error_text(output),
            format!("{REDACTED} (SSH tunnel: auth failed for <EMAIL_ADDRESS>)")
        );
    }

    #[tokio::test]
    async fn error_context_without_cause_leaves_message_unchanged() {
        let handler = Handler { redact: false };
        let output = invoke::<Handler, FailingTool>(
            &handler,
            Parameters(Request::default()),
            ErrorContextPart(Some(ErrorContext::new(|_| None))),
        )
        .await;

        assert_eq!(error_text(output), ERROR);
    }

    #[tokio::test]
    async fn error_context_callback_sees_the_error() {
        let handler = Handler { redact: false };
        let context = ErrorContext::new(|error| error.message.contains("Duplicate").then(|| "seen".into()));
        let output = invoke::<Handler, FailingTool>(
            &handler,
            Parameters(Request::default()),
            ErrorContextPart(Some(context)),
        )
        .await;

        assert_eq!(error_text(output), format!("{ERROR} (seen)"));
    }

    #[tokio::test]
    async fn error_context_applies_to_tools_without_input() {
        let handler = Handler { redact: false };
        let context = ErrorContext::new(|_| Some("SSH tunnel: down".into()));
        let output = invoke_without_params::<Handler, NoInputTool>(&handler, ErrorContextPart(Some(context))).await;

        assert_eq!(error_text(output), format!("{ERROR} (SSH tunnel: down)"));
    }

    #[tokio::test]
    async fn no_input_tool_runs_with_default_parameter() {
        let handler = Handler { redact: false };
        let output = invoke_without_params::<Handler, NoInputTool>(&handler, ErrorContextPart(None)).await;

        assert_eq!(error_text(output), ERROR);
    }

    #[test]
    fn no_input_attribute_matches_rmcp_registration() {
        let rmcp = ToolRouter::<Handler>::new().with_async_tool::<NoInputTool>();
        let ours = register::<Handler, NoInputTool>(ToolRouter::new());

        assert_eq!(ours.get("no_input"), rmcp.get("no_input"));
    }

    #[test]
    fn attribute_matches_rmcp_registration() {
        let rmcp = ToolRouter::<Handler>::new().with_async_tool::<FailingTool>();
        let ours = register::<Handler, FailingTool>(ToolRouter::new());

        assert_eq!(ours.get("failing"), rmcp.get("failing"));
    }
}
