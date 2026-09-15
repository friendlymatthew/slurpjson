use anyhow::{Result, bail};
use slurpjson::{Document, Parser};

fn main() -> Result<()> {
    env_logger::init();

    let mut args = std::env::args().skip(1);

    let json = match (args.next(), args.next(), args.next()) {
        (Some(arg), Some(p), None) if arg == "-f" => {
            let j = std::fs::read(p)?;
            todo!()
        }
        (Some(j), None, None) => j,
        _ => bail!("either pass a file -f or inline json"),
    };

    let parser = Parser::try_new()?;
    let tape = parser.parse_str(&json)?;

    let document = Document::new(json.as_bytes(), &tape);

    dbg!(document);

    Ok(())
}
