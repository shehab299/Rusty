#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    SubmitPrompt(String),
    Cancel,
    Shutdown,
}

pub enum Event {
    Started,
    TextDelta(String),
    Completed,
    Failed(String),
    ToolCall {
        name: String,
        arguments: String,
    },
    ToolResult {
        name: String,
        output: String,
        success: bool,
    },
}
