use anyhow::Result;
use wgpu::util::DeviceExt;

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Gpu {
    pub fn try_new() -> Result<Self> {
        pollster::block_on(Self::init())
    }

    pub fn new() -> Self {
        Self::try_new().expect("failed to initialize GPU")
    }

    async fn init() -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await?;

        let info = adapter.get_info();
        eprintln!("gpu: {} ({:?})", info.name, info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;

        Ok(Self { device, queue })
    }

    pub fn storage_buffer(&self, label: &str, data: &[u8]) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: data,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
    }

    pub fn storage_buffer_empty(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn staging_buffer(&self, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn create_encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None })
    }

    pub fn submit(&self, encoder: wgpu::CommandEncoder) {
        self.queue.submit(Some(encoder.finish()));
    }

    pub fn encode_copy_to_staging(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
    ) -> wgpu::Buffer {
        let staging = self.staging_buffer(buffer.size());
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
        staging
    }

    pub fn read_stagings(&self, stagings: &[&wgpu::Buffer]) -> Vec<Vec<u8>> {
        let receivers = stagings
            .iter()
            .map(|s| {
                let slice = s.slice(..);
                let (tx, rx) = std::sync::mpsc::channel();
                slice.map_async(wgpu::MapMode::Read, move |result| {
                    tx.send(result).unwrap();
                });
                rx
            })
            .collect::<Vec<_>>();

        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device polling must succeed");

        for rx in &receivers {
            rx.recv().unwrap().unwrap();
        }

        stagings
            .iter()
            .map(|s| {
                s.slice(..)
                    .get_mapped_range()
                    .expect("staging buffer must be mapped")
                    .to_vec()
            })
            .collect::<Vec<_>>()
    }

    #[cfg(test)]
    fn read_buffer(&self, buffer: &wgpu::Buffer) -> Vec<u8> {
        let mut encoder = self.create_encoder();
        let staging = self.encode_copy_to_staging(&mut encoder, buffer);
        self.submit(encoder);

        self.read_stagings(&[&staging]).into_iter().next().unwrap()
    }

    #[cfg(test)]
    fn read_buffer_as<T: bytemuck::Pod>(&self, buffer: &wgpu::Buffer) -> Vec<T> {
        let bytes = self.read_buffer(buffer);
        bytemuck::cast_slice(&bytes).to_vec()
    }

    pub fn compile_program(&self, shader_src: &str, entry_point: &str) -> wgpu::ComputePipeline {
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(shader_src.into()),
            });

        self.device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                // let wgsl autoinfer which storage buffers are read vs. read write
                layout: None,
                module: &module,
                entry_point: Some(entry_point),
                cache: None,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            })
    }

    pub fn encode_program(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        program: &wgpu::ComputePipeline,
        buffers: &[&wgpu::Buffer],
        workgroups: u32,
    ) {
        let bind_group_entries = buffers
            .iter()
            .enumerate()
            .map(|(i, buf)| wgpu::BindGroupEntry {
                binding: i.try_into().unwrap(),
                resource: buf.as_entire_binding(),
            })
            .collect::<Vec<_>>();

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &program.get_bind_group_layout(0),
            entries: &bind_group_entries,
        });

        let mut cpass = encoder.begin_compute_pass(&Default::default());
        cpass.set_pipeline(program);
        cpass.set_bind_group(0, &bind_group, &[]);
        cpass.dispatch_workgroups(workgroups, 1, 1);
    }

    #[cfg(test)]
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        shader_src: &str,
        entry_point: &str,
        buffers: &[&wgpu::Buffer],
        workgroups: u32,
    ) {
        let program = self.compile_program(shader_src, entry_point);
        self.encode_program(encoder, &program, &buffers, workgroups);
    }

    #[cfg(test)]
    fn dispatch(
        &self,
        shader_src: &str,
        entry_point: &str,
        buffers: &[&wgpu::Buffer],
        workgroups: u32,
    ) {
        let mut encoder = self.create_encoder();
        self.encode(&mut encoder, shader_src, entry_point, buffers, workgroups);
        self.submit(encoder);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fsm_string_detect() {
        let gpu = match Gpu::try_new() {
            Ok(g) => g,
            Err(_) => return,
        };

        let input_str = r#"hello "world" end"#;

        let mut input = input_str.bytes().collect::<Vec<u8>>();
        input.resize(input_str.len().next_multiple_of(4), 0);

        let input_buf = gpu.storage_buffer("fsm_in", &input);
        let input_len_buf = gpu.storage_buffer(
            "input_len",
            bytemuck::cast_slice(&[u32::try_from(input_str.len()).unwrap()]),
        );
        let output_buf =
            gpu.storage_buffer_empty("fsm_out", (256 * std::mem::size_of::<[u32; 4]>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_fsm.wgsl"),
            "main",
            &[&input_buf, &output_buf, &input_len_buf],
            1,
        );

        let out = gpu.read_buffer_as::<[u32; 4]>(&output_buf);

        let states = out[..input_str.len()]
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>();

        //                  h  e  l  l  o     "  w  o  r  l  d  "     e  n  d
        assert_eq!(states, [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_fsm_escape() {
        let gpu = match Gpu::try_new() {
            Ok(g) => g,
            Err(_) => return,
        };

        // the \" inside the string should NOT close it
        let input_str = r#"hello "wo\"rld" end"#;

        let mut input = input_str.bytes().collect::<Vec<u8>>();
        input.resize(input_str.len().next_multiple_of(4), 0);

        let input_buf = gpu.storage_buffer("fsm_in", &input);
        let input_len_buf = gpu.storage_buffer(
            "input_len",
            bytemuck::cast_slice(&[u32::try_from(input_str.len()).unwrap()]),
        );
        let output_buf =
            gpu.storage_buffer_empty("fsm_out", (256 * std::mem::size_of::<[u32; 4]>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_fsm.wgsl"),
            "main",
            &[&input_buf, &output_buf, &input_len_buf],
            1,
        );

        let out = gpu.read_buffer_as::<[u32; 4]>(&output_buf);

        let states = out[..input_str.len()]
            .iter()
            .map(|f| f[0])
            .collect::<Vec<_>>();

        assert_eq!(
            states,
            // h   e  l  l  o     "  w  o  \  "  r  l  d  "     e  n  d
            [0u32, 0, 0, 0, 0, 0, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn test_depth() {
        let gpu = match Gpu::try_new() {
            Ok(g) => g,
            Err(_) => return,
        };

        // {"a":{"b":[1,2]}}
        let input_str = r#"{"a":{"b":[1,2]}}"#;

        let mut input = input_str.bytes().collect::<Vec<u8>>();
        input.resize(input_str.len().next_multiple_of(4), 0);

        let input_buf = gpu.storage_buffer("bytes", &input);
        let input_len_buf = gpu.storage_buffer(
            "input_len",
            bytemuck::cast_slice(&[u32::try_from(input_str.len()).unwrap()]),
        );
        let fsm_buf =
            gpu.storage_buffer_empty("fsm", (256 * std::mem::size_of::<[u32; 4]>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_fsm.wgsl"),
            "main",
            &[&input_buf, &fsm_buf, &input_len_buf],
            1,
        );

        let compact_buf =
            gpu.storage_buffer_empty("compact", (256 * std::mem::size_of::<u32>()) as u64);
        let parser_state_buf = gpu.storage_buffer("parser_state", bytemuck::cast_slice(&[0u32; 2]));

        gpu.dispatch(
            include_str!("shaders/scan_structural.wgsl"),
            "main",
            &[
                &input_buf,
                &fsm_buf,
                &compact_buf,
                &parser_state_buf,
                &input_len_buf,
            ],
            1,
        );

        let depth_buf =
            gpu.storage_buffer_empty("depth", (256 * std::mem::size_of::<i32>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_depth.wgsl"),
            "main",
            &[&input_buf, &compact_buf, &depth_buf, &parser_state_buf],
            1,
        );

        let depths = gpu.read_buffer_as::<i32>(&depth_buf);
        let parser_state = gpu.read_buffer_as::<u32>(&parser_state_buf);
        let structural_count =
            usize::try_from(parser_state[0]).expect("structural_count fits usize");

        assert_eq!(
            &depths[..structural_count],
            &[1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 2, 1, 0]
        );
    }

    #[test]
    fn test_parent_link() {
        let gpu = match Gpu::try_new() {
            Ok(g) => g,
            Err(_) => return,
        };

        let input_str = r#"{"a":{"b":[1,2]}}"#;

        let mut input = input_str.bytes().collect::<Vec<u8>>();
        input.resize(input_str.len().next_multiple_of(4), 0);

        let input_buf = gpu.storage_buffer("bytes", &input);
        let input_len_buf = gpu.storage_buffer(
            "input_len",
            bytemuck::cast_slice(&[u32::try_from(input_str.len()).unwrap()]),
        );
        let fsm_buf =
            gpu.storage_buffer_empty("fsm", (256 * std::mem::size_of::<[u32; 4]>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_fsm.wgsl"),
            "main",
            &[&input_buf, &fsm_buf, &input_len_buf],
            1,
        );

        let compact_buf =
            gpu.storage_buffer_empty("compact", (256 * std::mem::size_of::<u32>()) as u64);
        let parser_state_buf = gpu.storage_buffer("parser_state", bytemuck::cast_slice(&[0u32; 2]));

        gpu.dispatch(
            include_str!("shaders/scan_structural.wgsl"),
            "main",
            &[
                &input_buf,
                &fsm_buf,
                &compact_buf,
                &parser_state_buf,
                &input_len_buf,
            ],
            1,
        );

        let depth_buf =
            gpu.storage_buffer_empty("depth", (256 * std::mem::size_of::<i32>()) as u64);

        gpu.dispatch(
            include_str!("shaders/scan_depth.wgsl"),
            "main",
            &[&input_buf, &compact_buf, &depth_buf, &parser_state_buf],
            1,
        );

        let parent_buf =
            gpu.storage_buffer_empty("parents", (256 * std::mem::size_of::<i32>()) as u64);
        let summary_size = (2 * std::mem::size_of::<u32>())
            + (256 * ((2 * std::mem::size_of::<u32>()) + std::mem::size_of::<i32>()));
        let summary_buffer_size =
            u64::try_from(256 * summary_size).expect("summary buffer size fits u64");
        let summary_a_buf = gpu.storage_buffer_empty("parent_summaries_a", summary_buffer_size);
        let summary_b_buf = gpu.storage_buffer_empty("parent_summaries_b", summary_buffer_size);
        gpu.dispatch(
            include_str!("shaders/parent_link.wgsl"),
            "main",
            &[
                &input_buf,
                &compact_buf,
                &parent_buf,
                &summary_a_buf,
                &summary_b_buf,
                &parser_state_buf,
            ],
            1,
        );

        let parents = gpu.read_buffer_as::<i32>(&parent_buf);
        let depths = gpu.read_buffer_as::<i32>(&depth_buf);
        let positions = gpu.read_buffer_as::<u32>(&compact_buf);
        let parser_state = gpu.read_buffer_as::<u32>(&parser_state_buf);
        let structural_count =
            usize::try_from(parser_state[0]).expect("structural_count fits usize");

        assert_eq!(parser_state[1], 0);
        assert_eq!(parents[0], -1);
        assert_eq!(
            &positions[..structural_count],
            &[0, 1, 4, 5, 6, 9, 10, 11, 12, 13, 14, 15, 16]
        );
        assert_eq!(
            &depths[..structural_count],
            &[1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 2, 1, 0]
        );
        assert_eq!(
            &parents[..structural_count],
            &[-1, 0, 0, 0, 3, 3, 3, 6, 6, 6, 6, 3, 0]
        );
    }
}
