@group(0)
@binding(0)
var<storage, read> global: array<u32>;

fn read_byte(idx: u32) -> u32 {
    return (global[idx / 4u] >> ((idx % 4u) * 8u)) & 0xFFu;
}

@group(0)
@binding(1)
var<storage, read_write> output: array<vec3<u32>>;

var<workgroup> scratch: array<vec3<u32>, 256>;

/*
this pass marks whether each byte is in normal json, inside a string, or after a string escape

instead of walking bytes one by one, each byte records a transition vector
[state if entering normal, state if entering string, state if entering escape]

for example:
    quote byte:   [string, normal, string]
    regular byte: [normal, string, string]

this means a quote starts a string from normal state, closes a string from string state, and becomes
literal text after an escape. a regular byte leaves normal text normal, leaves strng text in string state,
and ends an escape by returning to string state

the prefix scan composes these vectors, letting every lane
recover the fsm state up to its byte in parallel.

for example, in `"hello \" world"`, the escaped quote is entered from escape
state, so it returns to string state instead of closing the string.
*/
fn compose(lhs: vec3<u32>, rhs: vec3<u32>) -> vec3<u32> {
    return vec3(rhs[lhs[0]], rhs[lhs[1]], rhs[lhs[2]]);
}

const QUOTE = 0x22u;
const BACKSLASH = 0x5Cu;

const NORMAL = 0u;
const STRING = 1u;
const ESCAPE = 2u;

// transition vectors indexed by current state: [from_normal, from_string, from_escape]
const TRANSITION_QUOTE = vec3<u32>(STRING, NORMAL, STRING);
const TRANSITION_ESCAPE = vec3<u32>(NORMAL, ESCAPE, STRING);
const TRANSITION_DEFAULT = vec3<u32>(NORMAL, STRING, STRING);
const TRANSITION_IDENTITY = vec3<u32>(NORMAL, STRING, ESCAPE);

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    switch read_byte(local_id.x) {
        case QUOTE {
            scratch[local_id.x] = TRANSITION_QUOTE;
        }
        case BACKSLASH {
            scratch[local_id.x] = TRANSITION_ESCAPE;
        }
        default {
            scratch[local_id.x] = TRANSITION_DEFAULT;
        }
    }

    workgroupBarrier();

    for (var i = 0u; i < 8u; i++) {
        var stride = 1u << i;
        workgroupBarrier();

        var left = TRANSITION_IDENTITY;

        if local_id.x >= stride {
            left = scratch[local_id.x - stride];
        }

        workgroupBarrier();

        scratch[local_id.x] = compose(left, scratch[local_id.x]);
    }

    output[local_id.x] = scratch[local_id.x];
}
