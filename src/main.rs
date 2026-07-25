use anyhow::{Result, anyhow};
use slurpjson::{Document, Parser};

fn main() -> Result<()> {
    env_logger::init();

    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow!("pass in json as arg"))?;

    let json = std::fs::read_to_string(path)?;

    let parser = Parser::try_new()?;
    let tape = parser.parse_str(&json)?;

    let document = Document::new(json.as_bytes(), &tape);

    dbg!(document);

    Ok(())
}
