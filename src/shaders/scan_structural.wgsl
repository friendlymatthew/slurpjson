@group(0)
@binding(0)
var<storage, read> global: array<u32>;

@group(0)
@binding(1)
var<storage, read> fsm: array<vec3<u32>>;

@group(0)
@binding(2)
var<storage, read_write> output: array<u32>;

@group(0)
@binding(3)
var<storage, read_write> count: array<u32>;

fn read_byte(idx: u32) -> u32 {
    return (global[idx / 4u] >> ((idx % 4u) * 8u)) & 0xFFu;
}

const QUOTE = 0x22u;
const NORMAL = 0u;
const STRING = 1u;

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

fn is_scalar_byte(b: u32) -> bool {
    return b != 0u && !is_whitespace(b) && !is_structural(b) && b != QUOTE;
}

var<workgroup> scratch: array<u32, 256>;

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    let b = read_byte(local_id.x);
    let state = fsm[local_id.x][0];

    var prev_state = 0u;
    if local_id.x > 0u {
        prev_state = fsm[local_id.x - 1u][0];
    }

    let is_normal = state == NORMAL;
    let starts_string = b == QUOTE && prev_state == NORMAL && state == STRING;

    var starts_scalar = false;
    if is_normal && is_scalar_byte(b) {
        var prev_b = 0u;
        if local_id.x > 0u {
            prev_b = read_byte(local_id.x - 1u);
        }

        starts_scalar = local_id.x == 0u || is_whitespace(prev_b) || is_structural(prev_b);
    }

    let mask = select(0u, 1u, (is_normal && is_structural(b)) || starts_string || starts_scalar);

    scratch[local_id.x] = mask;

    workgroupBarrier();

    for (var i = 0u; i < 8u; i++) {
        var stride = 1u << i;
        workgroupBarrier();

        var left = 0u;
        if local_id.x >= stride {
            left = scratch[local_id.x - stride];
        }

        workgroupBarrier();

        scratch[local_id.x] += left;
    }

    if mask == 1u {
        output[scratch[local_id.x] - 1u] = local_id.x;
    }

    if local_id.x == 255u {
        count[0] = scratch[255u];
    }
}
