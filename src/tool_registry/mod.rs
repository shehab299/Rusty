mod read;
mod shell;
mod write;

use read::ReadTool;
use shell::ShellTool;
use tool::DynTool;

pub fn get_tools() -> Vec<Box<dyn DynTool>> {
    vec![Box::new(ReadTool), Box::new(ShellTool)]
}
