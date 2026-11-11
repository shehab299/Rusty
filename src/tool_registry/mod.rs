mod edit;
mod read;
mod shell;
mod write;

use edit::EditTool;
use read::ReadTool;
use shell::ShellTool;
use tool::DynTool;
use write::WriteTool;

pub fn get_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        Box::new(ReadTool),
        Box::new(ShellTool),
        Box::new(WriteTool),
        Box::new(EditTool),
    ]
}
