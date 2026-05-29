//! Shared imports for the `MySQL` tool modules.

pub(crate) use std::borrow::Cow;
pub(crate) use std::sync::Arc;

pub(crate) use dbmcp_server::{input_schema, output_schema};
pub(crate) use dbmcp_sql::Connection as _;
pub(crate) use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
pub(crate) use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

pub(crate) use crate::MysqlHandler;
