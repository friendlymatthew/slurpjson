# slurpjson

This project parses JSON entirely on the GPU via wgpu compute shaders. The algorithm decomposes JSON parsing into a pipeline of parallel prefix scans, producing a flat tape of structural characters.

There is a blog post in the works that will describe this parser in detail, but for now [here is a good code pointer](https://github.com/friendlymatthew/slurpjson/blob/c95ea7e08c9f5ce58ab1777b5334c65b917878a4/src/parser.rs#L90)

# Status

Currently, `slurpjson` is a research project exploring how JSON parsing can be reduced to what Raph Levien calls invitingly parallel problems. It is not (yet?) intended to outperform highly optimized CPU parsers such as `simdjson`

That being said, there are some interesting ideas I'd like to explore. For example, a less costly prefix scan implementation as the the current implementation involves 2 workgroup barriers per iteration. Another optimization would be to have each GPU invocation process a small consecutive block of bytes locally, then run the scans over the resulting blocks

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
