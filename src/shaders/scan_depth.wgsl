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

@group(0)
@binding(3)
var<storage, read> num_structual: array<u32>;

var<workgroup> scratch: array<i32, 256>;

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    let index = local_id.x;

    var delta = 0i;
    if index < num_structual[0] {
        delta = depth_delta(read_byte(compacted[index]));
    }

    scratch[index] = delta;

    workgroupBarrier();

    for (var i = 0u; i < 8u; i++) {
        var stride = 1u << i;
        workgroupBarrier();

        var left = 0i;
        if index >= stride {
            left = scratch[index - stride];
        }

        workgroupBarrier();

        scratch[index] += left;
    }

    output[index] = scratch[index];
}
