use lumina_onnx::{
    birefnet_manifest, ChannelLayout, ModelCapabilities, TensorFormat, BIREFNET_INFERENCE_HEIGHT,
    BIREFNET_INFERENCE_WIDTH,
};

#[test]
fn birefnet_has_subject_capability_and_no_prompts() {
    let m = birefnet_manifest();
    assert_eq!(m.model_name, "BiRefNet");
    assert_eq!(m.license, "Apache-2.0");

    let c: ModelCapabilities = m.capabilities;
    assert!(c.subject_segmentation);
    assert!(!c.box_prompt);
    assert!(!c.point_prompt);
    assert!(!c.mask_prompt);
    assert!(!c.class_detection);
    assert!(!c.instance_segmentation);

    // at least one capability set -> valid
    assert!(m.capabilities.validate().is_ok());

    assert_eq!(m.input.resolution.width, BIREFNET_INFERENCE_WIDTH);
    assert_eq!(m.input.resolution.height, BIREFNET_INFERENCE_HEIGHT);
    assert_eq!(m.input.channel_layout, ChannelLayout::Rgb);
    assert_eq!(m.input.tensor_format, TensorFormat::Nchw);
    assert_eq!(m.input.tensor_name, "input");
}
