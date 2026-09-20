# ML Models & ONNX Runtime — License Inventory and Source Pointers

**Status:** Model *weights* are **not** committed to this repository and are not
bundled by default (`model_hash = "pending-integration"` for BiRefNet/SAM 2
until hash-pinned fixtures land, see `feature/quality/fixtures-licensing.md` §5;
the face descriptors YuNet/SFace carry **verified** `sha256:<hex>` pins). The
license texts below must be shipped **with any bundle that distributes weights
or a binary that embeds/downloads them**.

| Component | Role | License (verified) | Text in this directory |
| --- | --- | --- | --- |
| BiRefNet (`ZhengPeng7/BiRefNet`) | automatic subject segmentation | **MIT** © 2024 ZhengPeng (GitHub `LICENSE` + HF model card `license: mit`) | [`BiRefNet-LICENSE-MIT.txt`](BiRefNet-LICENSE-MIT.txt) |
| SAM 2.1 (`facebookresearch/sam2`, `sam2.1_hiera_*`) | promptable segmentation (F-082) | **Apache-2.0** for code **and** weights (Meta announcement + repo `LICENSE`) | [`SAM-2-LICENSE-Apache-2.0.txt`](SAM-2-LICENSE-Apache-2.0.txt) |
| YuNet (`opencv/opencv_zoo`, `face_detection_yunet`) | face detection (LRPAR-G12-FACE-20) | **MIT** © 2020 Shiqi Yu — model-dir `LICENSE` + README "all files in this directory" (verified 2026-09-20) | [`YuNet-LICENSE-MIT.txt`](YuNet-LICENSE-MIT.txt) |
| SFace (`opencv/opencv_zoo`, `face_recognition_sface`, MobileFaceNet) | face embedding (LRPAR-G12-FACE-20) | **Apache-2.0** © 2021 Shenzhen Institute of AI and Robotics for Society — model-dir `LICENSE` + README "all files in this directory" (verified 2026-09-20) | [`SFace-LICENSE-Apache-2.0.txt`](SFace-LICENSE-Apache-2.0.txt) |
| ONNX Runtime (`microsoft/onnxruntime`) | inference runtime (prebuilt binaries fetched by `ort-sys` at build time; optional `onnx-rt` feature) | **MIT** © Microsoft Corporation (repo `LICENSE`); Rust bindings `ort`/`ort-sys` = `MIT OR Apache-2.0` | [`ONNXRuntime-LICENSE-MIT.txt`](ONNXRuntime-LICENSE-MIT.txt) |

## Source / provenance pointers

- BiRefNet: <https://github.com/ZhengPeng7/BiRefNet> ·
  <https://huggingface.co/ZhengPeng7/BiRefNet>
- SAM 2: <https://github.com/facebookresearch/sam2> · checkpoints published by
  Meta AI (<https://ai.meta.com/sam2/>)
- YuNet: <https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet>
  · weight `face_detection_yunet_2023mar.onnx`, 232 589 bytes, SHA-256
  `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4` (Git LFS
  object id; verified 2026-09-20). Model source:
  <https://github.com/ShiqiYu/libfacedetection.train>; paper
  <https://link.springer.com/article/10.1007/s11633-023-1423-y>.
- SFace: <https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface>
  · weight `face_recognition_sface_2021dec.onnx`, 38 696 353 bytes, SHA-256
  `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79` (Git LFS
  object id; verified 2026-09-20). Model source: <https://github.com/zhongyy/SFace>;
  paper <https://arxiv.org/abs/2205.12010>.
- ONNX Runtime: <https://github.com/microsoft/onnxruntime> ·
  prebuilt release binaries: <https://github.com/microsoft/onnxruntime/releases>
  (Redistribution under MIT is permitted with the included copyright/license
  notice; re-check the actual download channel at release time if it changes —
  open item R4.)

## Export-path constraint (AGPL trap)

The SAM 2.1 ONNX export **must not** go through the PyPI package
`ultralytics` (**AGPL-3.0**) — see `feature/quality/fixtures-licensing.md` §5.
Only the Apache-2.0-compliant path is allowed: Meta checkpoints via the
MIT-licensed Microsoft ORT export tooling or Apache-2.0 community artifacts.

## NOTICE-file status

Apache-2.0 §4(d) requires forwarding a `NOTICE` file **if one exists**. The
`facebookresearch/sam2` repository ships no separate `NOTICE` file
(checked 2026-08-25), so nothing additional has to be forwarded for SAM 2.
Re-check when pinning specific weight versions.

## Distribution obligations checklist

- [ ] When weights are first bundled (F-048): add the exact model version +
      content hash to `lumina-onnx` manifests and ship this directory with the
      bundle.
- [x] License texts present in this directory (MIT ×3, Apache-2.0 ×2).
- [x] Face descriptors YuNet/SFace carry verified `sha256:<hex>` pins in
      `lumina-onnx` (`FACE_DETECT_MODEL_HASH` / `FACE_EMBED_MODEL_HASH`,
      FACE-20-S6, 2026-09-20). No weight binaries are committed; a bundle that
      ships them must include this directory and match the pinned hashes.
