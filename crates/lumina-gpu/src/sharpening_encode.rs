//! Encoding for the GPU sharpening post-tone stage.

use crate::{stages, GpuContext, GpuError, GpuResources, PostPipelineState};

impl GpuContext {
    /// Encode the multi-pass sharpening stage (GPU-RENDER-PARITY-1 stage 2)
    /// for the color texture `input_view`, writing the final pixels into `dst`.
    ///
    /// Two horizontal Gaussian blur passes produce the separable `tmp` rows for
    /// the fine/coarse radii; when `masking != 0` a compute pass reduces the
    /// luminance-gradient maximum (`maxg`) into a single cell that is read back
    /// (the one host round-trip) — otherwise `maxg` is irrelevant and the
    /// readback is skipped. The apply pass then performs the vertical blur on
    /// the fly and the oracle's ratio, exactly like `apply_sharpening`.
    pub(super) fn encode_sharpening(
        &self,
        resources: &GpuResources,
        post: &PostPipelineState,
        size: (u32, u32),
        params: &stages::SharpenParams,
        input_view: &wgpu::TextureView,
        dst: &wgpu::TextureView,
    ) -> Result<(), GpuError> {
        let (width, height) = size;
        let fine = stages::create_sharpen_scalar_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-sharpen-fine",
        );
        let fine_view = fine.create_view(&wgpu::TextureViewDescriptor::default());
        let coarse = stages::create_sharpen_scalar_texture(
            &resources.device,
            width,
            height,
            "lumina-gpu-sharpen-coarse",
        );
        let coarse_view = coarse.create_view(&wgpu::TextureViewDescriptor::default());

        let max_cell = stages::create_sharpen_gradient_cell(&resources.device);
        resources
            .queue
            .write_buffer(&max_cell, 0, &0u32.to_le_bytes());
        let staging = resources.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lumina-gpu-sharpen-maxg-readback"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-sharpen-pre"),
                });
        // Horizontal fine kernel (base 0). A dedicated uniform buffer per pass:
        // the two blur passes share one encoder, so a shared buffer would make
        // the second `write_buffer` retroactively change the first draw.
        let select_fine = stages::create_sharpen_blur_select_buffer(&resources.device);
        stages::write_sharpen_blur_select(
            &resources.queue,
            &select_fine,
            &stages::SharpenBlurSelect {
                base: 0,
                radius: params.fine_radius,
                _pad: [0; 2],
            },
        );
        let bind_fine = stages::create_sharpen_blur_bind_group(
            &resources.device,
            &post.sharpen_blur_layout,
            &post.sharpen_params,
            input_view,
            &select_fine,
        );
        super::encode_fullscreen_pass(
            &mut encoder,
            &post.sharpen_blur_pipeline,
            &bind_fine,
            &fine_view,
        );
        // Horizontal coarse kernel (second half of the storage array).
        let select_coarse = stages::create_sharpen_blur_select_buffer(&resources.device);
        stages::write_sharpen_blur_select(
            &resources.queue,
            &select_coarse,
            &stages::SharpenBlurSelect {
                base: stages::MAX_SHARPEN_TAPS as u32,
                radius: params.coarse_radius,
                _pad: [0; 2],
            },
        );
        let bind_coarse = stages::create_sharpen_blur_bind_group(
            &resources.device,
            &post.sharpen_blur_layout,
            &post.sharpen_params,
            input_view,
            &select_coarse,
        );
        super::encode_fullscreen_pass(
            &mut encoder,
            &post.sharpen_blur_pipeline,
            &bind_coarse,
            &coarse_view,
        );

        if params.masking != 0.0 {
            // The `masking` edge term needs the global gradient maximum; reduce
            // it on the GPU and read the single cell back.
            let bind_gradient = stages::create_sharpen_gradient_bind_group(
                &resources.device,
                &post.sharpen_gradient_layout,
                input_view,
                &max_cell,
            );
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("lumina-gpu-sharpen-gradient"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&post.sharpen_gradient_pipeline);
                pass.set_bind_group(0, &bind_gradient, &[]);
                pass.dispatch_workgroups(width.div_ceil(16), height.div_ceil(16), 1);
            }
            encoder.copy_buffer_to_buffer(&max_cell, 0, &staging, 0, 4);
            resources.queue.submit(Some(encoder.finish()));
            let maxg = super::readback_gradient_max(resources, &staging)?;
            stages::write_sharpen_maxg(&resources.queue, &post.sharpen_maxg, maxg);
        } else {
            // `masking == 0` makes the edge term irrelevant; skip the reduction
            // and its readback.
            resources.queue.submit(Some(encoder.finish()));
            stages::write_sharpen_maxg(&resources.queue, &post.sharpen_maxg, 1.0);
        }

        let mut apply_encoder =
            resources
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("lumina-gpu-sharpen-apply"),
                });
        let bind_apply = stages::create_sharpen_apply_bind_group(
            &resources.device,
            &post.sharpen_apply_layout,
            &post.sharpen_params,
            input_view,
            &fine_view,
            &coarse_view,
            &post.sharpen_maxg,
        );
        super::encode_fullscreen_pass(
            &mut apply_encoder,
            &post.sharpen_apply_pipeline,
            &bind_apply,
            dst,
        );
        resources.queue.submit(Some(apply_encoder.finish()));
        Ok(())
    }
}
