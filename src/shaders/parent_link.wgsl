struct StackSummary {
    pops: u32,
    len: u32,
    pop_kinds: array<u32, 256>,
    push_indices: array<i32, 256>,
    push_kinds: array<u32, 256>,
}

@group(0)
@binding(0)
var<storage, read> global: array<u32>;

@group(0)
@binding(1)
var<storage, read> compacted: array<u32>;

@group(0)
@binding(2)
var<storage, read> num_structual: array<u32>;

@group(0)
@binding(3)
var<storage, read_write> parents: array<i32>;

@group(0)
@binding(4)
var<storage, read_write> summaries_a: array<StackSummary>;

@group(0)
@binding(5)
var<storage, read_write> summaries_b: array<StackSummary>;

@group(0)
@binding(6)
var<storage, read_write> errors: array<atomic<u32>>;

fn read_byte(idx: u32) -> u32 {
    return (global[idx / 4u] >> ((idx % 4u) * 8u)) & 0xFFu;
}

const LBRACE = 0x7Bu;
const RBRACE = 0x7Du;
const LBRACKET = 0x5Bu;
const RBRACKET = 0x5Du;

fn is_open(b: u32) -> bool {
    return b == LBRACE || b == LBRACKET;
}

fn is_close(b: u32) -> bool {
    return b == RBRACE || b == RBRACKET;
}

fn matches_close(open_kind: u32, close_kind: u32) -> bool {
    return (open_kind == LBRACE && close_kind == RBRACE)
        || (open_kind == LBRACKET && close_kind == RBRACKET);
}

fn mark_error() {
    atomicStore(&errors[0], 1u);
}

fn init_summary(index: u32) {
    summaries_a[index].pops = 0u;
    summaries_a[index].len = 0u;

    if index >= num_structual[0] {
        return;
    }

    let b = read_byte(compacted[index]);

    if is_open(b) {
        summaries_a[index].len = 1u;
        summaries_a[index].push_indices[0] = i32(index);
        summaries_a[index].push_kinds[0] = b;
    }

    if is_close(b) {
        summaries_a[index].pops = 1u;
        summaries_a[index].pop_kinds[0] = b;
    }
}

fn copy_a_to_b(index: u32) {
    summaries_b[index].pops = summaries_a[index].pops;
    summaries_b[index].len = summaries_a[index].len;

    for (var i = 0u; i < summaries_a[index].pops; i++) {
        summaries_b[index].pop_kinds[i] = summaries_a[index].pop_kinds[i];
    }

    for (var i = 0u; i < summaries_a[index].len; i++) {
        summaries_b[index].push_indices[i] = summaries_a[index].push_indices[i];
        summaries_b[index].push_kinds[i] = summaries_a[index].push_kinds[i];
    }
}

fn copy_b_to_a(index: u32) {
    summaries_a[index].pops = summaries_b[index].pops;
    summaries_a[index].len = summaries_b[index].len;

    for (var i = 0u; i < summaries_b[index].pops; i++) {
        summaries_a[index].pop_kinds[i] = summaries_b[index].pop_kinds[i];
    }

    for (var i = 0u; i < summaries_b[index].len; i++) {
        summaries_a[index].push_indices[i] = summaries_b[index].push_indices[i];
        summaries_a[index].push_kinds[i] = summaries_b[index].push_kinds[i];
    }
}

fn validate_a_cancel(left: u32, right: u32, cancel_count: u32) {
    let left_len = summaries_a[left].len;

    for (var i = 0u; i < cancel_count; i++) {
        let open_kind = summaries_a[left].push_kinds[left_len - 1u - i];
        let close_kind = summaries_a[right].pop_kinds[i];

        if !matches_close(open_kind, close_kind) {
            mark_error();
        }
    }
}

fn validate_b_cancel(left: u32, right: u32, cancel_count: u32) {
    let left_len = summaries_b[left].len;

    for (var i = 0u; i < cancel_count; i++) {
        let open_kind = summaries_b[left].push_kinds[left_len - 1u - i];
        let close_kind = summaries_b[right].pop_kinds[i];

        if !matches_close(open_kind, close_kind) {
            mark_error();
        }
    }
}

// combine the stack effect of an earlier range with the range ending at this token
//
// for example, a left range ending with `[` and a right range starting with `]`
// cancel each other because the right-side `]` closes the left-side `[`
fn combine_a_to_b(index: u32, stride: u32) {
    if index < stride {
        copy_a_to_b(index);
        return;
    }

    let left = index - stride;
    let right_pops = summaries_a[index].pops;
    let left_len = summaries_a[left].len;

    let cancel_count = min(left_len, right_pops);

    validate_a_cancel(left, index, cancel_count);

    if right_pops >= left_len {
        summaries_b[index].pops = summaries_a[left].pops + right_pops - left_len;
        summaries_b[index].len = summaries_a[index].len;

        for (var i = 0u; i < summaries_a[left].pops; i++) {
            summaries_b[index].pop_kinds[i] = summaries_a[left].pop_kinds[i];
        }

        for (var i = left_len; i < right_pops; i++) {
            let out = summaries_a[left].pops + i - left_len;
            summaries_b[index].pop_kinds[out] = summaries_a[index].pop_kinds[i];
        }

        for (var i = 0u; i < summaries_a[index].len; i++) {
            summaries_b[index].push_indices[i] = summaries_a[index].push_indices[i];
            summaries_b[index].push_kinds[i] = summaries_a[index].push_kinds[i];
        }

        return;
    }

    let keep = left_len - right_pops;

    summaries_b[index].pops = summaries_a[left].pops;
    summaries_b[index].len = keep + summaries_a[index].len;

    for (var i = 0u; i < summaries_a[left].pops; i++) {
        summaries_b[index].pop_kinds[i] = summaries_a[left].pop_kinds[i];
    }

    for (var i = 0u; i < keep; i++) {
        summaries_b[index].push_indices[i] = summaries_a[left].push_indices[i];
        summaries_b[index].push_kinds[i] = summaries_a[left].push_kinds[i];
    }

    for (var i = 0u; i < summaries_a[index].len; i++) {
        summaries_b[index].push_indices[keep + i] = summaries_a[index].push_indices[i];
        summaries_b[index].push_kinds[keep + i] = summaries_a[index].push_kinds[i];
    }
}

fn combine_b_to_a(index: u32, stride: u32) {
    if index < stride {
        copy_b_to_a(index);
        return;
    }

    let left = index - stride;
    let right_pops = summaries_b[index].pops;
    let left_len = summaries_b[left].len;
    let cancel_count = min(left_len, right_pops);

    validate_b_cancel(left, index, cancel_count);

    if right_pops >= left_len {
        summaries_a[index].pops = summaries_b[left].pops + right_pops - left_len;
        summaries_a[index].len = summaries_b[index].len;

        for (var i = 0u; i < summaries_b[left].pops; i++) {
            summaries_a[index].pop_kinds[i] = summaries_b[left].pop_kinds[i];
        }

        for (var i = left_len; i < right_pops; i++) {
            let out = summaries_b[left].pops + i - left_len;
            summaries_a[index].pop_kinds[out] = summaries_b[index].pop_kinds[i];
        }

        for (var i = 0u; i < summaries_b[index].len; i++) {
            summaries_a[index].push_indices[i] = summaries_b[index].push_indices[i];
            summaries_a[index].push_kinds[i] = summaries_b[index].push_kinds[i];
        }

        return;
    }

    let keep = left_len - right_pops;

    summaries_a[index].pops = summaries_b[left].pops;
    summaries_a[index].len = keep + summaries_b[index].len;

    for (var i = 0u; i < summaries_b[left].pops; i++) {
        summaries_a[index].pop_kinds[i] = summaries_b[left].pop_kinds[i];
    }

    for (var i = 0u; i < keep; i++) {
        summaries_a[index].push_indices[i] = summaries_b[left].push_indices[i];
        summaries_a[index].push_kinds[i] = summaries_b[left].push_kinds[i];
    }

    for (var i = 0u; i < summaries_b[index].len; i++) {
        summaries_a[index].push_indices[keep + i] = summaries_b[index].push_indices[i];
        summaries_a[index].push_kinds[keep + i] = summaries_b[index].push_kinds[i];
    }
}

fn parent_before(index: u32) -> i32 {
    if index == 0u || summaries_a[index - 1u].len == 0u {
        return -1;
    }

    let top = summaries_a[index - 1u].len - 1u;
    return summaries_a[index - 1u].push_indices[top];
}

@compute
@workgroup_size(256)
fn main(@builtin(local_invocation_id) local_id: vec3<u32>) {
    let index = local_id.x;

    init_summary(index);

    storageBarrier();

    for (var i = 0u; i < 8u; i++) {
        let stride = 1u << i;

        if (i & 1u) == 0u {
            combine_a_to_b(index, stride);
        } else {
            combine_b_to_a(index, stride);
        }

        storageBarrier();
    }

    if index < num_structual[0] {
        parents[index] = parent_before(index);
    }

    if index == 255u && (summaries_a[index].pops > 0u || summaries_a[index].len > 0u) {
        mark_error();
    }
}
