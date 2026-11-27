use std::fs;

use attool_lib::ecommerce::{
    models::{
        BatchRunMode, BatchTaskInput, BatchVariantInput, ImageFit, ImageLayerData, ShapeKind,
        ShapeLayerData, TemplateAsset, TemplateLayer, TemplateLayerType, TemplateProject,
        TextAlign, TextDecoration, TextLayerData,
    },
    render::run_batch_tasks,
    storage::EcommerceStore,
};

fn text_layer(id: &str, text: TextLayerData) -> TemplateLayer {
    TemplateLayer {
        id: id.to_string(),
        name: id.to_string(),
        r#type: TemplateLayerType::Text,
        x: 8.0,
        y: 48.0,
        width: 110.0,
        height: 40.0,
        visible: true,
        opacity: 1.0,
        rotation: 0.0,
        binding_key: None,
        locked: Some(false),
        children: None,
        text: Some(text),
        image: None,
        shape: None,
    }
}

fn sample_text() -> TextLayerData {
    TextLayerData {
        text: "默认标题".to_string(),
        font_family: "PingFang SC".to_string(),
        font_size: 18.0,
        font_weight: serde_json::json!(700),
        color: "#ffffff".to_string(),
        stroke_color: None,
        stroke_width: None,
        letter_spacing: None,
        line_height: None,
        align: Some(TextAlign::Left),
        font_style: None,
        text_decoration: None,
        orientation: None,
        background_color: None,
        background_radius: None,
        shadow_color: None,
        shadow_blur: None,
        shadow_offset_x: None,
        shadow_offset_y: None,
    }
}

fn project(
    id: &str,
    width: u32,
    height: u32,
    layers: Vec<TemplateLayer>,
    assets: Vec<TemplateAsset>,
) -> TemplateProject {
    TemplateProject {
        id: id.to_string(),
        name: id.to_string(),
        canvas_width: width,
        canvas_height: height,
        layers,
        assets,
        source_psd_path: None,
        preview_path: None,
        created_at: "2026-05-07 00:00:00".to_string(),
        updated_at: "2026-05-07 00:00:00".to_string(),
    }
}

#[test]
fn renders_text_background_shadow_and_decoration() {
    let root = tempfile::tempdir().unwrap();
    let store = EcommerceStore::new(root.path().join("store")).unwrap();
    let mut text = sample_text();
    text.text = "SALE".to_string();
    text.font_size = 34.0;
    text.font_weight = serde_json::json!(900);
    text.color = "#111111".to_string();
    text.stroke_color = Some("#ffffff".to_string());
    text.stroke_width = Some(2.0);
    text.text_decoration = Some(TextDecoration::Underline);
    text.background_color = Some("#ffdd66".to_string());
    text.background_radius = Some(10.0);
    text.shadow_color = Some("#000000".to_string());
    text.shadow_blur = Some(0.0);
    text.shadow_offset_x = Some(5.0);
    text.shadow_offset_y = Some(5.0);
    let mut layer = text_layer("text", text);
    layer.x = 30.0;
    layer.y = 36.0;
    layer.width = 180.0;
    layer.height = 72.0;
    store
        .save_template(project("styled-text", 240, 140, vec![layer], vec![]))
        .unwrap();

    let outputs = run_batch_tasks(
        &store,
        "styled-text",
        &[BatchTaskInput {
            layer_id: "text".to_string(),
            variants: vec![BatchVariantInput::Text {
                value: "SALE".to_string(),
            }],
        }],
        BatchRunMode::Product,
    )
    .unwrap();

    let image = image::open(&outputs[0].file_path).unwrap().to_rgba8();
    assert_eq!(
        image.get_pixel(34, 40).0,
        [255, 221, 102, 255],
        "background should be rendered"
    );
    assert_ne!(
        image.get_pixel(70, 80).0,
        [255, 255, 255, 255],
        "text or decoration should affect pixels"
    );
    assert_ne!(
        image.get_pixel(120, 98).0,
        [255, 255, 255, 255],
        "underline should affect pixels below text"
    );
}

#[test]
fn renders_shape_stroke_and_contained_image() {
    let root = tempfile::tempdir().unwrap();
    let image_path = root.path().join("wide.png");
    image::RgbaImage::from_pixel(80, 20, image::Rgba([255, 0, 0, 255]))
        .save(&image_path)
        .unwrap();
    let store = EcommerceStore::new(root.path().join("store")).unwrap();
    let layers = vec![
        TemplateLayer {
            id: "shape".to_string(),
            name: "Shape".to_string(),
            r#type: TemplateLayerType::Shape,
            x: 10.0,
            y: 10.0,
            width: 50.0,
            height: 40.0,
            visible: true,
            opacity: 1.0,
            rotation: 0.0,
            binding_key: None,
            locked: None,
            children: None,
            text: None,
            image: None,
            shape: Some(ShapeLayerData {
                shape: ShapeKind::Rect,
                fill: Some("#00ff00".to_string()),
                stroke: Some("#0000ff".to_string()),
                stroke_width: Some(3.0),
                radius: None,
            }),
        },
        TemplateLayer {
            id: "image".to_string(),
            name: "Image".to_string(),
            r#type: TemplateLayerType::Image,
            x: 10.0,
            y: 70.0,
            width: 80.0,
            height: 40.0,
            visible: true,
            opacity: 1.0,
            rotation: 0.0,
            binding_key: None,
            locked: None,
            children: None,
            text: None,
            image: Some(ImageLayerData {
                asset_id: "unused-while-bound".to_string(),
                fit: ImageFit::Contain,
                replaceable: true,
            }),
            shape: None,
        },
        TemplateLayer {
            id: "outline".to_string(),
            name: "Outline".to_string(),
            r#type: TemplateLayerType::Shape,
            x: 90.0,
            y: 10.0,
            width: 20.0,
            height: 20.0,
            visible: true,
            opacity: 1.0,
            rotation: 0.0,
            binding_key: None,
            locked: None,
            children: None,
            text: None,
            image: None,
            shape: Some(ShapeLayerData {
                shape: ShapeKind::Rect,
                fill: None,
                stroke: Some("#ff0000".to_string()),
                stroke_width: Some(2.0),
                radius: None,
            }),
        },
    ];
    store
        .save_template(project("shape-image", 120, 120, layers, vec![]))
        .unwrap();

    let outputs = run_batch_tasks(
        &store,
        "shape-image",
        &[BatchTaskInput {
            layer_id: "image".to_string(),
            variants: vec![BatchVariantInput::Image {
                source_path: image_path.to_string_lossy().into_owned(),
            }],
        }],
        BatchRunMode::Product,
    )
    .unwrap();

    let exported = image::open(&outputs[0].file_path).unwrap().to_rgba8();
    assert_eq!(exported.get_pixel(10, 10).0, [0, 0, 255, 255]);
    assert_eq!(exported.get_pixel(30, 30).0, [0, 255, 0, 255]);
    assert_eq!(exported.get_pixel(90, 10).0, [255, 0, 0, 255]);
    assert_eq!(exported.get_pixel(100, 20).0, [255, 255, 255, 255]);
    assert_eq!(exported.get_pixel(10, 70).0, [255, 255, 255, 255]);
    assert_eq!(exported.get_pixel(10, 80).0, [255, 0, 0, 255]);
}

#[test]
fn renders_product_variants_with_store_owned_asset() {
    let root = tempfile::tempdir().unwrap();
    let source_path = root.path().join("input.png");
    image::RgbaImage::from_pixel(120, 120, image::Rgba([255, 0, 0, 255]))
        .save(&source_path)
        .unwrap();
    let store = EcommerceStore::new(root.path().join("store")).unwrap();
    let asset = store
        .save_asset(
            "input.png".to_string(),
            "image/png".to_string(),
            fs::read(&source_path).unwrap(),
        )
        .unwrap();
    let image_layer = TemplateLayer {
        id: "image".to_string(),
        name: "商品图".to_string(),
        r#type: TemplateLayerType::Image,
        x: 0.0,
        y: 0.0,
        width: 120.0,
        height: 120.0,
        visible: true,
        opacity: 1.0,
        rotation: 0.0,
        binding_key: None,
        locked: Some(false),
        children: None,
        text: None,
        image: Some(ImageLayerData {
            asset_id: asset.id.clone(),
            fit: ImageFit::Stretch,
            replaceable: true,
        }),
        shape: None,
    };
    store
        .save_template(project(
            "tpl-render",
            120,
            120,
            vec![image_layer, text_layer("title", sample_text())],
            vec![asset],
        ))
        .unwrap();

    let outputs = run_batch_tasks(
        &store,
        "tpl-render",
        &[BatchTaskInput {
            layer_id: "title".to_string(),
            variants: vec![
                BatchVariantInput::Text {
                    value: "红色单椅".to_string(),
                },
                BatchVariantInput::Text {
                    value: "红色双人椅".to_string(),
                },
            ],
        }],
        BatchRunMode::Product,
    )
    .unwrap();

    assert_eq!(outputs.len(), 2);
    assert!(outputs
        .iter()
        .all(|output| fs::metadata(&output.file_path).unwrap().is_file()));
    assert_eq!(outputs[0].file_name, "红色单椅.png");
    assert_eq!(outputs[1].file_name, "红色双人椅.png");
    let exported = image::open(&outputs[0].file_path).unwrap().to_rgba8();
    let non_red_pixels = exported
        .pixels()
        .filter(|pixel| pixel.0 != [255, 0, 0, 255])
        .count();
    assert!(
        non_red_pixels > 0,
        "text rendering should change pixels over the red image"
    );
}

#[test]
fn rejects_parallel_tasks_with_different_variant_counts() {
    let root = tempfile::tempdir().unwrap();
    let store = EcommerceStore::new(root.path().join("store")).unwrap();
    store
        .save_template(project(
            "parallel",
            120,
            120,
            vec![
                text_layer("title", sample_text()),
                text_layer("subtitle", sample_text()),
            ],
            vec![],
        ))
        .unwrap();

    let error = run_batch_tasks(
        &store,
        "parallel",
        &[
            BatchTaskInput {
                layer_id: "title".to_string(),
                variants: vec![
                    BatchVariantInput::Text {
                        value: "A".to_string(),
                    },
                    BatchVariantInput::Text {
                        value: "B".to_string(),
                    },
                ],
            },
            BatchTaskInput {
                layer_id: "subtitle".to_string(),
                variants: vec![BatchVariantInput::Text {
                    value: "C".to_string(),
                }],
            },
        ],
        BatchRunMode::Parallel,
    )
    .unwrap_err();

    assert_eq!(error, "1:1 模式要求所有任务的变体数相等");
}
