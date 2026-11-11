//! Typed executable tools, with an optional `macros` feature for `#[tool]`.
//!
//! ```toml
//! tool = { path = "../tool", features = ["macros"] }
//! ```

pub use async_trait::async_trait;
#[cfg(feature = "macros")]
pub use tool_macros::tool;

#[doc(hidden)]
pub mod __private {
    pub use schemars;
    pub use serde;
}

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::sync::mpsc::{Sender, error::SendError};

/// An execution or JSON conversion failure. Errors are not serialized as output.
pub type ToolError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Provider-independent metadata describing a tool's JSON input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// An incremental update associated with one tool invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDelta {
    pub id: String,
    pub content: String,
}

/// Execution-only state, supplied by the caller rather than JSON arguments.
///
/// The receiver must be consumed concurrently with tool execution: emitting
/// waits for space when the bounded channel is full. Cloned contexts retain
/// the same tool-call ID.
#[derive(Debug, Clone)]
pub struct ToolContext {
    call_id: String,
    updates: Sender<ToolDelta>,
}

impl ToolContext {
    pub fn new(call_id: impl Into<String>, updates: Sender<ToolDelta>) -> Self {
        Self {
            call_id: call_id.into(),
            updates,
        }
    }

    /// Emit an update, or return an error if the receiver has been dropped.
    pub async fn emit(&self, content: impl Into<String>) -> Result<(), SendError<ToolDelta>> {
        self.updates
            .send(ToolDelta {
                id: self.call_id.clone(),
                content: content.into(),
            })
            .await
    }
}

/// A typed tool with JSON input/output schemas and asynchronous execution.
///
/// Inputs must be owned, deserializable values. Outputs must implement
/// `Serialize` and `JsonSchema`; custom types can derive these traits.
/// `Output` describes only the successful value, not the `Result` wrapper.
#[crate::async_trait]
pub trait Tool: Send + Sync {
    type Input: DeserializeOwned + JsonSchema + Send;
    type Output: Serialize + JsonSchema + Send;

    fn name(&self) -> &str;
    fn description(&self) -> &str;

    fn parameters(&self) -> Value {
        schemars::schema_for!(Self::Input).to_value()
    }

    fn return_type(&self) -> Value {
        schemars::schema_for!(Self::Output).to_value()
    }

    /// Provider-independent metadata for registering this tool.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: self.description().into(),
            parameters: self.parameters(),
        }
    }

    async fn call(
        &self,
        input: Self::Input,
        context: ToolContext,
    ) -> Result<Self::Output, ToolError>;

    /// Deserialize arguments, execute the function, and serialize its result.
    async fn execute(&self, arguments: Value, context: ToolContext) -> Result<Value, ToolError> {
        let input = serde_json::from_value(arguments)?;
        Ok(serde_json::to_value(self.call(input, context).await?)?)
    }
}

/// JSON-based interface for storing tools with different Rust input/output types.
///
/// Every `Tool` automatically implements this trait; use `Box<dyn DynTool>`
/// when building a heterogeneous collection of executable tools.
#[crate::async_trait]
pub trait DynTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, arguments: Value, context: ToolContext) -> Result<Value, ToolError>;
}

#[crate::async_trait]
impl<T: Tool> DynTool for T {
    fn definition(&self) -> ToolDefinition {
        Tool::definition(self)
    }

    async fn execute(&self, arguments: Value, context: ToolContext) -> Result<Value, ToolError> {
        Tool::execute(self, arguments, context).await
    }
}
