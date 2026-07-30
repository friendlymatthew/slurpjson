use crate::{
    Tape, TapeEntry,
    gpu::{ComputeProgram, Gpu},
};
use anyhow::{Result, ensure};

pub const MAX_INPUT_BYTES: usize = 256;
const PARENT_SUMMARY_SIZE: usize = (2 * std::mem::size_of::<u32>())
    + (MAX_INPUT_BYTES * ((2 * std::mem::size_of::<u32>()) + std::mem::size_of::<i32>()));

pub struct Parser {
    gpu: Gpu,
    programs: Programs,
}

struct Programs {
    scan_fsm: ComputeProgram,
    scan_structural: ComputeProgram,
    scan_depth: ComputeProgram,
    parent_link: ComputeProgram,
    assemble_tape: ComputeProgram,
}

impl Parser {
    pub fn try_new() -> Result<Self> {
        let gpu = Gpu::try_new()?;
        let programs = Programs::compile(&gpu);

        Ok(Self { gpu, programs })
    }

    pub const fn max_input_bytes(&self) -> usize {
        MAX_INPUT_BYTES
    }

    pub fn parse(&self, json: impl AsRef<[u8]>) -> Result<Tape> {
        self.parse_bytes(json.as_ref())
    }

    pub fn parse_str(&self, json: &str) -> Result<Tape> {
        self.parse_bytes(json.as_bytes())
    }

    pub fn parse_bytes(&self, json: &[u8]) -> Result<Tape> {
        ensure!(
            json.len() <= MAX_INPUT_BYTES,
            "input is too large, max len: {MAX_INPUT_BYTES} (for now)"
        );

        let input_len = u32::try_from(json.len()).expect("input length must fit in u32");

        let mut input = json.to_vec();
        input.resize(json.len().next_multiple_of(4).max(4), 0);

        let input_buf = self.gpu.storage_buffer("bytes", &input);
        let input_len_buf = self
            .gpu
            .storage_buffer("input_len", bytemuck::cast_slice(&[input_len]));

        let fsm_buf = self
            .gpu
            .storage_buffer_empty("fsm", buffer_size(std::mem::size_of::<[u32; 4]>()));

        let compact_buf = self
            .gpu
            .storage_buffer_empty("compact", buffer_size(std::mem::size_of::<u32>()));

        let num_structual_buf = self
            .gpu
            .storage_buffer_empty("num_structual", count_buffer_size());

        let depth_buf = self
            .gpu
            .storage_buffer_empty("depth", buffer_size(std::mem::size_of::<i32>()));

        let parent_buf = self
            .gpu
            .storage_buffer_empty("parents", buffer_size(std::mem::size_of::<i32>()));

        let parent_summary_a_buf = self
            .gpu
            .storage_buffer_empty("parent_summaries_a", parent_summary_buffer_size());

        let parent_summary_b_buf = self
            .gpu
            .storage_buffer_empty("parent_summaries_b", parent_summary_buffer_size());

        let parent_error_buf = self
            .gpu
            .storage_buffer("parent_errors", bytemuck::cast_slice(&[0u32]));

        let tape_buf = self
            .gpu
            .storage_buffer_empty("tape", buffer_size(std::mem::size_of::<TapeEntry>()));

        // example input: {"foo": "bar\"baz\""}
        let mut encoder = self.gpu.create_encoder();

        // input:
        //     json: {"foo": "bar\"baz\""}
        //
        // output:
        //     position: 00 01 02 03 04 05 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20
        //     byte:      {  "  f  o  o  "  : sp  "  b  a  r  \  "  b  a  z  \  "  "  }
        //     state:     n  s  s  s  s  n  n  n  s  s  s  s  e  s  s  s  s  e  s  n  n
        //
        //     n = normal json, s = inside a string, e = immediately after an escape
        self.gpu.encode_program(
            &mut encoder,
            &self.programs.scan_fsm,
            &[&input_buf, &fsm_buf, &input_len_buf],
            1,
        );

        // input:
        //     position: 00 01 02 03 04 05 06 07 08 09 10 11 12 13 14 15 16 17 18 19 20
        //     byte:      {  "  f  o  o  "  : sp  "  b  a  r  \  "  b  a  z  \  "  "  }
        //     state:     n  s  s  s  s  n  n  n  s  s  s  s  e  s  s  s  s  e  s  n  n
        //
        // output:
        //     compact: [0, 1, 6, 8, 20]
        //     num_structual: 5
        //
        // interpretation:
        //     tokens:  [{, "foo", :, "bar\"baz\"", }]
        //
        //     only structural bytes and token starts survive compaction
        self.gpu.encode_program(
            &mut encoder,
            &self.programs.scan_structural,
            &[
                &input_buf,
                &fsm_buf,
                &compact_buf,
                &num_structual_buf,
                &input_len_buf,
            ],
            1,
        );

        // input:
        //     json:    {"foo": "bar\"baz\""}
        //     compact: [0, 1, 6, 8, 20]
        //     tokens:  [{, "foo", :, "bar\"baz\"", }]
        //
        // output:
        //     index:   [ 0,     1, 2,            3, 4]
        //     depth:   [ 1,     1, 1,            1, 0]
        //
        //     an opener increments the running depth and a closer decrements it
        self.gpu.encode_program(
            &mut encoder,
            &self.programs.scan_depth,
            &[&input_buf, &compact_buf, &depth_buf, &num_structual_buf],
            1,
        );

        // input:
        //     json:    {"foo": "bar\"baz\""}
        //     compact: [0, 1, 6, 8, 20]
        //     num_structual: 5
        //     tokens:  [{, "foo", :, "bar\"baz\"", }]
        //
        // output:
        //     index:        [ 0,     1, 2,            3, 4]
        //     parents:      [-1,     0, 0,            0, 0]
        //     parent_error: 0
        //
        //     parent indices refer to the compact token list, not byte positions
        //     every token except the root is contained by token 0, the opening brace
        //     parent_error remains zero because the opening and closing braces match
        self.gpu.encode_program(
            &mut encoder,
            &self.programs.parent_link,
            &[
                &input_buf,
                &compact_buf,
                &num_structual_buf,
                &parent_buf,
                &parent_summary_a_buf,
                &parent_summary_b_buf,
                &parent_error_buf,
            ],
            1,
        );

        // input:
        //     json:    {"foo": "bar\"baz\""}
        //     state:   [n, s, s, s, s, n, n, n, s, s, s, s, e, s, s, s, s, e, s, n, n]
        //     compact: [0, 1, 6, 8, 20]
        //     tokens:  [{, "foo", :, "bar\"baz\"", }]
        //     depth:   [1, 1, 1, 1, 0]
        //     parents: [-1, 0, 0, 0, 0]
        //
        // output:
        //     tape: [
        //         { byte_pos: 0,  byte_end: 1,  depth: 1, parent: -1, kind: left_brace },
        //         { byte_pos: 1,  byte_end: 6,  depth: 1, parent: 0,  kind: string },
        //         { byte_pos: 6,  byte_end: 7,  depth: 1, parent: 0,  kind: colon },
        //         { byte_pos: 8,  byte_end: 20, depth: 1, parent: 0,  kind: string },
        //         { byte_pos: 20, byte_end: 21, depth: 0, parent: 0,  kind: right_brace },
        //     ]
        //
        //     byte ranges are half-open, so byte_pos is included and byte_end is excluded
        //     the fsm states let the string scan skip escaped quotes at byte positions 13 and 18
        self.gpu.encode_program(
            &mut encoder,
            &self.programs.assemble_tape,
            &[
                &input_buf,
                &compact_buf,
                &depth_buf,
                &parent_buf,
                &tape_buf,
                &fsm_buf,
                &input_len_buf,
                &num_structual_buf,
            ],
            1,
        );

        let num_structual_staging = self
            .gpu
            .encode_copy_to_staging(&mut encoder, &num_structual_buf);
        let tape_staging = self.gpu.encode_copy_to_staging(&mut encoder, &tape_buf);
        let parent_error_staging = self
            .gpu
            .encode_copy_to_staging(&mut encoder, &parent_error_buf);

        self.gpu.submit(encoder);

        let reads =
            self.gpu
                .read_stagings(&[&num_structual_staging, &tape_staging, &parent_error_staging]);

        let num_structual = usize::try_from(bytemuck::cast_slice::<u8, u32>(&reads[0])[0])
            .expect("num_structual must fit in usize");

        let parent_error = bytemuck::cast_slice::<u8, u32>(&reads[2])[0];

        ensure!(parent_error == 0, "mismatched json delimiters");

        let tape = bytemuck::cast_slice::<u8, TapeEntry>(&reads[1]);

        Ok(Tape::new(tape[..num_structual].to_vec()))
    }
}

impl Programs {
    fn compile(gpu: &Gpu) -> Self {
        let scan_fsm = gpu.compile_program(
            include_str!("shaders/scan_fsm.wgsl"),
            "main",
            &[true, false, true],
        );

        let scan_structural = gpu.compile_program(
            include_str!("shaders/scan_structural.wgsl"),
            "main",
            &[true, true, false, false, true],
        );

        let scan_depth = gpu.compile_program(
            include_str!("shaders/scan_depth.wgsl"),
            "main",
            &[true, true, false, true],
        );

        let parent_link = gpu.compile_program(
            include_str!("shaders/parent_link.wgsl"),
            "main",
            &[true, true, true, false, false, false, false],
        );

        let assemble_tape = gpu.compile_program(
            include_str!("shaders/assemble_tape.wgsl"),
            "main",
            &[true, true, true, true, false, true, true, true],
        );

        Self {
            scan_fsm,
            scan_structural,
            scan_depth,
            parent_link,
            assemble_tape,
        }
    }
}

fn buffer_size(element_size: usize) -> u64 {
    u64::try_from(MAX_INPUT_BYTES * element_size).expect("buffer size must fit in u64")
}

fn count_buffer_size() -> u64 {
    u64::try_from(std::mem::size_of::<u32>()).expect("count buffer size must fit in u64")
}

fn parent_summary_buffer_size() -> u64 {
    u64::try_from(MAX_INPUT_BYTES * PARENT_SUMMARY_SIZE)
        .expect("parent summary buffer size fits in u64")
}

#[cfg(test)]
mod tests {
    use crate::Parser;

    #[test]
    fn test_basic_json() {
        let Ok(parser) = Parser::try_new() else {
            return;
        };

        let tape = parser.parse_str(r#"{"foo":"bar"}"#).unwrap();
        let entries = tape.iter().copied().collect::<Vec<_>>();

        insta::assert_debug_snapshot!(&entries, @"
        [
            TapeEntry {
                byte_pos: 0,
                byte_end: 1,
                depth: 1,
                parent: -1,
                token_kind: LeftBrace,
            },
            TapeEntry {
                byte_pos: 1,
                byte_end: 6,
                depth: 1,
                parent: 0,
                token_kind: String,
            },
            TapeEntry {
                byte_pos: 6,
                byte_end: 7,
                depth: 1,
                parent: 0,
                token_kind: Colon,
            },
            TapeEntry {
                byte_pos: 7,
                byte_end: 12,
                depth: 1,
                parent: 0,
                token_kind: String,
            },
            TapeEntry {
                byte_pos: 12,
                byte_end: 13,
                depth: 0,
                parent: 0,
                token_kind: RightBrace,
            },
        ]
        ");
    }

    #[test]
    fn test_mismatched_delimiters_error() {
        let Ok(parser) = Parser::try_new() else {
            return;
        };

        let err = parser.parse_str("{]").unwrap_err();

        assert_eq!(err.to_string(), "mismatched json delimiters");
    }
}
