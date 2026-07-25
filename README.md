# slurpjson 

This project parses JSON entirely on the GPU via wgpu compute shaders. The parser decomposes JSON parsing into a pipeline of parallel prefix scans, producing a flat tape of structural characters

This is purely for research into reducing JSON parsing into what Raph Levien calls invitingly parallel problems

# Usage

```rust
fn main() {
    let json = r#"
    {
        "foo": "bar",
        "baz": {
            "wef": [1, 2, 3],
            "yearn": {
                "1": 2,
                "2": 3.0 
            } 
        } 
    }
    "#;

    let parser = slurpjson::Parser::try_new()?;
    let tape = parser.parse_str(&json)?;

    let document = slurpjson::Document::new(json.as_bytes(), &tape);

    dbg!(document);
}
```


# Reading

https://raphlinus.github.io/gpu/2020/09/05/stack-monoid.html<br>
https://raphlinus.github.io/personal/2018/05/10/toward-gpu-json-parsing.html<br>
