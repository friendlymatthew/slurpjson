@group(0)
@binding(0)
var<storage, read> global: array<u32>;

@group(0)
@binding(1)
var<storage, read> compacted: array<u32>;

fn read_byte(idx: u32) -> u32 {
    return (global[idx / 4u] >> ((idx % 4u) * 8u)) & 0xFFu;
}

const LBRACE = 0x7Bu;
const RBRACE = 0x7Du;
const LBRACKET = 0x5Bu;
const RBRACKET = 0x5Du;

fn depth_delta(b: u32) -> i32 {
    if b == LBRACE || b == LBRACKET {
        return 1i;
    }

    if b == RBRACE || b == RBRACKET {
        return -1i;
    }

    return 0i;
}

@group(0)
@binding(2)
var<storage, read_write> output: array<i32>;

var<workgroup> scratch: array<i32, 256>;

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    let i = compacted[local_id.x];
    scratch[local_id.x] = depth_delta(read_byte(i));

    workgroupBarrier();

    for (var i = 0u; i < 8u; i++) {
        var stride = 1u << i;
        workgroupBarrier();

        var left = 0i;
        if local_id.x >= stride {
            left = scratch[local_id.x - stride];
        }

        workgroupBarrier();

        scratch[local_id.x] += left;
    }

    output[local_id.x] = scratch[local_id.x];
}
