use tool::{DynTool, tool};

#[tool(description = "Add two integers")]
fn add(a: i32, b: i32) -> i64 {
    i64::from(a) + i64::from(b)
}


pub fn get_tools() -> Vec<Box<dyn DynTool>> {
    vec![
        Box::new(AddTool)
    ]
}