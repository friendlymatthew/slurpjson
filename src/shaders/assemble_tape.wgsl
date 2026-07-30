struct TapeEntry {
    byte_pos: u32,
    byte_end: u32,
    depth: i32,
    parent: i32,
    token_kind: u32,
}

@group(0)
@binding(0)
var<storage, read> global: array<u32>;

@group(0)
@binding(1)
var<storage, read> compacted: array<u32>;

@group(0)
@binding(5)
var<storage, read> fsm: array<vec3<u32>>;

@group(0)
@binding(6)
var<storage, read> input_len: array<u32>;

@group(0)
@binding(7)
var<storage, read> num_structual: array<u32>;

fn read_byte(idx: u32) -> u32 {
    return (global[idx / 4u] >> ((idx % 4u) * 8u)) & 0xFFu;
}

const TOKEN_LEFT_BRACE = 1u;
const TOKEN_RIGHT_BRACE = 2u;
const TOKEN_LEFT_BRACKET = 3u;
const TOKEN_RIGHT_BRACKET = 4u;
const TOKEN_COLON = 5u;
const TOKEN_COMMA = 6u;
const TOKEN_STRING = 7u;
const TOKEN_NUMBER = 8u;
const TOKEN_TRUE = 9u;
const TOKEN_FALSE = 10u;
const TOKEN_NULL = 11u;
const TOKEN_INVALID = 12u;

const QUOTE = 0x22u;
const MINUS = 0x2Du;

const LBRACE = 0x7Bu;
const RBRACE = 0x7Du;
const LBRACKET = 0x5Bu;
const RBRACKET = 0x5Du;
const COLON = 0x3Au;
const COMMA = 0x2Cu;

fn is_whitespace(b: u32) -> bool {
    return b == 0x20u || b == 0x0Au || b == 0x0Du || b == 0x09u;
}

fn is_structural(b: u32) -> bool {
    return b == LBRACE || b == RBRACE || b == LBRACKET || b == RBRACKET || b == COLON || b == COMMA;
}

fn is_digit(b: u32) -> bool {
    return b >= 0x30u && b <= 0x39u;
}

fn is_scalar_kind(kind: u32) -> bool {
    return kind >= TOKEN_NUMBER;
}

fn token_kind(b: u32) -> u32 {
    if b == LBRACE { return TOKEN_LEFT_BRACE; }
    if b == RBRACE { return TOKEN_RIGHT_BRACE; }
    if b == LBRACKET { return TOKEN_LEFT_BRACKET; }
    if b == RBRACKET { return TOKEN_RIGHT_BRACKET; }
    if b == COLON { return TOKEN_COLON; }
    if b == COMMA { return TOKEN_COMMA; }
    if b == QUOTE { return TOKEN_STRING; }
    if b == MINUS || is_digit(b) { return TOKEN_NUMBER; }
    if b == 0x74u { return TOKEN_TRUE; }
    if b == 0x66u { return TOKEN_FALSE; }
    if b == 0x6Eu { return TOKEN_NULL; }

    return TOKEN_INVALID;
}

fn string_end(pos: u32) -> u32 {
    for (var i = pos + 1u; i < input_len[0]; i++) {
        let b = read_byte(i);
        if b == QUOTE && fsm[i - 1u][0] == 1u && fsm[i][0] == 0u {
            return i + 1u;
        }
    }

    return input_len[0];
}

fn scalar_end(pos: u32) -> u32 {
    for (var i = pos + 1u; i < input_len[0]; i++) {
        let b = read_byte(i);
        if b == 0u || is_whitespace(b) || is_structural(b) {
            return i;
        }
    }

    return input_len[0];
}

fn token_end(pos: u32, kind: u32) -> u32 {
    if kind == TOKEN_STRING { return string_end(pos); }
    if is_scalar_kind(kind) { return scalar_end(pos); }

    return pos + 1u;
}

@group(0)
@binding(2)
var<storage, read> depths: array<i32>;

@group(0)
@binding(3)
var<storage, read> parents: array<i32>;

@group(0)
@binding(4)
var<storage, read_write> tape: array<TapeEntry>;

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    if local_id.x >= num_structual[0] {
        return;
    }

    let pos = compacted[local_id.x];
    let b = read_byte(pos);
    let kind = token_kind(b);

    tape[local_id.x] = TapeEntry(
        pos,
        token_end(pos, kind),
        depths[local_id.x],
        parents[local_id.x],
        kind,
    );
}
