pub mod config;
use ai_sdk::{
    GenerateRequest, GenerateResponse, generate_text,
    providers::{Model, OpenRouter},
};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Ok(())
}
